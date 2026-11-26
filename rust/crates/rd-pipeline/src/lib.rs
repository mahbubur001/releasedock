//! The release pipeline. Ported from `ReleaseDockCore`:
//!
//! - [`pipeline`]: `Release/ReleasePipelineService.swift` (`ReleasePipelineService`,
//!   `PipelineDependencies`, `PipelineEvent`).
//! - [`strategy`], [`plugin_strategy`], [`theme_strategy`]: the SVN side of
//!   `Release/ReleaseStrategy.swift`, `PluginReleaseStrategy.swift` and
//!   `ThemeReleaseStrategy.swift` (the pure validation rules are in `rd-validation`).
//! - [`working_copy`]: `WorkingCopyOperations` and `FileMirror` from `ReleaseStrategy.swift`.
//! - [`staging`] and [`error`]: `Release/ReleaseStagingService.swift` (`ReleaseStaging`,
//!   `ReleaseStagingService`, `ReleaseError`).
//! - [`app_services`]: `Services/AppServices.swift` (`NotificationServicing`, `UpdateServicing`).
//! - [`release_zip`]: the GitHub release asset (Swift runs `/usr/bin/ditto`).
//!
//! The safety properties are those of the Swift pipeline: one commit per release; approval
//! binds to the preview fingerprint and is void when the working copy changes; the commit
//! message carries a marker that recovery looks for in the SVN log, so an uncertain commit
//! is never retried blindly; interrupted releases are reconciled on launch; the working copy
//! is backed up before anything of the user's is reverted; committed files are verified by
//! SHA-256; automatic publishing passes the automation gate; a dry run never commits.

pub mod app_services;
pub mod error;
mod fs_util;
pub mod pipeline;
pub mod plugin_strategy;
pub mod release_zip;
pub mod staging;
pub mod strategy;
pub mod theme_strategy;
pub mod working_copy;

pub use app_services::{NoopNotificationService, NotificationServicing, UpdateServicing};
pub use error::ReleaseError;
pub use pipeline::{PipelineDependencies, PipelineEvent, ReleasePipelineService};
pub use staging::{ReleaseStaging, ReleaseStagingService};
pub use strategy::{
    PluginReleaseStrategy, ReleaseStrategy, RepositoryStrategy, StrategyProvider, ThemeReleaseStrategy,
};
