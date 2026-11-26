//! Port of `SVN/SVNTranscript.swift`: a per-product JSON-lines log of the `svn` commands
//! ReleaseDock ran. The file format is shared with the Swift app.

use crate::process_runner::{ProcessResult, home_directory};
use rd_model::{Id, SwiftDate};
use rd_support::redact::redact;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

tokio::task_local! {
    static SCOPE: TranscriptScope;
}

/// The operation an `svn` command belongs to. Set with [`SvnTranscript::with_scope`]
/// around a pipeline operation; commands outside any scope are not recorded.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TranscriptScope {
    pub product_id: Id,
    pub release_id: Option<Id>,
    /// What the user asked for, e.g. "Prepare" or "Update working copy".
    pub operation: String,
}

impl TranscriptScope {
    pub fn new(product_id: Id, release_id: Option<Id>, operation: impl Into<String>) -> Self {
        Self {
            product_id,
            release_id,
            operation: operation.into(),
        }
    }
}

/// One `svn` command as it ran: arguments (never a password; it goes through stdin),
/// exit code, duration and the end of its output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub id: Id,
    #[serde(with = "crate::iso8601")]
    pub date: SwiftDate,
    #[serde(rename = "productID")]
    pub product_id: Id,
    #[serde(rename = "releaseID", default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<Id>,
    pub operation: String,
    /// `svn update --set-depth infinity ~/svn/plugin/trunk`, without the global options.
    pub command: String,
    /// `None` when the command could not start or was stopped (timeout, cancel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Seconds.
    pub duration: f64,
    /// Last lines of stdout and stderr. XML, file contents and diffs are summarised by size.
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

impl TranscriptEntry {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && self.failure.is_none()
    }
}

pub trait SvnTranscribing: Send + Sync {
    fn record(&self, entry: TranscriptEntry);
    fn entries(&self, product_id: Id, release_id: Option<Id>, limit: usize) -> Vec<TranscriptEntry>;
}

/// Appends entries to one JSON-lines file per product under `Transcripts/`, keeping the
/// newest `maximum_entries`.
pub struct SvnTranscript {
    folder: PathBuf,
    maximum_entries: usize,
    lock: Mutex<()>,
}

