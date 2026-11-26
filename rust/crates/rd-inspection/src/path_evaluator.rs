//! Inspection limits and the path evaluator. Ported from `PackagePathEvaluator.swift`.

use crate::file_classifier::FileClassifier;
use rd_model::{InspectionIssue, InspectionIssueKind, ValidationSeverity};
use rd_support::path_safety::Violation;
use std::collections::HashSet;

/// Limits applied while inspecting untrusted packages.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectionOptions {
    pub exclude_patterns: Vec<String>,
    /// Maximum total uncompressed size of a package.
    pub maximum_total_size: u64,
    /// Maximum size of a single file.
    pub maximum_file_size: u64,
    /// Compression ratio above which a large entry is treated as a possible zip bomb.
    pub maximum_compression_ratio: f64,
    pub maximum_entries: usize,
}

impl Default for InspectionOptions {
    fn default() -> Self {
        Self {
            exclude_patterns: Vec::new(),
            maximum_total_size: 512 << 20,
            maximum_file_size: 256 << 20,
            maximum_compression_ratio: 200.0,
            maximum_entries: 50_000,
        }
    }
}

/// `InspectionIssue(kind:severity:path:message:)` with `excluded: false`.
pub(crate) fn issue(
    kind: InspectionIssueKind,
    severity: ValidationSeverity,
    path: Option<String>,
    message: impl Into<String>,
) -> InspectionIssue {
    InspectionIssue {
        kind,
        severity,
        path,
        message: message.into(),
        excluded: false,
    }
}

/// Applies [`FileClassifier`] to package-relative paths, propagating directory
/// exclusions to descendants and reporting each excluded directory only once.
#[derive(Clone, Debug)]
pub struct PackagePathEvaluator {
    pub classifier: FileClassifier,
    issues: Vec<InspectionIssue>,
    excluded_paths: Vec<String>,
    excluded_directories: HashSet<String>,
    evaluated_directories: HashSet<String>,
}

impl PackagePathEvaluator {
    pub fn new(classifier: FileClassifier) -> Self {
        Self {
            classifier,
            issues: Vec::new(),
            excluded_paths: Vec::new(),
            excluded_directories: HashSet::new(),
            evaluated_directories: HashSet::new(),
        }
    }

    pub fn issues(&self) -> &[InspectionIssue] {
        &self.issues
    }

    pub fn excluded_paths(&self) -> &[String] {
        &self.excluded_paths
    }

    pub(crate) fn into_parts(self) -> (Vec<InspectionIssue>, Vec<String>) {
        (self.issues, self.excluded_paths)
    }

    /// Returns true when the path is included in the release candidate.
    pub fn include(&mut self, path: &str, is_directory: bool) -> bool {
        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        // Ancestors first: an excluded ancestor excludes this path silently.
        let mut prefix = String::new();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if self.excluded_directories.contains(&prefix) {
                return false;
            }
            if !self.evaluated_directories.contains(&prefix) {
                self.evaluated_directories.insert(prefix.clone());
                if !self.evaluate(&prefix.clone(), true) {
                    return false;
                }
            }
        }
        if is_directory {
            if self.excluded_directories.contains(path) {
                return false;
            }
            if self.evaluated_directories.contains(path) {
                return true;
            }
            self.evaluated_directories.insert(path.to_owned());
        }
        self.evaluate(path, is_directory)
    }

    fn evaluate(&mut self, path: &str, is_directory: bool) -> bool {
        let Some(classification) = self.classifier.classify(path, is_directory) else {
            return true;
        };
        let display_path = if is_directory {
            format!("{path}/")
        } else {
            path.to_owned()
        };
        let suffix = if classification.exclude {
            " — excluded from the release"
        } else {
            ""
        };
        self.issues.push(InspectionIssue {
            kind: classification.kind,
            severity: classification.severity,
            path: Some(display_path.clone()),
            message: format!("{}{suffix}", classification.message),
            excluded: classification.exclude,
        });
        if classification.exclude {
            self.excluded_paths.push(display_path);
            if is_directory {
                self.excluded_directories.insert(path.to_owned());
            }
            return false;
        }
        true
    }

    pub fn add_issue(&mut self, issue: InspectionIssue) {
        self.issues.push(issue);
    }
}

/// `PathSafety.Violation.issueKind` in Swift.
pub fn issue_kind(violation: &Violation) -> InspectionIssueKind {
    match violation {
        Violation::ParentTraversal | Violation::EscapesRoot => InspectionIssueKind::PathTraversal,
        Violation::AbsolutePath => InspectionIssueKind::AbsolutePath,
        _ => InspectionIssueKind::UnsafeFilename,
    }
}
