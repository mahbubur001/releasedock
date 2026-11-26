use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationCheckId {
    SourceIntegrity,
    ProductIdentity,
    VersionValidity,
    VersionIncrease,
    MetadataValidity,
    RequiredFiles,
    ForbiddenFiles,
    PackageStructure,
    SvnConnectivity,
    WorkingCopyClean,
    ExistingTag,
    RemoteFreshness,
    ReleaseDiff,
    Authentication,
    ReleasedVersion,
    // Optional checks
    ChangelogEntry,
    TestedUpTo,
    PackageSize,
    GitWorkingTree,
    PortableFilenames,
    PhpSyntax,
    PluginCheck,
    DirectoryAssets,
}

impl ValidationCheckId {
    pub const ALL: [Self; 23] = [
        Self::SourceIntegrity,
        Self::ProductIdentity,
        Self::VersionValidity,
        Self::VersionIncrease,
        Self::MetadataValidity,
        Self::RequiredFiles,
        Self::ForbiddenFiles,
        Self::PackageStructure,
        Self::SvnConnectivity,
        Self::WorkingCopyClean,
        Self::ExistingTag,
        Self::RemoteFreshness,
        Self::ReleaseDiff,
        Self::Authentication,
        Self::ReleasedVersion,
        Self::ChangelogEntry,
        Self::TestedUpTo,
        Self::PackageSize,
        Self::GitWorkingTree,
        Self::PortableFilenames,
        Self::PhpSyntax,
        Self::PluginCheck,
        Self::DirectoryAssets,
    ];

    /// Required checks always run and their failures always block a release.
    pub fn is_required(self) -> bool {
        !matches!(
            self,
            Self::ChangelogEntry
                | Self::TestedUpTo
                | Self::PackageSize
                | Self::GitWorkingTree
                | Self::PortableFilenames
                | Self::PhpSyntax
                | Self::PluginCheck
                | Self::DirectoryAssets
        )
    }

    /// A required check that could not run (e.g. SVN unreachable) must not silently pass.
    pub fn blocks_when_skipped(self) -> bool {
        // The release diff is only computed after staging.
        self != Self::ReleaseDiff
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::SourceIntegrity => "Source integrity",
            Self::ProductIdentity => "Product identity",
            Self::VersionValidity => "Version validity",
            Self::VersionIncrease => "Version increase",
            Self::MetadataValidity => "Metadata validity",
            Self::RequiredFiles => "Required files",
            Self::ForbiddenFiles => "Forbidden files",
            Self::PackageStructure => "Package structure",
            Self::SvnConnectivity => "SVN connectivity",
            Self::WorkingCopyClean => "Working copy cleanliness",
            Self::ExistingTag => "Existing release tag",
            Self::RemoteFreshness => "Remote repository freshness",
            Self::ReleaseDiff => "Release diff",
            Self::Authentication => "Authentication",
            Self::ReleasedVersion => "Released version",
            Self::ChangelogEntry => "Changelog entry",
            Self::TestedUpTo => "Tested up to",
            Self::PackageSize => "Package size",
            Self::GitWorkingTree => "Git working tree",
            Self::PortableFilenames => "Portable file names",
            Self::PhpSyntax => "PHP syntax",
            Self::PluginCheck => "Plugin Check",
            Self::DirectoryAssets => "Directory assets",
        }
    }

    /// The raw value used in JSON, e.g. `releaseDiff`.
    pub fn raw_value(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationStatus {
    Passed,
    Warning,
    Failed,
    Skipped,
}

/// Ordered info < warning < error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub check: ValidationCheckId,
    pub name: String,
    pub status: ValidationStatus,
    pub severity: ValidationSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl ValidationResult {
    pub fn new(
        check: ValidationCheckId,
        status: ValidationStatus,
        severity: ValidationSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            check,
            name: check.title().to_owned(),
            status,
            severity,
            message: message.into(),
            file_path: None,
            suggestion: None,
        }
    }

    pub fn pass(check: ValidationCheckId, message: impl Into<String>) -> Self {
        Self::new(check, ValidationStatus::Passed, ValidationSeverity::Info, message)
    }

    pub fn fail(check: ValidationCheckId, message: impl Into<String>) -> Self {
        Self::new(check, ValidationStatus::Failed, ValidationSeverity::Error, message)
    }

    pub fn warn(check: ValidationCheckId, message: impl Into<String>) -> Self {
        Self::new(check, ValidationStatus::Warning, ValidationSeverity::Warning, message)
    }

    pub fn skip(check: ValidationCheckId, message: impl Into<String>) -> Self {
        Self::new(check, ValidationStatus::Skipped, ValidationSeverity::Info, message)
    }

    pub fn file(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    pub fn fix(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Stable identity, as in Swift: `check:file:message`.
    pub fn id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.check.raw_value(),
            self.file_path.as_deref().unwrap_or(""),
            self.message
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub results: Vec<ValidationResult>,
    pub block_on_warnings: bool,
}

impl ValidationReport {
    /// Results that prevent publishing.
    pub fn blocking_results(&self) -> Vec<&ValidationResult> {
        self.results
            .iter()
            .filter(|r| match r.status {
                ValidationStatus::Failed => true,
                ValidationStatus::Warning => self.block_on_warnings,
                ValidationStatus::Passed => false,
                ValidationStatus::Skipped => r.check.is_required() && r.check.blocks_when_skipped(),
            })
            .collect()
    }

    pub fn is_blocking(&self) -> bool {
        !self.blocking_results().is_empty()
    }

    pub fn warnings(&self) -> Vec<&ValidationResult> {
        self.results
            .iter()
            .filter(|r| r.status == ValidationStatus::Warning)
            .collect()
    }

    pub fn failures(&self) -> Vec<&ValidationResult> {
        self.results
            .iter()
            .filter(|r| r.status == ValidationStatus::Failed)
            .collect()
    }
}
