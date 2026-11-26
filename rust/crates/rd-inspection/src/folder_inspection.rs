//! Read-only inspection of a source folder. Ported from `FolderInspectionService.swift`.

use crate::error::Error;
use crate::file_classifier::{FileClassifier, GlobPattern};
use crate::path_evaluator::{InspectionOptions, PackagePathEvaluator, issue, issue_kind};
use crate::text;
use rd_model::{
    EntryKind, InspectionIssueKind, PackageEntry, PackageInspection, SourceKind, SwiftDate, ValidationSeverity,
};
use rd_support::hashing;
use rd_support::path_safety::{self, Violation};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub trait FolderInspecting: Send + Sync {
    /// Walks the folder read-only and hashes every included file.
    fn inspect(&self, folder: &Path, options: &InspectionOptions) -> Result<PackageInspection, Error>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FolderInspectionService;

impl FolderInspectionService {
    pub fn new() -> Self {
        Self
    }

    /// Patterns marked `export-ignore` in a .gitattributes file (`/tests export-ignore`).
    pub fn export_ignore_patterns(text: &str) -> Vec<String> {
        text::lines(text)
            .filter_map(|line| {
                let trimmed = text::trim_whitespace(line);
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return None;
                }
                let parts: Vec<&str> = trimmed.split([' ', '\t']).filter(|p| !p.is_empty()).collect();
                if parts.len() >= 2 && parts[1..].contains(&"export-ignore") {
                    Some(parts[0].to_owned())
                } else {
                    None
                }
            })
            .collect()
    }
}

/// One item of the depth-first walk, like an `NSDirectoryEnumerator` element.
struct WalkItem {
    path: PathBuf,
    /// `/`-separated path relative to the root, or its lossy form for a non-UTF-8 name.
    relative: Result<String, String>,
}

/// Depth-first, pre-order directory walk that never follows symbolic links. Children are
/// visited in byte order of their names so results are deterministic on every platform.
/// Unreadable directories are skipped silently (the Swift enumerator's error handler
/// returns `true`).
struct Walker {
    stack: Vec<std::vec::IntoIter<WalkItem>>,
}

impl Walker {
    fn new(root: &Path) -> Self {
        let mut walker = Self { stack: Vec::new() };
        walker.descend(root, "");
        walker
    }

    /// Queues the children of `directory`, whose relative path is `parent`, to be visited next.
    fn descend(&mut self, directory: &Path, parent: &str) {
        let Ok(read) = fs::read_dir(directory) else { return };
        let mut names: Vec<_> = read.filter_map(Result::ok).map(|e| e.file_name()).collect();
        names.sort();
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        let items: Vec<WalkItem> = names
            .into_iter()
            .map(|name| WalkItem {
                path: directory.join(&name),
                relative: match name.to_str() {
                    Some(name) => Ok(format!("{prefix}{name}")),
                    None => Err(format!("{prefix}{}", name.to_string_lossy())),
                },
            })
            .collect();
        self.stack.push(items.into_iter());
    }

    fn next(&mut self) -> Option<WalkItem> {
        loop {
            let top = self.stack.last_mut()?;
            match top.next() {
                Some(item) => return Some(item),
                None => {
                    self.stack.pop();
                }
            }
        }
    }
}

