//! Stored outcome of a product's build command. Ported from the data type in
//! `Release/BuildService.swift`; the service itself is ported later.

use serde::{Deserialize, Serialize};

/// Outcome of a product's build command, run on a copy of the development folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub command: String,
    pub exit_code: i32,
    pub duration_seconds: f64,
    /// The end of the combined output, for the report.
    pub output_tail: String,
    /// The built copy that is inspected and released.
    pub built_path: String,
}

impl BuildResult {
    pub fn new(
        command: impl Into<String>,
        exit_code: i32,
        duration_seconds: f64,
        output_tail: impl Into<String>,
        built_path: impl Into<String>,
    ) -> Self {
        Self {
            command: command.into(),
            exit_code,
            duration_seconds,
            output_tail: output_tail.into(),
            built_path: built_path.into(),
        }
    }
}
