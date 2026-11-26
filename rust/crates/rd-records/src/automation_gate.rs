//! Ported from `Release/AutomationGate.swift`.

use crate::release::{ReleaseKind, ReleaseRecord};
use crate::repository_snapshot::AuthenticationAvailability;
use rd_model::{Product, ReleaseMode, ValidationCheckId, ValidationStatus};

/// Decides whether a release may publish without a human confirmation. Every
/// condition must hold; the returned list explains each one that does not.
pub struct AutomationGate;

impl AutomationGate {
    pub fn blocking_reasons(
        product: &Product,
        record: &ReleaseRecord,
        current_preview_fingerprint: Option<&str>,
    ) -> Vec<String> {
        let mut reasons = Vec::new();
        if product.release_mode != ReleaseMode::Automatic || !product.automation_enabled {
            reasons.push(format!(
                "Automatic publishing is not explicitly enabled for {}",
                product.name
            ));
        }
        let Some(report) = &record.validation else {
            reasons.push("The release has not been validated".to_owned());
            return reasons;
        };
        let all_passed = |check: ValidationCheckId| {
            let mut results = report.results.iter().filter(|r| r.check == check).peekable();
            results.peek().is_some() && results.all(|r| r.status == ValidationStatus::Passed)
        };

        if !all_passed(ValidationCheckId::VersionValidity) {
            reasons.push("The version is not a valid, final release version".to_owned());
        }
        match record.kind() {
            ReleaseKind::Version => {
                if !all_passed(ValidationCheckId::VersionIncrease) {
                    reasons.push("The version is not confirmed to be newer than the current release".to_owned());
                }
            }
            ReleaseKind::ReadmeUpdate => {
                if !all_passed(ValidationCheckId::ReleasedVersion) {
                    reasons.push("The package is not confirmed to be the released version".to_owned());
                }
            }
        }
        if !all_passed(ValidationCheckId::ProductIdentity) {
            reasons.push("Product identity is not fully confirmed".to_owned());
        }
        if report.is_blocking() {
            reasons.push(format!(
                "{} required check(s) did not pass",
                report.blocking_results().len()
            ));
        }
        let skipped_required: Vec<&str> = report
            .results
            .iter()
            .filter(|r| r.status == ValidationStatus::Skipped && r.check.is_required())
            .map(|r| r.name.as_str())
            .collect();
        if !skipped_required.is_empty() {
            reasons.push(format!("Required checks were skipped: {}", skipped_required.join(", ")));
        }

        if let Some(repo) = &record.repository {
            if !repo.working_copy_changes.is_empty() {
                reasons.push("The working copy had local changes".to_owned());
            }
            if !repo.out_of_date_paths.is_empty() {
                reasons.push("The working copy is not current with the repository".to_owned());
            }
            match record.kind() {
                ReleaseKind::Version => {
                    if repo.release_path_exists != Some(false) {
                        reasons.push("The release tag is not confirmed absent".to_owned());
                    }
                }
                ReleaseKind::ReadmeUpdate => {
                    if repo.release_path_exists != Some(true) {
                        reasons.push("The released tag is not confirmed to exist".to_owned());
                    }
                }
            }
            if !matches!(
                repo.authentication,
                AuthenticationAvailability::Keychain | AuthenticationAvailability::CachedBySvn
            ) {
                reasons.push("No valid authentication is available".to_owned());
            }
        } else {
            reasons.push("Repository state is unknown".to_owned());
        }

        if let Some(preview) = &record.preview {
            if current_preview_fingerprint != Some(preview.fingerprint.as_str()) {
                reasons.push("The release preview changed".to_owned());
            }
        } else {
            reasons.push("No release preview exists".to_owned());
        }
        if !record.errors.is_empty() {
            reasons.push("The release has unresolved errors".to_owned());
        }
        reasons
    }
}
