use crate::time::SwiftDate;
use crate::validation::ValidationSeverity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Zip,
    Folder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    /// Path relative to the package root, `/`-separated.
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectionIssueKind {
    PathTraversal,
    AbsolutePath,
    UnsafeFilename,
    SymbolicLink,
    DuplicatePath,
    NestedRoot,
    MultipleRoots,
    EncryptedEntry,
    UnsupportedCompression,
    CorruptArchive,
    ArchiveTooLarge,
    SuspiciousCompressionRatio,
    JunkFile,
    VersionControlMetadata,
    DevelopmentArtifact,
    ForbiddenFile,
    PossibleSecret,
    ZeroByteFile,
    Portability,
    SpecialFile,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionIssue {
    pub kind: InspectionIssueKind,
    pub severity: ValidationSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
    /// Whether the path is excluded from the release candidate automatically.
    pub excluded: bool,
}

/// Result of inspecting a ZIP archive or source folder. Inspection never modifies the source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInspection {
    pub source_kind: SourceKind,
    pub source_path: String,
    /// SHA-256 of the ZIP file, or the tree digest of the included files for a folder.
    pub sha256: String,
    /// Top-level directory inside the archive, when the archive wraps the package in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_root_name: Option<String>,
    /// Entries that will form the release candidate (after exclusions).
    pub included_entries: Vec<PackageEntry>,
    /// Paths excluded from the release candidate, relative to the package root.
    pub excluded_paths: Vec<String>,
    pub issues: Vec<InspectionIssue>,
    pub total_uncompressed_size: u64,
    pub inspected_at: SwiftDate,
}

impl PackageInspection {
    pub fn blocking_issues(&self) -> Vec<&InspectionIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Error && !i.excluded)
            .collect()
    }

    pub fn included_files(&self) -> Vec<&PackageEntry> {
        self.included_entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .collect()
    }
}
