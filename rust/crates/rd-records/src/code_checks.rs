//! Stored results of the optional code checks. Ported from the data types in
//! `CodeChecks/CodeCheckService.swift`; the service itself is ported later.

use rd_model::ValidationSeverity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodeCheckTool {
    PhpLint,
    PluginCheck,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCheckFinding {
    pub tool: CodeCheckTool,
    pub severity: ValidationSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
}

impl CodeCheckFinding {
    pub fn new(
        tool: CodeCheckTool,
        severity: ValidationSeverity,
        file: Option<String>,
        line: Option<i64>,
        code: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool,
            severity,
            file,
            line,
            code,
            message: message.into(),
        }
    }
}

/// Outcome of the optional code checks run on the staged release candidate.
///
/// `Default` matches the Swift initializer's defaults.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCheckReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub php_version: Option<String>,
    pub linted_files: i64,
    /// Why PHP lint did not run (no php, local PHP older than Requires PHP…); None when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub php_lint_skipped: Option<String>,
    pub plugin_check_ran: bool,
    /// Why Plugin Check did not run; None when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_check_skipped: Option<String>,
    pub findings: Vec<CodeCheckFinding>,
}

impl CodeCheckReport {
    /// Swift's `findings(_ tool:)`.
    pub fn findings_for(&self, tool: CodeCheckTool) -> Vec<&CodeCheckFinding> {
        self.findings.iter().filter(|f| f.tool == tool).collect()
    }
}
