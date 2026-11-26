//! Every pre-release check. Ported from `ValidationService` in
//! `Validation/ValidationService.swift`.

use crate::ResultExt;
use crate::context::ValidationContext;
use crate::strategy::{ReleaseStrategy, StrategyProvider};
use crate::text::{byte_count_file_style, lowercased, prefix};
use crate::wordpress_branch::WordPressBranch;
use rd_inspection::asset_rules;
use rd_model::{
    InspectionIssueKind, ProductType, ReleaseMode, SourceKind, ValidationCheckId, ValidationReport, ValidationResult,
    ValidationSeverity, ValidationStatus,
};
use rd_records::{AuthenticationAvailability, ChangeKind, CodeCheckFinding, CodeCheckTool, ReleaseKind};
use rd_versioning::{VersionComparison, VersionService, VersionServicing, VersionValidity, php_version};
use std::sync::Arc;

pub trait ValidationServicing: Send + Sync {
    fn validate(&self, context: &ValidationContext) -> ValidationReport;
}

#[derive(Clone)]
pub struct ValidationService {
    versions: Arc<dyn VersionServicing>,
    strategies: StrategyProvider,
}

impl Default for ValidationService {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ValidationService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidationService").finish_non_exhaustive()
    }
}

/// Inspection issues reported under "Package structure".
pub const STRUCTURAL_KINDS: [InspectionIssueKind; 12] = [
    InspectionIssueKind::PathTraversal,
    InspectionIssueKind::AbsolutePath,
    InspectionIssueKind::UnsafeFilename,
    InspectionIssueKind::SymbolicLink,
    InspectionIssueKind::DuplicatePath,
    InspectionIssueKind::NestedRoot,
    InspectionIssueKind::EncryptedEntry,
    InspectionIssueKind::UnsupportedCompression,
    InspectionIssueKind::ArchiveTooLarge,
    InspectionIssueKind::SuspiciousCompressionRatio,
    InspectionIssueKind::SpecialFile,
    InspectionIssueKind::MultipleRoots,
];

/// Swift `ProductType.rawValue`.
fn raw_type(product_type: ProductType) -> &'static str {
    match product_type {
        ProductType::Plugin => "plugin",
        ProductType::Theme => "theme",
    }
}

impl ValidationService {
    /// Code check findings listed one by one before the rest are summarized.
    pub const LISTED_FINDINGS: usize = 15;

    pub fn new() -> Self {
        Self::with(Arc::new(VersionService::new()), StrategyProvider::new())
    }

    /// Swift `init(versions:strategies:)`.
    pub fn with(versions: Arc<dyn VersionServicing>, strategies: StrategyProvider) -> Self {
        Self { versions, strategies }
    }

    // MARK: Source

    pub fn source_integrity(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        let corrupt: Vec<_> = c
            .inspection
            .issues
            .iter()
            .filter(|issue| issue.kind == InspectionIssueKind::CorruptArchive)
            .collect();
        if !corrupt.is_empty() {
            return corrupt
                .into_iter()
                .map(|issue| {
                    ValidationResult::fail(ValidationCheckId::SourceIntegrity, &issue.message)
                        .fix("Rebuild the ZIP package and try again.")
                })
                .collect();
        }
        let label = if c.inspection.source_kind == SourceKind::Zip {
            "ZIP SHA-256"
        } else {
            "Folder digest"
        };
        let files = c.inspection.included_files().len();
        if files == 0 {
            return vec![ValidationResult::fail(
                ValidationCheckId::SourceIntegrity,
                "The package contains no publishable files",
            )];
        }
        vec![ValidationResult::pass(
            ValidationCheckId::SourceIntegrity,
            format!("{label} {}…, {files} files", prefix(&c.inspection.sha256, 16)),
        )]
    }

