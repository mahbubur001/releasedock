//! ZIP inspection and safe extraction. Ported from `ZIPInspectionService.swift`.

use crate::error::{Error, PackageInspectionError};
use crate::file_classifier::{FileClassifier, GlobPattern};
use crate::path_evaluator::{InspectionOptions, PackagePathEvaluator, issue, issue_kind};
use crate::zip_archive::{ZipArchive, ZipEntry};
use rd_model::{
    EntryKind, InspectionIssue, InspectionIssueKind, PackageEntry, PackageInspection, SourceKind, SwiftDate,
    ValidationSeverity,
};
use rd_support::hashing;
use rd_support::path_safety::{self, Violation};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

pub trait ZipInspecting: Send + Sync {
    /// Inspects an archive without writing anything to disk.
    fn inspect(&self, zip: &Path, options: &InspectionOptions) -> Result<PackageInspection, Error>;
    /// Safely extracts the included entries of an inspected archive into an empty
    /// `destination`, stripping the archive root folder. Returns entries with SHA-256.
    fn extract(
        &self,
        zip: &Path,
        inspection: &PackageInspection,
        destination: &Path,
    ) -> Result<Vec<PackageEntry>, Error>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ZipInspectionService;

struct Candidate<'a> {
    components: Vec<String>,
    entry: &'a ZipEntry,
}

/// Standard base64 with padding, like `Data.base64EncodedString()`.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 0x3F) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(unix)]
fn create_dirs(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().recursive(true).mode(mode).create(path)
}

#[cfg(not(unix))]
fn create_dirs(path: &Path, _mode: u32) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

impl ZipInspectionService {
    pub fn new() -> Self {
        Self
    }

