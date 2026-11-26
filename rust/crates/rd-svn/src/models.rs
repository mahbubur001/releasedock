//! Port of `SVN/SVNModels.swift`.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SvnInstallation {
    pub executable: PathBuf,
    pub version: String,
}

impl SvnInstallation {
    /// `--password-from-stdin` exists since Subversion 1.10.
    pub fn supports_password_from_stdin(&self) -> bool {
        compare_versions(&self.version, "1.10.0") != Ordering::Less
    }
}

/// Numeric dotted-version comparison, enough for `svn --version --quiet` output
/// (`1.14.5`). Swift uses PHP's `version_compare` here, which orders such versions the
/// same way; the full port lives in `rd-versioning`.
fn compare_versions(a: &str, b: &str) -> Ordering {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(u64::MAX))
            .collect()
    }
    let (a, b) = (parts(a), parts(b));
    for index in 0..a.len().max(b.len()) {
        let ordering = a.get(index).unwrap_or(&0).cmp(b.get(index).unwrap_or(&0));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

/// Credentials supplied from the Keychain. Never logged, never persisted outside the Keychain,
/// and passed to svn only through stdin.
#[derive(Clone, PartialEq, Eq)]
pub struct SvnCredentials {
    pub username: String,
    pub password: String,
}

impl SvnCredentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

impl fmt::Debug for SvnCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SvnCredentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SvnNodeKind {
    File,
    Dir,
    None,
    Unknown,
}

impl SvnNodeKind {
    pub fn from_raw(raw: &str) -> Option<Self> {
        Some(match raw {
            "file" => Self::File,
            "dir" => Self::Dir,
            "none" => Self::None,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SvnInfo {
    pub url: String,
    pub repository_root: String,
    pub repository_uuid: Option<String>,
    pub revision: i64,
    pub kind: SvnNodeKind,
    pub last_changed_revision: Option<i64>,
    pub last_changed_author: Option<String>,
    pub last_changed_date: Option<String>,
    pub working_copy_root: Option<String>,
}

pub use rd_model::SvnItemStatus;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SvnStatusEntry {
    /// Path as reported by svn (absolute when an absolute target was given).
    pub path: String,
    pub item: SvnItemStatus,
    pub props: SvnItemStatus,
    pub copied: bool,
    pub is_tree_conflict: bool,
    /// Present with `--show-updates`: the item changed on the server.
    pub repository_item: Option<SvnItemStatus>,
    pub repository_props: Option<SvnItemStatus>,
    pub working_revision: Option<i64>,
}

impl SvnStatusEntry {
    pub fn is_out_of_date(&self) -> bool {
        let changed = |status: Option<SvnItemStatus>| {
            status.is_some_and(|s| s != SvnItemStatus::None && s != SvnItemStatus::Normal)
        };
        changed(self.repository_item) || changed(self.repository_props)
    }

    /// A local modification of any kind (excluding unversioned files, which callers handle).
    pub fn is_local_change(&self) -> bool {
        use SvnItemStatus::*;
        match self.item {
            Added | Conflicted | Deleted | Missing | Modified | Replaced | Obstructed | Incomplete | Merged => true,
            _ => self.props == Modified || self.props == Conflicted || self.is_tree_conflict,
        }
    }

    /// A text, property or tree conflict left by an update.
    pub fn is_conflict(&self) -> bool {
        self.item == SvnItemStatus::Conflicted || self.props == SvnItemStatus::Conflicted || self.is_tree_conflict
    }
}

/// `svn resolve --accept` choices ReleaseDock uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SvnConflictChoice {
    /// Replace with the local version, dropping the server's change.
    MineFull,
    /// Replace with the server version, dropping the local change.
    TheirsFull,
    /// Keep the file on disk as it is (the only choice valid for tree conflicts).
    Working,
}

impl SvnConflictChoice {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::MineFull => "mine-full",
            Self::TheirsFull => "theirs-full",
            Self::Working => "working",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SvnLogEntry {
    pub revision: i64,
    pub author: Option<String>,
    pub date: Option<String>,
    pub message: String,
    pub changed_paths: Vec<String>,
    /// The action for each changed path, in the same order: `A`, `M`, `D` or `R`.
    pub changed_path_actions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SvnListEntry {
    /// Path relative to the listed URL.
    pub name: String,
    pub kind: SvnNodeKind,
    pub size: Option<u64>,
    pub commit_revision: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SvnDepth {
    Empty,
    Files,
    Immediates,
    Infinity,
}

impl SvnDepth {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Files => "files",
            Self::Immediates => "immediates",
            Self::Infinity => "infinity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SvnErrorKind {
    AuthenticationRequired,
    AuthenticationFailed,
    Forbidden,
    Network,
    NotFound,
    OutOfDate,
    WorkingCopyLocked,
    NotAWorkingCopy,
    Conflict,
    AlreadyExists,
    /// A server-side pre-commit hook rejected the commit (WordPress.org uses these to enforce rules).
    HookRejected,
    Certificate,
    Other,
}

#[cfg(target_os = "macos")]
const NOT_INSTALLED: &str = "Subversion (svn) is not installed. Install it with Homebrew: brew install subversion";
#[cfg(windows)]
const NOT_INSTALLED: &str = "Subversion (svn) is not installed. Install a command-line client such as SlikSVN, or TortoiseSVN with its command line client tools.";
#[cfg(not(any(target_os = "macos", windows)))]
const NOT_INSTALLED: &str =
    "Subversion (svn) is not installed. Install it with your package manager, for example: sudo apt install subversion";

#[derive(Debug, thiserror::Error)]
pub enum SvnError {
    #[error("{}", NOT_INSTALLED)]
    NotInstalled,
    #[error("{}: {message}", kind.user_description())]
    CommandFailed {
        kind: SvnErrorKind,
        codes: Vec<String>,
        message: String,
        command: String,
    },
    /// The commit process timed out or was interrupted; the commit may or may not exist on the server.
    #[error(
        "The commit did not finish normally ({0}). Its outcome is unknown; ReleaseDock will inspect the repository before doing anything else."
    )]
    CommitOutcomeUncertain(String),
    #[error("Unexpected svn output: {0}")]
    UnexpectedOutput(String),
    /// The svn process could not run or was stopped (Swift rethrows `ProcessRunnerError` as is).
    #[error(transparent)]
    Process(#[from] crate::process_runner::ProcessRunnerError),
    /// A temporary file (commit message, `--targets` list) could not be written.
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl SvnError {
    pub fn kind(&self) -> Option<SvnErrorKind> {
        match self {
            Self::CommandFailed { kind, .. } => Some(*kind),
            _ => None,
        }
    }
}

impl SvnErrorKind {
    pub fn user_description(self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "SVN credentials are required",
            Self::AuthenticationFailed => "SVN authentication failed",
            Self::Forbidden => "Access forbidden",
            Self::Network => "Network error",
            Self::NotFound => "Not found",
            Self::OutOfDate => "Working copy is out of date",
            Self::WorkingCopyLocked => "Working copy is locked",
            Self::NotAWorkingCopy => "Not an SVN working copy",
            Self::Conflict => "Conflict",
            Self::AlreadyExists => "Already exists",
            Self::HookRejected => "The repository rejected the commit",
            Self::Certificate => "Server certificate could not be verified",
            Self::Other => "SVN error",
        }
    }

    /// Classifies svn stderr by its `E`/`W` error codes (stable across locales).
    pub fn classify(codes: &[String], stderr: &str) -> SvnErrorKind {
        let set: HashSet<&str> = codes.iter().map(String::as_str).collect();
        let any = |list: &[&str]| list.iter().any(|code| set.contains(code));
        let lower = stderr.to_lowercase();
        if set.contains("E215004") || lower.contains("no more credentials") {
            return Self::AuthenticationRequired;
        }
        if set.contains("E170001") || lower.contains("authorization failed") {
            return Self::AuthenticationFailed;
        }
        if set.contains("E175013") || lower.contains("403 forbidden") {
            return Self::Forbidden;
        }
        if set.contains("E165001") || lower.contains("commit blocked by pre-commit hook") {
            return Self::HookRejected;
        }
        if set.contains("E230001") || lower.contains("server certificate verification failed") {
            return Self::Certificate;
        }
        if any(&["E155011", "E160024", "E160028", "E170004", "E155036"])
            || lower.contains("out of date")
            || lower.contains("out-of-date")
        {
            return Self::OutOfDate;
        }
        if set.contains("E155004") || set.contains("E155037") {
            return Self::WorkingCopyLocked;
        }
        if set.contains("E155007") {
            return Self::NotAWorkingCopy;
        }
        if set.contains("E155015") || set.contains("E155035") || lower.contains("conflict") {
            return Self::Conflict;
        }
        if set.contains("E160020") || set.contains("E150002") || lower.contains("already exists") {
            return Self::AlreadyExists;
        }
        if any(&[
            "E170000", "W170000", "E160013", "W160013", "E200009", "E155010", "W155010",
        ]) {
            return Self::NotFound;
        }
        if any(&[
            "E670002", "E670008", "E170013", "E175002", "E175012", "E120171", "E120108", "E000060", "E000061",
            "E000065", "E731001", "E731004",
        ]) || lower.contains("unable to connect")
            || lower.contains("timed out")
            || lower.contains("could not resolve")
        {
            return Self::Network;
        }
        Self::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_from_stdin_needs_1_10() {
        let installation = |version: &str| SvnInstallation {
            executable: PathBuf::from("svn"),
            version: version.into(),
        };
        assert!(installation("1.14.5").supports_password_from_stdin());
        assert!(installation("1.10.0").supports_password_from_stdin());
        assert!(!installation("1.9.7").supports_password_from_stdin());
    }

    #[test]
    fn credentials_debug_hides_the_password() {
        let text = format!("{:?}", SvnCredentials::new("alice", "s3cret"));
        assert!(text.contains("alice"));
        assert!(!text.contains("s3cret"));
    }

    #[test]
    fn error_kind_serializes_as_swift_raw_value() {
        assert_eq!(
            serde_json::to_string(&SvnErrorKind::AuthenticationRequired).unwrap(),
            "\"authenticationRequired\""
        );
        assert_eq!(
            serde_json::to_string(&SvnErrorKind::NotAWorkingCopy).unwrap(),
            "\"notAWorkingCopy\""
        );
        assert_eq!(
            serde_json::from_str::<SvnNodeKind>("\"dir\"").unwrap(),
            SvnNodeKind::Dir
        );
        assert_eq!(
            serde_json::to_string(&SvnItemStatus::Unversioned).unwrap(),
            "\"unversioned\""
        );
    }
}