    pub fn package_structure(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        let issues: Vec<_> = c
            .inspection
            .issues
            .iter()
            .filter(|issue| STRUCTURAL_KINDS.contains(&issue.kind))
            .collect();
        if issues.is_empty() {
            return vec![ValidationResult::pass(
                ValidationCheckId::PackageStructure,
                "Package structure is safe",
            )];
        }
        issues
            .into_iter()
            .map(|issue| {
                if issue.severity == ValidationSeverity::Error {
                    ValidationResult::fail(ValidationCheckId::PackageStructure, &issue.message)
                        .file_opt(issue.path.as_deref())
                        .fix(structure_fix(issue.kind))
                } else {
                    ValidationResult::warn(ValidationCheckId::PackageStructure, &issue.message)
                        .file_opt(issue.path.as_deref())
                }
            })
            .collect()
    }

    pub fn forbidden_files(&self, c: &ValidationContext, strategy: &dyn ReleaseStrategy) -> Vec<ValidationResult> {
        use ValidationCheckId::ForbiddenFiles;
        let mut results = Vec::new();
        for issue in c.inspection.issues.iter().filter(|i| {
            matches!(
                i.kind,
                InspectionIssueKind::ForbiddenFile | InspectionIssueKind::PossibleSecret
            )
        }) {
            if issue.excluded {
                results.push(ValidationResult::warn(ForbiddenFiles, &issue.message).file_opt(issue.path.as_deref()));
                continue;
            }
            results.push(if issue.severity == ValidationSeverity::Error {
                ValidationResult::fail(ForbiddenFiles, &issue.message)
                    .file_opt(issue.path.as_deref())
                    .fix("Remove the file from the package or add an exclusion pattern.")
            } else {
                ValidationResult::warn(ForbiddenFiles, &issue.message).file_opt(issue.path.as_deref())
            });
        }
        results.extend(strategy.validate_forbidden_files(&c.inspection));
        if results.is_empty() {
            let excluded: Vec<_> = c.inspection.issues.iter().filter(|i| i.excluded).collect();
            let dev_count = excluded
                .iter()
                .filter(|i| {
                    matches!(
                        i.kind,
                        InspectionIssueKind::DevelopmentArtifact | InspectionIssueKind::VersionControlMetadata
                    )
                })
                .count();
            let mut message = "No forbidden files in the release candidate".to_owned();
            if !excluded.is_empty() {
                message += &format!("; {} path(s) excluded", excluded.len());
                if dev_count > 0 {
                    message += &format!(" ({dev_count} development or VCS artifacts)");
                }
            }
            results.push(ValidationResult::pass(ForbiddenFiles, message));
        }
        results
    }

    // MARK: Identity