    /// Pure analysis of central-directory entries; separated for testing.
    pub fn analyze(
        &self,
        entries: &[ZipEntry],
        source_path: &str,
        sha256: &str,
        options: &InspectionOptions,
    ) -> PackageInspection {
        use InspectionIssueKind as Kind;
        use ValidationSeverity::{Error as E, Warning as W};

        let mut issues: Vec<InspectionIssue> = Vec::new();
        let mut total: u64 = 0;
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut seen_files: HashMap<String, String> = HashMap::new(); // lowercased path → original

        if entries.len() > options.maximum_entries {
            issues.push(issue(
                Kind::ArchiveTooLarge,
                E,
                None,
                format!(
                    "Archive has {} entries (limit {})",
                    entries.len(),
                    options.maximum_entries
                ),
            ));
        }

        for entry in entries {
            let Some(name) = entry.name.as_deref() else {
                issues.push(issue(
                    Kind::UnsafeFilename,
                    E,
                    Some(base64(&entry.raw_name)),
                    "File name is not valid UTF-8",
                ));
                continue;
            };
            let components = match path_safety::validate_relative_path(name) {
                Ok(components) => components,
                Err(violation) => {
                    issues.push(issue(
                        issue_kind(&violation),
                        E,
                        Some(name.into()),
                        violation.to_string(),
                    ));
                    continue;
                }
            };
            if entry.is_encrypted() {
                issues.push(issue(
                    Kind::EncryptedEntry,
                    E,
                    Some(name.into()),
                    "Encrypted entries are not supported",
                ));
                continue;
            }
            if entry.is_symlink() {
                issues.push(issue(
                    Kind::SymbolicLink,
                    E,
                    Some(name.into()),
                    "Symbolic links are not allowed in a release package",
                ));
                continue;
            }
            if entry.is_special_file() {
                issues.push(issue(
                    Kind::SpecialFile,
                    E,
                    Some(name.into()),
                    "Special files (devices, FIFOs, sockets) are not allowed",
                ));
                continue;
            }
            if !entry.is_directory() {
                if entry.method != 0 && entry.method != 8 {
                    issues.push(issue(
                        Kind::UnsupportedCompression,
                        E,
                        Some(name.into()),
                        format!("Unsupported compression method {}", entry.method),
                    ));
                    continue;
                }
                if entry.uncompressed_size > options.maximum_file_size {
                    issues.push(issue(
                        Kind::ArchiveTooLarge,
                        E,
                        Some(name.into()),
                        format!("File is larger than {} MiB", options.maximum_file_size >> 20),
                    ));
                }
                if entry.compressed_size > 0
                    && entry.uncompressed_size > 1 << 20
                    && entry.uncompressed_size as f64 / entry.compressed_size as f64 > options.maximum_compression_ratio
                {
                    issues.push(issue(
                        Kind::SuspiciousCompressionRatio,
                        E,
                        Some(name.into()),
                        "Suspicious compression ratio (possible zip bomb)",
                    ));
                }
                total = total.saturating_add(entry.uncompressed_size);
                let key = components.join("/").to_lowercase();
                if let Some(existing) = seen_files.get(&key) {
                    issues.push(issue(
                        Kind::DuplicatePath,
                        E,
                        Some(name.into()),
                        format!(
                            "Duplicate archive path (also \"{existing}\"; macOS file systems are case-insensitive)"
                        ),
                    ));
                    continue;
                }
                seen_files.insert(key, name.to_owned());
            }
            candidates.push(Candidate { components, entry });
        }

        if total > options.maximum_total_size {
            issues.push(issue(
                Kind::ArchiveTooLarge,
                E,
                None,
                format!(
                    "Uncompressed size {} MiB exceeds the {} MiB limit",
                    total >> 20,
                    options.maximum_total_size >> 20
                ),
            ));
        }

        // A file path that is also used as a directory path is a conflict.
        for candidate in &candidates {
            for depth in 1..candidate.components.len().max(1) {
                let ancestor = candidate.components[..depth].join("/").to_lowercase();
                if seen_files.contains_key(&ancestor) {
                    issues.push(issue(
                        Kind::DuplicatePath,
                        E,
                        Some(candidate.components.join("/")),
                        format!("Path uses \"{ancestor}\" as both a file and a directory"),
                    ));
                }
            }
        }

        // Determine the archive root, ignoring OS junk at the top level.
        const JUNK_TOP_LEVEL: [&str; 2] = ["__MACOSX", ".DS_Store"];
        let meaningful: Vec<&Candidate> = candidates
            .iter()
            .filter(|c| !JUNK_TOP_LEVEL.contains(&c.components[0].as_str()))
            .collect();
        let top_level: HashSet<&str> = meaningful.iter().map(|c| c.components[0].as_str()).collect();
        let mut root_name: Option<String> = None;
        if top_level.len() == 1
            && meaningful.iter().any(|c| c.components.len() > 1)
            && !meaningful
                .iter()
                .any(|c| c.components.len() == 1 && !c.entry.is_directory())
        {
            root_name = top_level.iter().next().map(|s| (*s).to_owned());
        } else if !meaningful.is_empty() {
            issues.push(issue(
                Kind::MultipleRoots,
                W,
                None,
                "Archive does not wrap its files in a single top-level folder; the archive root is used as the package root",
            ));
        }

        let patterns = options
            .exclude_patterns
            .iter()
            .filter_map(|p| GlobPattern::new(p))
            .collect();
        let mut evaluator = PackagePathEvaluator::new(FileClassifier::new(SourceKind::Zip, patterns));
        let mut included: Vec<PackageEntry> = Vec::new();
        let mut directories: BTreeSet<String> = BTreeSet::new();

        for candidate in &candidates {
            let mut components: &[String] = &candidate.components;
            if let Some(root_name) = &root_name {
                if &components[0] == root_name {
                    components = &components[1..];
                    if components.is_empty() {
                        continue; // the root folder itself
                    }
                }
                // Entries outside the root (e.g. __MACOSX/) keep their path and are classified as junk.
            }
            let path = components.join("/");
            let is_directory = candidate.entry.is_directory();
            if !evaluator.include(&path, is_directory) {
                continue;
            }
            for depth in 1..components.len().max(1) {
                directories.insert(components[..depth].join("/"));
            }
            if is_directory {
                directories.insert(path);
                continue;
            }
            if let Some(warning) = components.iter().find_map(|c| path_safety::portability_warning(c)) {
                evaluator.add_issue(issue(Kind::Portability, W, Some(path.clone()), warning));
            }
            if candidate.entry.uncompressed_size == 0 {
                evaluator.add_issue(issue(Kind::ZeroByteFile, W, Some(path.clone()), "Zero-byte file"));
            }
            included.push(PackageEntry {
                path,
                kind: EntryKind::File,
                size: candidate.entry.uncompressed_size,
                sha256: None,
            });
        }

        // A package root that holds nothing but one folder means an extra nesting level
        // (e.g. my-plugin/my-plugin/my-plugin.php).
        let has_root_files = included.iter().any(|e| !e.path.contains('/'));
        let root_directories: Vec<&String> = directories.iter().filter(|d| !d.contains('/')).collect();
        if !has_root_files && root_directories.len() == 1 {
            let nested = root_directories[0];
            evaluator.add_issue(issue(
                Kind::NestedRoot,
                E,
                Some(format!("{nested}/")),
                format!("Unexpected nested directory: the package root contains only the folder \"{nested}\""),
            ));
        }

        // BTreeSet iterates sorted, like `directories.sorted()`.
        included.extend(directories.into_iter().map(|path| PackageEntry {
            path,
            kind: EntryKind::Directory,
            size: 0,
            sha256: None,
        }));
        included.sort_by(|a, b| a.path.cmp(&b.path));
        let (evaluator_issues, excluded_paths) = evaluator.into_parts();
        issues.extend(evaluator_issues);
        PackageInspection {
            source_kind: SourceKind::Zip,
            source_path: source_path.to_owned(),
            sha256: sha256.to_owned(),
            archive_root_name: root_name,
            included_entries: included,
            excluded_paths,
            issues,
            total_uncompressed_size: total,
            inspected_at: SwiftDate::now(),
        }
    }
}

