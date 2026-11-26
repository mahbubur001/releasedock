//! Ported from `CodeCheckTests.swift`: `CodeCheckParsingTests` and `CodeCheckServiceTests`
//! (validation and pipeline tests are listed in the README). Tests that need PHP skip
//! themselves (print `skipped: …` and pass) when it is not installed.

mod common;

use async_trait::async_trait;
use common::{TempDir, make_product};
use rd_integrations::{
    CodeCheckConfiguration, CodeCheckConfigurationStore, CodeCheckService, CodeCheckTool, CodeChecking,
};
use rd_model::{ProductType, ValidationSeverity};
use rd_svn::{ProcessRequest, ProcessResult, ProcessRunnerError, ProcessRunning};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// MARK: CodeCheckParsingTests

#[test]
fn parses_lint_output_once_despite_duplicates() {
    let output = "PHP Parse error:  syntax error, unexpected end of file, expecting \",\" or \";\" in includes/bad.php on line 3\n\nParse error: syntax error, unexpected end of file, expecting \",\" or \";\" in includes/bad.php on line 3\nErrors parsing includes/bad.php\nDeprecated: Using ${var} in strings is deprecated in old.php on line 7\nNo syntax errors detected in ok.php";
    let findings = CodeCheckService::parse_lint(output);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(findings[0].file.as_deref(), Some("includes/bad.php"));
    assert_eq!(findings[0].line, Some(3));
    assert_eq!(findings[0].severity, ValidationSeverity::Error);
    assert!(findings[0].message.starts_with("Parse error: syntax error"));
    assert_eq!(findings[1].severity, ValidationSeverity::Warning);
    assert_eq!(findings[1].file.as_deref(), Some("old.php"));
}

#[test]
fn parses_lint_output_with_windows_line_endings() {
    // Rust-only: PHP on Windows ends lines with \r\n.
    let output = "PHP Parse error:  syntax error, unexpected end of file in includes\\bad.php on line 3\r\nNo syntax errors detected in ok.php\r\n";
    let findings = CodeCheckService::parse_lint(output);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file.as_deref(), Some("includes\\bad.php"));
    assert_eq!(findings[0].message, "Parse error: syntax error, unexpected end of file");
}

#[test]
fn parses_plugin_check_strict_json() {
    let output = "Deprecated: Case statements followed by a semicolon (;) are deprecated in phar:///wp/vendor/a.php on line 369\nNotice: [plugin-check] something odd\n[{\"file\":\"sample-plugin.php\",\"line\":12,\"type\":\"ERROR\",\"code\":\"WordPress.Security.EscapeOutput.OutputNotEscaped\",\"message\":\"All output should be run through an escaping function\"},{\"file\":\"readme.txt\",\"line\":0,\"type\":\"WARNING\",\"code\":\"outdated_tested_upto_header\",\"message\":\"Tested up to: 6.8 < 7.1.\"}]";
    let findings = CodeCheckService::parse_plugin_check(output).unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].severity, ValidationSeverity::Error);
    assert_eq!(
        findings[0].code.as_deref(),
        Some("WordPress.Security.EscapeOutput.OutputNotEscaped")
    );
    assert_eq!(findings[1].severity, ValidationSeverity::Warning);
    assert_eq!(CodeCheckService::parse_plugin_check("[]"), Some(vec![]));
    assert_eq!(
        CodeCheckService::parse_plugin_check("Error: This does not seem to be a WordPress installation."),
        None
    );
}

// MARK: CodeCheckServiceTests

fn service(wordpress_path: Option<&str>) -> Option<CodeCheckService> {
    if CodeCheckService::find_php().is_none() {
        eprintln!("skipped: php not installed");
        return None;
    }
    Some(
        CodeCheckService::new().with_configuration(CodeCheckConfigurationStore::new(CodeCheckConfiguration::new(
            wordpress_path.map(str::to_owned),
        ))),
    )
}

#[tokio::test]
async fn lints_with_real_php() {
    let Some(service) = service(None) else { return };
    let dir = TempDir::new();
    dir.write("p/ok.php", "<?php\necho 'ok';\n");
    dir.write("p/includes/bad.php", "<?php\necho 'a'\n");
    let report = service
        .check(
            &dir.path("p"),
            &["ok.php".into(), "includes/bad.php".into()],
            &make_product(ProductType::Plugin),
            Some("7.4"),
        )
        .await;
    assert_eq!(report.php_lint_skipped, None);
    assert!(report.php_version.is_some());
    assert_eq!(report.linted_files, 2);
    let errors: Vec<_> = report
        .findings_for(CodeCheckTool::PhpLint)
        .into_iter()
        .filter(|f| f.severity == ValidationSeverity::Error)
        .collect();
    assert_eq!(
        errors.iter().map(|f| f.file.as_deref()).collect::<Vec<_>>(),
        vec![Some("includes/bad.php")],
        "{:?}",
        report.findings
    );
    assert_eq!(errors.first().and_then(|f| f.line), Some(3));
    assert_eq!(
        report.plugin_check_skipped.as_ref().map(|s| s.contains("Settings")),
        Some(true),
        "Plugin Check is opt-in"
    );
}