    /// Lowercased, `&amp;` decoded and whitespace collapsed, so cosmetic differences in a
    /// header name do not fail the identity check.
    pub fn normalize_name(s: &str) -> String {
        lowercased(s)
            .replace("&amp;", "&")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn identity(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        use ValidationCheckId::ProductIdentity;
        let Some(detected) = &c.detected else {
            return vec![
                ValidationResult::fail(
                    ProductIdentity,
                    format!("No {} was detected in the package", raw_type(c.product.product_type)),
                )
                .fix_opt(c.detection_error.as_deref()),
            ];
        };
        let mut results = Vec::new();
        if detected.product_type != c.product.product_type {
            results.push(ValidationResult::fail(
                ProductIdentity,
                format!(
                    "Package is a {}, but {} is registered as a {}",
                    raw_type(detected.product_type),
                    c.product.name,
                    raw_type(c.product.product_type)
                ),
            ));
        }
        if let Some(name) = &detected.name {
            if Self::normalize_name(name) == Self::normalize_name(&c.product.expected_header_name) {
                let kind = if detected.product_type == ProductType::Plugin {
                    "Plugin"
                } else {
                    "Theme"
                };
                results.push(ValidationResult::pass(
                    ProductIdentity,
                    format!("{kind} Name \"{name}\" matches"),
                ));
            } else {
                results.push(
                    ValidationResult::fail(
                        ProductIdentity,
                        format!(
                            "Package declares \"{name}\", expected \"{}\"",
                            c.product.expected_header_name
                        ),
                    )
                    .file_opt(detected.main_file.as_deref())
                    .fix(format!(
                        "Make sure this package belongs to {}. If the product was renamed, update its expected header name.",
                        c.product.name
                    )),
                );
            }
        }
        if let Some(domain) = detected.text_domain.as_deref().filter(|d| *d != c.product.slug) {
            results.push(
                ValidationResult::warn(
                    ProductIdentity,
                    format!(
                        "Text Domain \"{domain}\" differs from the slug \"{}\"; WordPress.org translations use the slug",
                        c.product.slug
                    ),
                )
                .file_opt(detected.main_file.as_deref()),
            );
        }
        if let Some(root) = c
            .inspection
            .archive_root_name
            .as_deref()
            .filter(|r| *r != c.product.slug)
        {
            results.push(ValidationResult::warn(
                ProductIdentity,
                format!(
                    "ZIP top-level folder \"{root}\" differs from the slug \"{}\"",
                    c.product.slug
                ),
            ));
        }
        results
    }

    // MARK: Version

    pub fn version(&self, c: &ValidationContext, strategy: &dyn ReleaseStrategy) -> Vec<ValidationResult> {
        use ValidationCheckId::{ExistingTag, VersionIncrease, VersionValidity as Validity};
        let main_file = c.detected.as_ref().and_then(|d| d.main_file.as_deref());
        let Some(version) = c.detected.as_ref().and_then(|d| d.version.as_deref()) else {
            return vec![
                ValidationResult::fail(Validity, "No Version header found")
                    .file_opt(main_file)
                    .fix("Add a \"Version:\" header."),
                ValidationResult::skip(VersionIncrease, "No version to compare"),
                ValidationResult::skip(ExistingTag, "No version to check"),
            ];
        };
        let mut results = Vec::new();
        match self.versions.validate(version) {
            VersionValidity::Invalid { reason } => results.push(
                ValidationResult::fail(Validity, format!("Version \"{version}\" requires review: {reason}"))
                    .file_opt(main_file),
            ),
            VersionValidity::Valid { is_prerelease } => {
                results.push(ValidationResult::pass(Validity, format!("Version {version}")));
                if is_prerelease {
                    results.push(
                        ValidationResult::warn(
                            Validity,
                            format!(
                                "{version} is a pre-release; publishing it makes it the stable version for every site"
                            ),
                        )
                        .fix("Publish pre-releases outside WordPress.org, or confirm this is intended."),
                    );
                }
            }
        }
        results.extend(strategy.validate_version_format(version));
        if c.kind == ReleaseKind::ReadmeUpdate {
            results.extend(self.released_version(c, version, strategy));
            return results;
        }

        match c.repository.as_ref().filter(|repo| repo.connectivity_error.is_none()) {
            Some(repo) => {
                match self.versions.compare(version, repo.current_version.as_deref()) {
                    VersionComparison::Newer => results.push(ValidationResult::pass(
                        VersionIncrease,
                        format!(
                            "{version} is newer than {}",
                            repo.current_version.as_deref().unwrap_or("-")
                        ),
                    )),
                    VersionComparison::NoCurrentRelease => results.push(ValidationResult::pass(
                        VersionIncrease,
                        format!("No previous release found; {version} will be the first"),
                    )),
                    VersionComparison::Same => results.push(
                        ValidationResult::fail(
                            VersionIncrease,
                            format!("Version {version} is already the current release"),
                        )
                        .fix("Increase the version number."),
                    ),
                    VersionComparison::Older => results.push(
                        ValidationResult::fail(
                            VersionIncrease,
                            format!(
                                "Version {version} is older than the current release {}",
                                repo.current_version.as_deref().unwrap_or("")
                            ),
                        )
                        .fix("Increase the version number."),
                    ),
                    VersionComparison::RequiresReview { reason } => results.push(ValidationResult::fail(
                        VersionIncrease,
                        format!("Version comparison requires review: {reason}"),
                    )),
                }
                let release_path = strategy.release_path(version);
                match repo.release_path_exists {
                    Some(true) => results.push(
                        ValidationResult::fail(ExistingTag, format!("{release_path} already exists in SVN"))
                            .fix("Releases are immutable. Publish a higher version."),
                    ),
                    Some(false) => results.push(ValidationResult::pass(
                        ExistingTag,
                        format!("{release_path} does not exist yet"),
                    )),
                    None => results.push(ValidationResult::skip(
                        ExistingTag,
                        "Could not determine whether the release already exists",
                    )),
                }
            }
            None => {
                results.push(ValidationResult::skip(
                    VersionIncrease,
                    "Repository unavailable; version could not be compared",
                ));
                results.push(ValidationResult::skip(ExistingTag, "Repository unavailable"));
            }
        }
        results
    }

    /// A readme update edits the released version in place: the package must carry exactly
    /// that version, and its tag must exist.
    pub fn released_version(
        &self,
        c: &ValidationContext,
        version: &str,
        strategy: &dyn ReleaseStrategy,
    ) -> Vec<ValidationResult> {
        use ValidationCheckId::{ExistingTag, ReleasedVersion};
        let release_path = strategy.release_path(version);
        if c.product.product_type != ProductType::Plugin {
            return vec![
                ValidationResult::fail(
                    ReleasedVersion,
                    "Readme updates without a new version are only available for plugins",
                )
                .fix("Publish a new theme version instead."),
            ];
        }
        let Some(repo) = c.repository.as_ref().filter(|repo| repo.connectivity_error.is_none()) else {
            return vec![ValidationResult::skip(
                ReleasedVersion,
                "Repository unavailable; the released version could not be checked",
            )];
        };
        let Some(current) = repo.current_version.as_deref() else {
            return vec![
                ValidationResult::fail(
                    ReleasedVersion,
                    format!("{} has no released version to update", c.product.name),
                )
                .fix("Publish a version first."),
            ];
        };
        let mut results = Vec::new();
        if php_version::compare(version, current) == 0 {
            results.push(ValidationResult::pass(
                ReleasedVersion,
                format!("Updates the readme of the released version {current}"),
            ));
        } else {
            results.push(
                ValidationResult::fail(
                    ReleasedVersion,
                    format!("The package is version {version}, but the released version is {current}"),
                )
                .fix(format!(
                    "Use the package of {current}, or publish {version} as a new version."
                )),
            );
        }
        match repo.release_path_exists {
            Some(true) => results.push(ValidationResult::pass(
                ExistingTag,
                format!("{release_path} exists and will be updated"),
            )),
            Some(false) => results.push(
                ValidationResult::fail(ExistingTag, format!("{release_path} does not exist in SVN"))
                    .fix(format!("Publish {version} as a new version.")),
            ),
            None => results.push(ValidationResult::skip(
                ExistingTag,
                format!("Could not determine whether {release_path} exists"),
            )),
        }
        results
    }

    // MARK: Repository

    pub fn repository(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        use ValidationCheckId::{Authentication, RemoteFreshness, SvnConnectivity, WorkingCopyClean};
        let Some(repo) = &c.repository else {
            return [SvnConnectivity, WorkingCopyClean, RemoteFreshness, Authentication]
                .into_iter()
                .map(|check| ValidationResult::skip(check, "Repository not checked"))
                .collect();
        };
        let mut results = Vec::new();
        if let Some(error) = &repo.connectivity_error {
            results.push(
                ValidationResult::fail(SvnConnectivity, error)
                    .fix("Check the network connection and the SVN repository URL."),
            );
        } else if repo.repository_exists == Some(false) {
            results.push(
                ValidationResult::fail(
                    SvnConnectivity,
                    format!("Repository {} does not exist", c.product.svn_repository_url),
                )
                .fix("Check the slug and repository URL."),
            );
        } else {
            results.push(ValidationResult::pass(
                SvnConnectivity,
                format!(
                    "Connected (svn {}, r{})",
                    repo.svn_version.as_deref().unwrap_or("?"),
                    repo.remote_revision.map_or_else(|| "?".to_owned(), |r| r.to_string())
                ),
            ));
        }
        for problem in &repo.structure_problems {
            results.push(ValidationResult::fail(SvnConnectivity, problem));
        }

        if let Some(mismatch) = &repo.working_copy_url_mismatch {
            results.push(
                ValidationResult::fail(WorkingCopyClean, mismatch)
                    .fix("Choose an empty folder or a checkout of this product's repository."),
            );
        } else if !repo.working_copy_changes.is_empty() {
            let shown = repo
                .working_copy_changes
                .iter()
                .take(5)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            results.push(
                ValidationResult::fail(
                    WorkingCopyClean,
                    format!(
                        "Working copy has {} local change(s): {shown}",
                        repo.working_copy_changes.len()
                    ),
                )
                .fix("Commit or revert them. ReleaseDock can back them up and revert."),
            );
        } else {
            results.push(ValidationResult::pass(
                WorkingCopyClean,
                if repo.working_copy_exists {
                    "Working copy is clean"
                } else {
                    "Working copy will be checked out"
                },
            ));
        }

        if !repo.out_of_date_paths.is_empty() {
            results.push(
                ValidationResult::fail(
                    RemoteFreshness,
                    format!(
                        "{} path(s) changed on the server since the last update",
                        repo.out_of_date_paths.len()
                    ),
                )
                .fix("Update the working copy and prepare the release again."),
            );
        } else if repo.connectivity_error.is_none() {
            let revision = repo
                .working_copy_revision
                .map_or_else(String::new, |r| format!(" (r{r})"));
            results.push(ValidationResult::pass(
                RemoteFreshness,
                format!("Working copy is up to date{revision}"),
            ));
        } else {
            results.push(ValidationResult::skip(RemoteFreshness, "Repository unavailable"));
        }

        match repo.authentication {
            AuthenticationAvailability::Keychain => results.push(ValidationResult::pass(
                Authentication,
                "SVN credentials stored in the Keychain",
            )),
            AuthenticationAvailability::CachedBySvn => results.push(ValidationResult::pass(
                Authentication,
                "svn has cached credentials for this repository",
            )),
            AuthenticationAvailability::None => {
                if c.product.release_mode == ReleaseMode::DryRun {
                    results.push(ValidationResult::warn(
                        Authentication,
                        "No SVN credentials are available; a real release will need them",
                    ));
                } else {
                    results.push(
                        ValidationResult::fail(Authentication, "No SVN credentials are available").fix(
                            "Add your WordPress.org username and SVN password (Profile → Account & Security → SVN credentials).",
                        ),
                    );
                }
            }
            AuthenticationAvailability::Unknown => results.push(ValidationResult::skip(
                Authentication,
                "Authentication availability unknown",
            )),
        }
        results
    }

    pub fn release_diff(&self, c: &ValidationContext, strategy: &dyn ReleaseStrategy) -> Vec<ValidationResult> {
        use ValidationCheckId::ReleaseDiff;
        let Some(preview) = &c.preview else {
            return vec![ValidationResult::skip(ReleaseDiff, "Computed after staging")];
        };
        let Some(version) = c.detected.as_ref().and_then(|d| d.version.as_deref()) else {
            return vec![ValidationResult::fail(ReleaseDiff, "No version")];
        };
        let mut results = Vec::new();
        if preview.changes.is_empty() {
            results.push(ValidationResult::fail(
                ReleaseDiff,
                if c.kind == ReleaseKind::ReadmeUpdate {
                    "The readme and assets already match the server; there is nothing to update"
                } else {
                    "The release produces no SVN changes"
                },
            ));
        }
        let release_path = strategy.release_path(version);
        if c.kind == ReleaseKind::ReadmeUpdate {
            let readme = c
                .detected
                .as_ref()
                .and_then(|d| d.readme_file.as_deref())
                .unwrap_or("readme.txt");
            let allowed = [format!("trunk/{readme}"), format!("{release_path}/{readme}")];
            let unexpected = preview.changes.iter().filter(|change| {
                !(allowed.contains(&change.path) || change.path == "assets" || change.path.starts_with("assets/"))
            });
            for change in unexpected.take(10) {
                results.push(
                    ValidationResult::fail(ReleaseDiff, "A readme update may only change the readme and /assets")
                        .file(&change.path),
                );
            }
            if results.is_empty() {
                results.push(ValidationResult::pass(
                    ReleaseDiff,
                    format!(
                        "{} change(s) to the readme and assets of {release_path}",
                        preview.changes.len()
                    ),
                ));
            }
            return results;
        }
        if !preview
            .changes
            .iter()
            .any(|change| change.path == release_path && change.kind == ChangeKind::Added)
        {
            results.push(ValidationResult::fail(
                ReleaseDiff,
                format!("The preview does not create {release_path}"),
            ));
        }
        let allowed = strategy.allowed_change_prefixes(version);
        let unexpected = preview.changes.iter().filter(|change| {
            !allowed
                .iter()
                .any(|prefix| change.path == *prefix || change.path.starts_with(&format!("{prefix}/")))
        });
        for change in unexpected.take(10) {
            results.push(
                ValidationResult::fail(ReleaseDiff, "Unexpected change outside the release paths").file(&change.path),
            );
        }
        if results.is_empty() {
            results.push(ValidationResult::pass(
                ReleaseDiff,
                format!(
                    "{} added, {} modified, {} deleted; creates {release_path}",
                    preview.added(),
                    preview.modified(),
                    preview.deleted()
                ),
            ));
        }
        results
    }

    // MARK: Tested up to

    /// Compares "Tested up to" with the current WordPress branch (Plugin Check's
    /// `outdated_tested_upto_header`) and, for plugins, with the main file's own
    /// "Tested up to" header (`mismatched_tested_up_to_header`). Format problems are
    /// reported by the strategies; values that are not version numbers are ignored here.
    pub fn tested_up_to_freshness(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        use ValidationCheckId::TestedUpTo;
        let Some(detected) = &c.detected else {
            return Vec::new();
        };
        let mut results = Vec::new();
        let readme_value = detected.readme.as_ref().and_then(|r| r.tested_up_to());
        let header = detected.headers.get("Tested up to").map(String::as_str);
        let (tested, file) = match detected.product_type {
            ProductType::Plugin => {
                if let (Some(header), Some(readme_value)) = (header, readme_value) {
                    if WordPressBranch::compare(header, readme_value).is_some_and(|order| order != 0) {
                        results.push(
                            ValidationResult::warn(
                                TestedUpTo,
                                format!(
                                    "Tested up to is {header} in {} but {readme_value} in the readme",
                                    detected.main_file.as_deref().unwrap_or("the plugin file")
                                ),
                            )
                            .file_opt(detected.main_file.as_deref())
                            .fix("Use the same value in both files, or remove it from the plugin header."),
                        );
                    }
                }
                (readme_value, detected.readme_file.as_deref())
            }
            ProductType::Theme => (header.or(readme_value), detected.main_file.as_deref()),
        };
        let Some(tested) = tested else {
            return results;
        };
        let Some(latest) = c
            .repository
            .as_ref()
            .and_then(|r| r.latest_wordpress_version.as_deref())
        else {
            return results;
        };
        let (Some(latest_branch), Some(order)) =
            (WordPressBranch::of(latest), WordPressBranch::compare(tested, latest))
        else {
            return results;
        };
        if order < 0 {
            results.push(
                ValidationResult::warn(
                    TestedUpTo,
                    format!(
                        "Tested up to {tested} is behind the current WordPress release {latest}; WordPress.org's Plugin Check reports it as outdated"
                    ),
                )
                .file_opt(file)
                .fix(format!(
                    "Test with WordPress {latest_branch}, then set \"Tested up to: {latest_branch}\"."
                )),
            );
        } else if results.is_empty() {
            results.push(ValidationResult::pass(
                TestedUpTo,
                format!("Tested up to {tested} covers the current WordPress release {latest}"),
            ));
        }
        results
    }

    // MARK: Code checks

    pub fn code_checks(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        use ValidationCheckId::{PhpSyntax, PluginCheck};
        let Some(report) = &c.code_checks else {
            return Vec::new();
        };
        let mut results = Vec::new();
        fn location(finding: &CodeCheckFinding) -> String {
            finding.line.map_or_else(String::new, |line| format!("Line {line}: "))
        }

        if let Some(reason) = &report.php_lint_skipped {
            results.push(ValidationResult::skip(PhpSyntax, reason));
        } else {
            let lint = report.findings_for(CodeCheckTool::PhpLint);
            let errors: Vec<_> = lint
                .iter()
                .filter(|f| f.severity == ValidationSeverity::Error)
                .collect();
            for finding in errors.iter().take(Self::LISTED_FINDINGS) {
                results.push(
                    ValidationResult::fail(PhpSyntax, location(finding) + &finding.message)
                        .file_opt(finding.file.as_deref())
                        .fix("Fix the syntax error. WordPress cannot load a file that does not parse."),
                );
            }
            if errors.len() > Self::LISTED_FINDINGS {
                results.push(ValidationResult::fail(
                    PhpSyntax,
                    format!("{} more syntax error(s)", errors.len() - Self::LISTED_FINDINGS),
                ));
            }
            for finding in lint
                .iter()
                .filter(|f| f.severity != ValidationSeverity::Error)
                .take(Self::LISTED_FINDINGS)
            {
                results.push(
                    ValidationResult::warn(PhpSyntax, location(finding) + &finding.message)
                        .file_opt(finding.file.as_deref()),
                );
            }
            if lint.is_empty() {
                results.push(ValidationResult::pass(
                    PhpSyntax,
                    format!(
                        "{} PHP file(s) parse with PHP {}",
                        report.linted_files,
                        report.php_version.as_deref().unwrap_or("?")
                    ),
                ));
            }
        }

        if let Some(reason) = &report.plugin_check_skipped {
            results.push(ValidationResult::skip(PluginCheck, reason));
        } else if report.plugin_check_ran {
            let mut findings = report.findings_for(CodeCheckTool::PluginCheck);
            if findings.is_empty() {
                results.push(ValidationResult::pass(PluginCheck, "Plugin Check found no issues"));
            } else {
                let errors = findings
                    .iter()
                    .filter(|f| f.severity == ValidationSeverity::Error)
                    .count();
                results.push(
                    ValidationResult::warn(
                        PluginCheck,
                        format!(
                            "Plugin Check: {errors} error(s), {} warning(s). WordPress.org runs these checks on every release.",
                            findings.len() - errors
                        ),
                    )
                    .fix("Review the findings; fix real issues before publishing."),
                );
                findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
                for finding in findings.into_iter().take(Self::LISTED_FINDINGS) {
                    let code = finding
                        .code
                        .as_deref()
                        .map_or_else(String::new, |code| format!("[{code}] "));
                    results.push(
                        ValidationResult::warn(PluginCheck, location(finding) + &code + &finding.message)
                            .file_opt(finding.file.as_deref()),
                    );
                }
            }
        }
        results
    }

    // MARK: Optional

    pub fn optional(&self, c: &ValidationContext) -> Vec<ValidationResult> {
        use ValidationCheckId::{GitWorkingTree, PackageSize, PortableFilenames};
        let mut results = Vec::new();
        let settings = &c.product.validation;
        // Swift traps on a negative setting; here it counts as 0.
        let limit = u64::try_from(settings.package_size_warning_mb).unwrap_or(0) << 20;
        let total = c.inspection.total_uncompressed_size;
        if total > limit {
            results.push(ValidationResult::warn(
                PackageSize,
                format!(
                    "Package is {} MiB (warning threshold {} MiB)",
                    total >> 20,
                    settings.package_size_warning_mb
                ),
            ));
        } else {
            results.push(ValidationResult::pass(
                PackageSize,
                format!("Package size {}", byte_count_file_style(total)),
            ));
        }

        let portability: Vec<_> = c
            .inspection
            .issues
            .iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    InspectionIssueKind::Portability | InspectionIssueKind::ZeroByteFile
                )
            })
            .collect();
        if portability.is_empty() {
            results.push(ValidationResult::pass(PortableFilenames, "File names are portable"));
        } else {
            results.extend(
                portability
                    .into_iter()
                    .map(|i| ValidationResult::warn(PortableFilenames, &i.message).file_opt(i.path.as_deref())),
            );
        }

        if let Some(restored) = &c.restores_version {
            // The package was exported from the released tag, not from a development folder.
            results.push(ValidationResult::pass(
                GitWorkingTree,
                format!("Code exported from the released {restored} on WordPress.org"),
            ));
        } else if let Some(git) = c.git.as_ref().filter(|git| git.is_repository) {
            if git.is_clean() {
                let head = git.head_commit.as_deref().map_or("?", |commit| prefix(commit, 10));
                results.push(ValidationResult::pass(
                    GitWorkingTree,
                    format!("Git tree clean at {head}"),
                ));
            } else if settings.require_clean_git_tree {
                results.push(
                    ValidationResult::fail(
                        GitWorkingTree,
                        format!(
                            "Development folder has {} uncommitted change(s)",
                            git.uncommitted_changes.len()
                        ),
                    )
                    .fix("Commit or stash changes before releasing."),
                );
            } else {
                results.push(ValidationResult::warn(
                    GitWorkingTree,
                    format!(
                        "Development folder has {} uncommitted change(s)",
                        git.uncommitted_changes.len()
                    ),
                ));
            }
        } else if settings.require_clean_git_tree && c.inspection.source_kind == SourceKind::Folder {
            results.push(ValidationResult::fail(
                GitWorkingTree,
                "A clean Git tree is required but the folder is not a Git repository",
            ));
        }
        results
    }
}