impl ZipInspecting for ZipInspectionService {
    fn inspect(&self, zip: &Path, options: &InspectionOptions) -> Result<PackageInspection, Error> {
        let sha = hashing::sha256_file(zip)?;
        let source_path = zip.display().to_string();
        let archive = match ZipArchive::open(zip) {
            Ok(archive) => archive,
            Err(Error::Zip(error)) => {
                return Ok(PackageInspection {
                    source_kind: SourceKind::Zip,
                    source_path,
                    sha256: sha,
                    archive_root_name: None,
                    included_entries: Vec::new(),
                    excluded_paths: Vec::new(),
                    issues: vec![issue(
                        InspectionIssueKind::CorruptArchive,
                        ValidationSeverity::Error,
                        None,
                        error.to_string(),
                    )],
                    total_uncompressed_size: 0,
                    inspected_at: SwiftDate::now(),
                });
            }
            Err(other) => return Err(other),
        };
        Ok(self.analyze(&archive.entries, &source_path, &sha, options))
    }

    fn extract(
        &self,
        zip: &Path,
        inspection: &PackageInspection,
        destination: &Path,
    ) -> Result<Vec<PackageEntry>, Error> {
        let blocking = inspection.blocking_issues();
        if !blocking.is_empty() {
            return Err(PackageInspectionError::BlockingIssues(blocking.into_iter().cloned().collect()).into());
        }
        if hashing::sha256_file(zip)? != inspection.sha256 {
            return Err(PackageInspectionError::SourceChanged(
                "ZIP checksum differs from the inspected archive".into(),
            )
            .into());
        }
        create_dirs(destination, 0o700)?;
        if fs::read_dir(destination)?.next().is_some() {
            return Err(PackageInspectionError::DestinationNotEmpty(destination.display().to_string()).into());
        }

        let archive = ZipArchive::open(zip)?;
        let mut by_path: HashMap<String, &ZipEntry> = HashMap::new();
        for entry in archive.entries.iter().filter(|e| !e.is_directory()) {
            let Some(name) = entry.name.as_deref() else { continue };
            let Ok(mut components) = path_safety::validate_relative_path(name) else {
                continue;
            };
            if let Some(root) = &inspection.archive_root_name {
                if components.first() != Some(root) {
                    continue;
                }
                components.remove(0);
            }
            by_path.insert(components.join("/"), entry);
        }

        let mut items: Vec<&PackageEntry> = inspection.included_entries.iter().collect();
        items.sort_by(|a, b| a.path.cmp(&b.path));
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            // Re-validate: never trust the stored inspection blindly.
            let components = path_safety::validate_relative_path(&item.path)?;
            let target = path_safety::resolve(&components, destination)?;
            match item.kind {
                EntryKind::Directory => {
                    create_dirs(&target, 0o755)?;
                    result.push(item.clone());
                }
                EntryKind::File => {
                    let Some(entry) = by_path.get(&item.path) else {
                        return Err(
                            PackageInspectionError::SourceChanged(format!("missing entry {}", item.path)).into(),
                        );
                    };
                    let parent = target.parent().unwrap_or(destination);
                    create_dirs(parent, 0o755)?;
                    // The parent must still resolve inside the destination (no symlinked parents).
                    if !path_safety::is_contained(parent, destination) {
                        return Err(Violation::EscapesRoot.into());
                    }
                    let sha = archive.extract(entry, &target)?;
                    let mut extracted = item.clone();
                    extracted.sha256 = Some(sha);
                    result.push(extracted);
                }
                EntryKind::Symlink => {
                    return Err(PackageInspectionError::BlockingIssues(vec![issue(
                        InspectionIssueKind::SymbolicLink,
                        ValidationSeverity::Error,
                        Some(item.path.clone()),
                        "Symbolic link",
                    )])
                    .into());
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_foundation() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xFF, 0xFE, 0x80, 0x00]), "//6AAA==");
    }
}
