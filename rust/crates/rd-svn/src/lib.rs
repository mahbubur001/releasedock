//! Subversion support for ReleaseDock: a shell-free process runner, a small XML tree for
//! `svn --xml` output, the SVN models and service, and the command transcript.
//! Ported from `ReleaseDockCore/SVN`, `Support/ProcessRunner.swift` and `Support/XMLTree.swift`.

pub mod models;
pub mod process_runner;
pub mod service;
pub mod transcript;
pub mod xml_tree;

pub use models::*;
pub use process_runner::{
    ProcessRequest, ProcessResult, ProcessRunner, ProcessRunnerError, ProcessRunning, with_cancellation,
};
pub use service::{SvnService, SvnServicing};
pub use transcript::{SvnTranscribing, SvnTranscript, TranscriptEntry, TranscriptScope};
pub use xml_tree::{XmlElement, XmlError};

pub use tokio_util::sync::CancellationToken;