fn structure_fix(kind: InspectionIssueKind) -> &'static str {
    match kind {
        InspectionIssueKind::SymbolicLink => "Replace the link with the real file, or exclude it.",
        InspectionIssueKind::NestedRoot => {
            "Rebuild the ZIP so the plugin or theme files sit directly inside one top-level folder."
        }
        InspectionIssueKind::DuplicatePath => "Remove the duplicate or case-conflicting file.",
        InspectionIssueKind::PathTraversal | InspectionIssueKind::AbsolutePath => {
            "The archive is unsafe. Rebuild it with a standard ZIP tool."
        }
        _ => "Rebuild the package without this entry.",
    }
}

impl ValidationServicing for ValidationService {
    fn validate(&self, context: &ValidationContext) -> ValidationReport {
        let strategy = self.strategies.strategy(context.product.product_type);
        let mut results = Vec::new();
        results.extend(self.source_integrity(context));
        results.extend(self.package_structure(context));
        results.extend(self.forbidden_files(context, strategy));
        results.extend(self.identity(context));
        results.extend(self.version(context, strategy));
        if let Some(detected) = &context.detected {
            results.extend(strategy.validate_metadata(detected, &context.product));
            results.extend(strategy.validate_required_files(&context.inspection, detected));
        } else {
            results.push(
                ValidationResult::fail(
                    ValidationCheckId::MetadataValidity,
                    "Product metadata could not be read",
                )
                .fix_opt(context.detection_error.as_deref()),
            );
            results.push(ValidationResult::skip(
                ValidationCheckId::RequiredFiles,
                "Skipped because the product was not detected",
            ));
        }
        results.extend(self.repository(context));
        results.extend(self.release_diff(context, strategy));
        results.extend(self.optional(context));
        let freshness = self.tested_up_to_freshness(context);
        if !freshness.is_empty() {
            // The freshness result supersedes the strategy's format-only pass.
            results.retain(|r| !(r.check == ValidationCheckId::TestedUpTo && r.status == ValidationStatus::Passed));
        }
        results.extend(freshness);
        results.extend(self.code_checks(context));
        if let Some(assets) = &context.assets {
            results.extend(asset_rules::check(assets));
        }

        let disabled = &context.product.validation.disabled_checks;
        results.retain(|r| r.check.is_required() || !disabled.contains(&r.check));
        ValidationReport {
            results,
            block_on_warnings: context.product.validation.block_on_warnings,
        }
    }
}