impl SvnTranscript {
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        Self::with_maximum_entries(folder, 2000)
    }

    pub fn with_maximum_entries(folder: impl Into<PathBuf>, maximum_entries: usize) -> Self {
        Self {
            folder: folder.into(),
            maximum_entries,
            lock: Mutex::new(()),
        }
    }

    /// Runs `future` with `scope` as the current transcript scope (Swift:
    /// `SVNTranscript.$scope.withValue(scope) { … }`). Like a Swift task-local, the scope
    /// does not follow work moved to other tasks with `tokio::spawn`.
    pub async fn with_scope<F: Future>(scope: TranscriptScope, future: F) -> F::Output {
        SCOPE.scope(scope, future).await
    }

    /// The scope of the current task, if any.
    pub fn scope() -> Option<TranscriptScope> {
        SCOPE.try_with(TranscriptScope::clone).ok()
    }

    pub fn file(&self, product_id: Id) -> PathBuf {
        self.folder.join(format!("{product_id}.jsonl"))
    }

    fn append(&self, entry: &TranscriptEntry) -> std::io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        let file = self.file(entry.product_id);
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)?
            .write_all(&line)?;
        self.trim_if_needed(&file);
        Ok(())
    }

    fn read(file: &Path) -> Vec<TranscriptEntry> {
        let Ok(text) = fs::read_to_string(file) else {
            return Vec::new();
        };
        text.split('\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Rewrites the file with the newest entries once it is a quarter over the limit.
    fn trim_if_needed(&self, file: &Path) {
        let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        if size <= 256 * 1024 {
            return;
        }
        let all = Self::read(file);
        if all.len() <= self.maximum_entries + self.maximum_entries / 4 {
            return;
        }
        let mut data = Vec::new();
        for entry in &all[all.len() - self.maximum_entries..] {
            if let Ok(line) = serde_json::to_vec(entry) {
                data.extend_from_slice(&line);
                data.push(b'\n');
            }
        }
        let _ = write_atomically(file, &data);
    }

    // MARK: Building entries

    /// Commands whose stdout is data rather than a message: only its size is kept.
    pub const DATA_COMMANDS: [&'static str; 3] = ["cat", "diff", "export"];

    /// Builds the entry for one command. `duration` is in seconds.
    pub fn entry(
        scope: &TranscriptScope,
        arguments: &[String],
        _working_directory: Option<&Path>,
        result: Option<&ProcessResult>,
        failure: Option<&str>,
        duration: f64,
        date: SwiftDate,
    ) -> TranscriptEntry {
        let stdout = result.map(ProcessResult::stdout).unwrap_or_default();
        let stderr = result.map(ProcessResult::stderr).unwrap_or_default();
        let mut output = Vec::new();
        if !stdout.is_empty() {
            let is_data = arguments.iter().any(|a| a == "--xml")
                || Self::DATA_COMMANDS.contains(&arguments.first().map(String::as_str).unwrap_or(""));
            if is_data {
                let size = result.map(|r| r.standard_output.len() as u64).unwrap_or(0);
                output.push(format!("({} of output)", byte_count_file_style(size)));
            } else {
                output.push(tail(&stdout, 12));
            }
        }
        if !stderr.is_empty() {
            output.push(tail(&stderr, 12));
        }
        let home = home_directory();
        let mut command = vec!["svn".to_owned()];
        command.extend(arguments.iter().map(|a| display(a, &home)));
        TranscriptEntry {
            id: Id::new(),
            date,
            product_id: scope.product_id,
            release_id: scope.release_id,
            operation: scope.operation.clone(),
            command: redact(&command.join(" ")),
            exit_code: result.map(|r| r.exit_code),
            duration,
            output: redact(&output.join("\n")),
            failure: failure.map(redact),
        }
    }
}

impl SvnTranscribing for SvnTranscript {
    fn record(&self, entry: TranscriptEntry) {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) = self.append(&entry) {
            log::error!(target: "svn", "Could not write the SVN transcript: {error}");
        }
    }

    fn entries(&self, product_id: Id, release_id: Option<Id>, limit: usize) -> Vec<TranscriptEntry> {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut matching = Self::read(&self.file(product_id));
        if let Some(id) = release_id {
            matching.retain(|e| e.release_id == Some(id));
        }
        let skip = matching.len().saturating_sub(limit);
        matching.split_off(skip)
    }
}

fn write_atomically(file: &Path, data: &[u8]) -> std::io::Result<()> {
    let temporary = file.with_extension(format!("jsonl.{}.tmp", Id::new()));
    fs::write(&temporary, data)?;
    fs::rename(&temporary, file).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

fn tail(text: &str, lines: usize) -> String {
    let mut all: Vec<&str> = text.split('\n').collect();
    if all.last() == Some(&"") {
        all.pop();
    }
    let kept = &all[all.len().saturating_sub(lines)..];
    let prefix = if all.len() > lines {
        format!("… {} more lines\n", all.len() - lines)
    } else {
        String::new()
    };
    prefix + &kept.join("\n")
}

/// Paths under the home folder as `~/…`; arguments with spaces quoted.
fn display(argument: &str, home: &str) -> String {
    let mut value = argument.to_owned();
    if !home.is_empty() {
        let separator = std::path::MAIN_SEPARATOR;
        for prefix in [format!("{home}/"), format!("{home}{separator}")] {
            if value.starts_with(&prefix) {
                value = format!("~{}", &value[home.len()..]);
                break;
            }
        }
    }
    if value.contains(' ') {
        format!("\"{value}\"")
    } else {
        value
    }
}

/// `ByteCountFormatter.string(fromByteCount:countStyle: .file)` for the en locale:
/// decimal units, `KB` without decimals, `MB` with one, larger units with two, half-up
/// rounding and trailing zeros dropped (`1 byte`, `999 bytes`, `2 KB`, `1.2 MB`, `1.23 GB`).
pub fn byte_count_file_style(bytes: u64) -> String {
    if bytes == 0 {
        return "Zero KB".to_owned();
    }
    if bytes == 1 {
        return "1 byte".to_owned();
    }
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    const UNITS: [(&str, u32); 5] = [("KB", 0), ("MB", 1), ("GB", 2), ("TB", 2), ("PB", 2)];
    let value = u128::from(bytes);
    let mut index = 0;
    while index + 1 < UNITS.len() && value >= 1000u128.pow(index as u32 + 2) {
        index += 1;
    }
    loop {
        let (name, decimals) = UNITS[index];
        let unit = 1000u128.pow(index as u32 + 1);
        let scale = 10u128.pow(decimals);
        let scaled = (value * scale * 2 + unit) / (unit * 2);
        if scaled >= 1000 * scale && index + 1 < UNITS.len() {
            index += 1;
            continue;
        }
        let whole = scaled / scale;
        let fraction = scaled % scale;
        if fraction == 0 {
            return format!("{whole} {name}");
        }
        let digits = format!("{fraction:0width$}", width = decimals as usize);
        return format!("{whole}.{} {name}", digits.trim_end_matches('0'));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counts_match_foundation() {
        let cases = [
            (1, "1 byte"),
            (2, "2 bytes"),
            (999, "999 bytes"),
            (1000, "1 KB"),
            (1499, "1 KB"),
            (1500, "2 KB"),
            (1600, "2 KB"),
            (2500, "3 KB"),
            (999_499, "999 KB"),
            (999_500, "1 MB"),
            (1_000_000, "1 MB"),
            (1_049_999, "1 MB"),
            (1_050_000, "1.1 MB"),
            (1_234_567, "1.2 MB"),
            (9_950_000, "10 MB"),
            (12_345_678, "12.3 MB"),
            (999_950_000, "1 GB"),
            (1_234_567_890, "1.23 GB"),
            (12_345_678_901, "12.35 GB"),
        ];
        for (bytes, expected) in cases {
            assert_eq!(byte_count_file_style(bytes), expected, "{bytes}");
        }
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc\n", 2), "… 1 more lines\nb\nc");
        assert_eq!(tail("a\nb", 5), "a\nb");
    }

    #[test]
    fn display_shortens_home_and_quotes_spaces() {
        assert_eq!(display("/Users/me/svn/my plugin", "/Users/me"), "\"~/svn/my plugin\"");
        assert_eq!(display("/Users/meme/x", "/Users/me"), "/Users/meme/x");
        assert_eq!(display("trunk", "/Users/me"), "trunk");
    }
}
