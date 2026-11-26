//! Ported from CodeCheckTests.swift (`CodeCheckValidationTests`). The parsing and service tests
//! belong to the code check service; the pipeline test is ported with the pipeline.

mod common;

use common::{TempDir, detect, inspect, make_plugin, make_product};
use rd_model::{
    ProductType, ValidationCheckId, ValidationReport, ValidationResult, ValidationSeverity, ValidationStatus,
};
use rd_records::{AuthenticationAvailability, CodeCheckFinding, CodeCheckReport, CodeCheckTool, RepositorySnapshot};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};
use std::collections::BTreeSet;

fn validate(report: CodeCheckReport, disabled: BTreeSet<ValidationCheckId>) -> ValidationReport {
    let dir = TempDir::new();
    let root = make_plugin(&dir, "1.1.0", None, &[]);
    let mut product = make_product();
    product.validation.disabled_checks = disabled;
    let inspection = inspect(&root);
    let detected = detect(&root, Some(ProductType::Plugin), Some("sample-plugin"));
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        current_version: Some("1.0.0".into()),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    };
    let mut context = ValidationContext::new(product, inspection, Some(detected), Some(repo));
    context.code_checks = Some(report);
    ValidationService::new().validate(&context)
}

fn results(report: &ValidationReport, check: ValidationCheckId) -> Vec<&ValidationResult> {
    report.results.iter().filter(|r| r.check == check).collect()
}

fn finding(
    tool: CodeCheckTool,
    severity: ValidationSeverity,
    file: &str,
    line: i64,
    code: Option<&str>,
    message: &str,
) -> CodeCheckFinding {
    CodeCheckFinding::new(
        tool,
        severity,
        Some(file.into()),
        Some(line),
        code.map(str::to_owned),
        message,
    )
}

#[test]
fn syntax_error_blocks() {
    let report = validate(
        CodeCheckReport {
            php_version: Some("8.5.7".into()),
            linted_files: 3,
            plugin_check_skipped: Some("Not set up".into()),
            findings: vec![finding(
                CodeCheckTool::PhpLint,
                ValidationSeverity::Error,
                "includes/bad.php",
                3,
                None,
                "Parse error: syntax error",
            )],
            ..CodeCheckReport::default()
        },
        BTreeSet::new(),
    );
    assert!(report.is_blocking());
    let failure = results(&report, ValidationCheckId::PhpSyntax)[0];
    assert_eq!(failure.status, ValidationStatus::Failed);
    assert_eq!(failure.file_path.as_deref(), Some("includes/bad.php"));
    assert_eq!(failure.message, "Line 3: Parse error: syntax error");
    assert_eq!(
        results(&report, ValidationCheckId::PluginCheck)
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [ValidationStatus::Skipped]
    );
}

#[test]
fn disabled_syntax_check_does_not_block() {
    let report = validate(
        CodeCheckReport {
            php_version: Some("8.5.7".into()),
            linted_files: 3,
            findings: vec![finding(
                CodeCheckTool::PhpLint,
                ValidationSeverity::Error,
                "a.php",
                1,
                None,
                "Parse error",
            )],
            ..CodeCheckReport::default()
        },
        [ValidationCheckId::PhpSyntax].into(),
    );
    assert!(!report.is_blocking());
    assert!(results(&report, ValidationCheckId::PhpSyntax).is_empty());
}

#[test]
fn clean_lint_passes_and_skipped_lint_does_not_block() {
    let clean = validate(
        CodeCheckReport {
            php_version: Some("8.5.7".into()),
            linted_files: 4,
            ..CodeCheckReport::default()
        },
        BTreeSet::new(),
    );
    assert_eq!(
        results(&clean, ValidationCheckId::PhpSyntax)
            .iter()
            .map(|r| r.message.as_str())
            .collect::<Vec<_>>(),
        ["4 PHP file(s) parse with PHP 8.5.7"]
    );
    let skipped = validate(
        CodeCheckReport {
            php_lint_skipped: Some("PHP is not installed".into()),
            ..CodeCheckReport::default()
        },
        BTreeSet::new(),
    );
    assert_eq!(
        results(&skipped, ValidationCheckId::PhpSyntax)
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [ValidationStatus::Skipped]
    );
    assert!(
        !skipped.is_blocking(),
        "an optional check that could not run never blocks"
    );
}

#[test]
fn plugin_check_findings_warn_without_blocking() {
    let findings = (1..=20)
        .map(|i| {
            finding(
                CodeCheckTool::PluginCheck,
                if i == 20 {
                    ValidationSeverity::Error
                } else {
                    ValidationSeverity::Warning
                },
                &format!("f{i}.php"),
                i,
                Some(&format!("code_{i}")),
                &format!("Finding {i}"),
            )
        })
        .collect();
    let report = validate(
        CodeCheckReport {
            php_version: Some("8.5.7".into()),
            linted_files: 1,
            plugin_check_ran: true,
            findings,
            ..CodeCheckReport::default()
        },
        BTreeSet::new(),
    );
    assert!(!report.is_blocking());
    let pcp = results(&report, ValidationCheckId::PluginCheck);
    assert_eq!(
        pcp.first().map(|r| r.message.as_str()),
        Some("Plugin Check: 1 error(s), 19 warning(s). WordPress.org runs these checks on every release.")
    );
    assert_eq!(pcp.len(), 1 + ValidationService::LISTED_FINDINGS);
    assert_eq!(
        pcp[1].message, "Line 20: [code_20] Finding 20",
        "errors are listed first"
    );
    let none = validate(
        CodeCheckReport {
            php_version: Some("8.5.7".into()),
            linted_files: 1,
            plugin_check_ran: true,
            ..CodeCheckReport::default()
        },
        BTreeSet::new(),
    );
    assert_eq!(
        results(&none, ValidationCheckId::PluginCheck)
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [ValidationStatus::Passed]
    );
}

// Added beyond the Swift suite.

#[test]
fn many_syntax_errors_are_summarized() {
    let findings = (1..=17)
        .map(|i| {
            finding(
                CodeCheckTool::PhpLint,
                ValidationSeverity::Error,
                &format!("f{i}.php"),
                i,
                None,
                "Parse error",
            )
        })
        .chain(std::iter::once(CodeCheckFinding::new(
            CodeCheckTool::PhpLint,
            ValidationSeverity::Warning,
            Some("old.php".into()),
            None,
            None,
            "Deprecated",
        )))
        .collect();
    let report = validate(
        CodeCheckReport {
            php_version: Some("8.5.7".into()),
            linted_files: 18,
            plugin_check_skipped: Some("Not set up".into()),
            findings,
            ..CodeCheckReport::default()
        },
        BTreeSet::new(),
    );
    let syntax = results(&report, ValidationCheckId::PhpSyntax);
    assert_eq!(syntax.len(), ValidationService::LISTED_FINDINGS + 2);
    assert_eq!(
        syntax[ValidationService::LISTED_FINDINGS].message,
        "2 more syntax error(s)"
    );
    let last = syntax.last().unwrap();
    assert_eq!(
        (last.status, last.message.as_str()),
        (ValidationStatus::Warning, "Deprecated")
    );
}