impl FolderInspecting for FolderInspectionService {
    fn inspect(&self, folder: &Path, options: &InspectionOptions) -> Result<PackageInspection, Error> {
        let root = text::standardize(folder);
        let root_display = root.display().to_string();
        if !fs::metadata(&root).is_ok_and(|m| m.is_dir()) {
            return Err(Error::NotADirectory(root_display));
        }

        let mut patterns: Vec<GlobPattern> = options
            .exclude_patterns
            .iter()
            .filter_map(|p| GlobPattern::new(p))
            .collect();
        let mut pre_issues = Vec::new();
        if let Ok(text) = fs::read_to_string(root.join(".distignore")) {
            let parsed = GlobPattern::parse_list(&text);
            pre_issues.push(issue(
                InspectionIssueKind::DevelopmentArtifact,
                ValidationSeverity::Info,
                Some(".distignore".into()),
                format!("Applied {} exclusion pattern(s) from .distignore", parsed.len()),
            ));
            patterns.extend(parsed);
            if text::lines(&text).any(|line| text::trim_whitespace(line).starts_with('!')) {
                pre_issues.push(issue(
                    InspectionIssueKind::DevelopmentArtifact,
                    ValidationSeverity::Warning,
                    Some(".distignore".into()),
                    "Negated patterns (!) in .distignore are not supported and were ignored",
                ));
            }
        } else if let Ok(text) = fs::read_to_string(root.join(".gitattributes")) {
            // Without a .distignore, `export-ignore` attributes decide (as `git archive` and
            // 10up's deploy action do).
            let parsed: Vec<GlobPattern> = Self::export_ignore_patterns(&text)
                .iter()
                .filter_map(|p| GlobPattern::new(p))
                .collect();
            if !parsed.is_empty() {
                pre_issues.push(issue(
                    InspectionIssueKind::DevelopmentArtifact,
                    ValidationSeverity::Info,
                    Some(".gitattributes".into()),
                    format!("Applied {} export-ignore pattern(s) from .gitattributes", parsed.len()),
                ));
                patterns.extend(parsed);
            }
        }

        let mut evaluator = PackagePathEvaluator::new(FileClassifier::new(SourceKind::Folder, patterns));
        for pre in pre_issues {
            evaluator.add_issue(pre);
        }

        let mut walker = Walker::new(&root);
        let mut included: Vec<PackageEntry> = Vec::new();
        let mut total: u64 = 0;
        let mut count = 0usize;

        while let Some(item) = walker.next() {
            count += 1;
            if count > options.maximum_entries {
                evaluator.add_issue(issue(
                    InspectionIssueKind::ArchiveTooLarge,
                    ValidationSeverity::Error,
                    None,
                    format!("Folder has more than {} entries", options.maximum_entries),
                ));
                break;
            }
            // Not descending into a directory is `skipDescendants()`.
            let relative = match &item.relative {
                Ok(relative) => relative.clone(),
                Err(lossy) => {
                    let violation = Violation::InvalidUtf8;
                    evaluator.add_issue(issue(
                        issue_kind(&violation),
                        ValidationSeverity::Error,
                        Some(lossy.clone()),
                        violation.to_string(),
                    ));
                    continue;
                }
            };

            if let Err(violation) = path_safety::validate_relative_path(&relative) {
                evaluator.add_issue(issue(
                    issue_kind(&violation),
                    ValidationSeverity::Error,
                    Some(relative),
                    violation.to_string(),
                ));
                continue;
            }

            let metadata = fs::symlink_metadata(&item.path)?;
            let file_type = metadata.file_type();
            let is_symlink = file_type.is_symlink();
            let is_directory = file_type.is_dir() && !is_symlink;
            if !evaluator.include(&relative, is_directory) {
                continue;
            }

            if is_symlink {
                evaluator.add_issue(issue(
                    InspectionIssueKind::SymbolicLink,
                    ValidationSeverity::Error,
                    Some(relative),
                    "Symbolic links are not published; replace it with the real file or exclude it",
                ));
                continue;
            }
            if is_directory {
                walker.descend(&item.path, &relative);
                included.push(PackageEntry {
                    path: relative,
                    kind: EntryKind::Directory,
                    size: 0,
                    sha256: None,
                });
                continue;
            }
            if !file_type.is_file() {
                evaluator.add_issue(issue(
                    InspectionIssueKind::SpecialFile,
                    ValidationSeverity::Error,
                    Some(relative),
                    "Special files (devices, FIFOs, sockets) are not allowed",
                ));
                continue;
            }

            let size = metadata.len();
            total = total.saturating_add(size);
            if size > options.maximum_file_size {
                evaluator.add_issue(issue(
                    InspectionIssueKind::ArchiveTooLarge,
                    ValidationSeverity::Error,
                    Some(relative),
                    format!("File is larger than {} MiB", options.maximum_file_size >> 20),
                ));
                continue;
            }
            if size == 0 {
                evaluator.add_issue(issue(
                    InspectionIssueKind::ZeroByteFile,
                    ValidationSeverity::Warning,
                    Some(relative.clone()),
                    "Zero-byte file",
                ));
            }
            if let Some(warning) = relative
                .split('/')
                .filter(|c| !c.is_empty())
                .find_map(path_safety::portability_warning)
            {
                evaluator.add_issue(issue(
                    InspectionIssueKind::Portability,
                    ValidationSeverity::Warning,
                    Some(relative.clone()),
                    warning,
                ));
            }
            let sha = match hashing::sha256_file(&item.path) {
                Ok(sha) => sha,
                Err(error) => {
                    evaluator.add_issue(issue(
                        InspectionIssueKind::UnsafeFilename,
                        ValidationSeverity::Error,
                        Some(relative),
                        format!("File cannot be read: {error}"),
                    ));
                    continue;
                }
            };
            included.push(PackageEntry {
                path: relative,
                kind: EntryKind::File,
                size,
                sha256: Some(sha),
            });
        }

        if total > options.maximum_total_size {
            evaluator.add_issue(issue(
                InspectionIssueKind::ArchiveTooLarge,
                ValidationSeverity::Error,
                None,
                format!(
                    "Folder size {} MiB exceeds the {} MiB limit",
                    total >> 20,
                    options.maximum_total_size >> 20
                ),
            ));
        }

        // Case-insensitive duplicates cannot exist on a default macOS volume, but can on
        // case-sensitive volumes and would collide in SVN checkouts on macOS/Windows.
        let mut seen: HashMap<String, String> = HashMap::new();
        for entry in &included {
            let key = entry.path.to_lowercase();
            if let Some(other) = seen.get(&key) {
                evaluator.add_issue(issue(
                    InspectionIssueKind::DuplicatePath,
                    ValidationSeverity::Error,
                    Some(entry.path.clone()),
                    format!("Differs only by case from \"{other}\""),
                ));
            }
            seen.insert(key, entry.path.clone());
        }

        let digest = hashing::tree_digest(
            included
                .iter()
                .filter(|e| e.kind == EntryKind::File)
                .map(|e| (e.path.as_str(), e.sha256.as_deref().unwrap_or(""))),
        );
        included.sort_by(|a, b| a.path.cmp(&b.path));
        let (issues, excluded_paths) = evaluator.into_parts();
        Ok(PackageInspection {
            source_kind: SourceKind::Folder,
            source_path: root_display,
            sha256: digest,
            archive_root_name: None,
            included_entries: included,
            excluded_paths,
            issues,
            total_uncompressed_size: total,
            inspected_at: SwiftDate::now(),
        })
    }
}
