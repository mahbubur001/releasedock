//! Services that talk to external tools and web services. Ported from `ReleaseDockCore`:
//!
//! - [`git`]: read-only `git` inspection of a development folder (`Git/GitService.swift`).
//! - [`build`]: a product's build command on a copy of its folder (`Release/BuildService.swift`).
//! - [`code_checks`]: `php -l` and `wp plugin check` (`CodeChecks/CodeCheckService.swift`).
//! - [`wordpress_org`]: WordPress.org API lookups (`WordPressOrg/WordPressOrgService.swift`).
//! - [`github`] / [`github_release_publisher`]: GitHub REST client and the release publisher
//!   (`GitHub/GitHubService.swift`, `GitHub/GitHubReleasePublisher.swift`).
//! - [`repository_setup`]: connects a product to SVN (`Services/RepositorySetupService.swift`),
//!   with the remote inspection and sparse checkout it needs in [`repository_layout`]
//!   (the repository parts of `Release/*ReleaseStrategy.swift`).
//!
//! External programs run through `rd-svn`'s shell-free [`rd_svn::ProcessRunning`]; HTTP uses
//! `reqwest` with rustls.

pub mod build;
pub mod code_checks;
pub mod discovery;
pub mod git;
pub mod github;
pub mod github_release_publisher;
mod http;
pub mod repository_layout;
pub mod repository_setup;
mod url_text;
pub mod wordpress_org;

pub use build::{BuildError, BuildService, Building};
pub use code_checks::{CodeCheckConfiguration, CodeCheckConfigurationStore, CodeCheckService, CodeChecking};
pub use git::{GitService, GitServicing};
pub use github::{
    CreateRelease, GitHubAsset, GitHubError, GitHubRelease, GitHubReleasing, GitHubRepository, GitHubService,
    GitHubServicing, RateLimitStatus, ReleaseChannel, ReleaseCheckCache,
};
pub use github_release_publisher::GitHubReleasePublisher;
pub use repository_setup::{
    RepositoryDescription, RepositorySetupError, RepositorySetupService, WorkingCopyDescription,
};
pub use wordpress_org::{WordPressBranch, WordPressOrgError, WordPressOrgService, WordPressOrgServicing};

// Stored result types these services produce; they live in `rd-records`.
pub use rd_records::{
    BuildResult, CodeCheckFinding, CodeCheckReport, CodeCheckTool, DirectoryListing, GitHubReleaseResult,
    GitHubReleaseSettings, GitRepositoryState,
};