#[tokio::test]
async fn skips_lint_when_local_php_is_older_than_required() {
    let Some(service) = service(None) else { return };
    let dir = TempDir::new();
    dir.write("p/ok.php", "<?php\n");
    let report = service
        .check(
            &dir.path("p"),
            &["ok.php".into()],
            &make_product(ProductType::Plugin),
            Some("99.0"),
        )
        .await;
    assert!(
        report
            .php_lint_skipped
            .as_deref()
            .is_some_and(|s| s.contains("older than Requires PHP 99.0"))
    );
    assert_eq!(report.linted_files, 0);
}

#[tokio::test]
async fn plugin_check_is_not_run_for_themes() {
    let Some(service) = service(Some("/tmp/wordpress")) else {
        return;
    };
    let dir = TempDir::new();
    dir.write("t/functions.php", "<?php\n");
    let mut theme = make_product(ProductType::Plugin);
    theme.product_type = ProductType::Theme;
    let report = service
        .check(&dir.path("t"), &["functions.php".into()], &theme, None)
        .await;
    assert!(!report.plugin_check_ran);
    assert_eq!(report.plugin_check_skipped, None);
}

// MARK: Rust-only: requests and missing tools, with a scripted runner

/// Answers `php -r` with a version, `php -l` with lint output and `wp` with JSON.
struct ScriptedRunner {
    version: &'static str,
    wp_output: &'static str,
    requests: Mutex<Vec<ProcessRequest>>,
}

#[async_trait]
impl ProcessRunning for ScriptedRunner {
    async fn run(&self, request: ProcessRequest) -> Result<ProcessResult, ProcessRunnerError> {
        self.requests.lock().expect("lock").push(request.clone());
        let stdout = if request.arguments.contains(&"-r".to_owned()) {
            self.version.to_owned()
        } else if request.arguments.contains(&"-l".to_owned()) {
            "No syntax errors detected".to_owned()
        } else {
            self.wp_output.to_owned()
        };
        Ok(ProcessResult {
            exit_code: 0,
            standard_output: stdout.into_bytes(),
            standard_error: Vec::new(),
            duration: Duration::ZERO,
            display_command: request.display_command(),
        })
    }
}

#[tokio::test]
async fn batches_lint_and_runs_plugin_check() {
    let runner = Arc::new(ScriptedRunner {
        version: "8.4.1",
        wp_output: r#"[{"file":"a.php","line":1,"type":"WARNING","code":"x","message":"y"}]"#,
        requests: Mutex::new(Vec::new()),
    });
    let service = CodeCheckService::new()
        .with_runner(runner.clone())
        .with_php(Some(PathBuf::from("/fake/php")))
        .with_wp(Some(PathBuf::from("/fake/wp")))
        .with_configuration(CodeCheckConfigurationStore::new(CodeCheckConfiguration::new(Some(
            "  /srv/wordpress ".into(),
        ))));
    let files: Vec<String> = (0..150).map(|i| format!("f{i}.php")).collect();
    let report = service
        .check(
            std::path::Path::new("/candidate"),
            &files,
            &make_product(ProductType::Plugin),
            None,
        )
        .await;
    assert_eq!(report.linted_files, 150);
    assert!(report.plugin_check_ran);
    assert_eq!(report.findings_for(CodeCheckTool::PluginCheck).len(), 1);
    let requests = runner.requests.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        4,
        "version, two lint batches of 100 and 50, Plugin Check"
    );
    assert_eq!(requests[1].arguments.len(), 8 + 100);
    assert_eq!(requests[2].arguments.len(), 8 + 50);
    assert_eq!(
        requests[3].arguments,
        vec![
            "--path=/srv/wordpress",
            "plugin",
            "check",
            "/candidate",
            "--slug=sample-plugin",
            "--mode=new",
            "--format=strict-json",
            "--fields=file,line,type,code,message",
        ]
    );
}

#[tokio::test]
async fn missing_tools_are_reported_as_skipped() {
    let service =
        CodeCheckService::new()
            .with_php(None)
            .with_wp(None)
            .with_configuration(CodeCheckConfigurationStore::new(CodeCheckConfiguration::new(Some(
                "/srv/wordpress".into(),
            ))));
    let report = service
        .check(
            std::path::Path::new("/candidate"),
            &["a.php".into()],
            &make_product(ProductType::Plugin),
            None,
        )
        .await;
    assert!(report.php_lint_skipped.unwrap().starts_with("PHP is not installed"));
    assert!(
        report
            .plugin_check_skipped
            .unwrap()
            .starts_with("WP-CLI is not installed")
    );
}
