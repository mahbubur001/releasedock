//! Release validation. Ported from `ReleaseDockCore/Validation` (`ValidationService.swift`,
//! `ReadmeRules.swift`) plus the pure helpers it needs from files that are not ported yet:
//! the validation rules of `Release/PluginReleaseStrategy.swift` and
//! `Release/ThemeReleaseStrategy.swift`, `StrategyProvider` from `Release/ReleaseStrategy.swift`,
//! and `WordPressBranch` from `WordPressOrg/WordPressOrgService.swift`.
//!
//! Validation is a pure function of a [`ValidationContext`]; gathering the context (files,
//! SVN, Git, WordPress.org) happens elsewhere. Every message and suggestion is the Swift text.

pub mod context;
pub mod plugin_strategy;
pub mod readme_rules;
pub mod service;
pub mod strategy;
mod text;
pub mod theme_strategy;
pub mod wordpress_branch;

pub use context::ValidationContext;
pub use service::{ValidationService, ValidationServicing};
pub use strategy::{PluginReleaseStrategy, ReleaseStrategy, StrategyProvider, ThemeReleaseStrategy};
pub use wordpress_branch::WordPressBranch;

use rd_model::ValidationResult;

/// Swift's `file:` and `fix:` arguments are optional; these take an `Option`.
pub(crate) trait ResultExt {
    fn file_opt(self, path: Option<&str>) -> Self;
    fn fix_opt(self, suggestion: Option<&str>) -> Self;
}

impl ResultExt for ValidationResult {
    fn file_opt(mut self, path: Option<&str>) -> Self {
        self.file_path = path.map(str::to_owned);
        self
    }

    fn fix_opt(mut self, suggestion: Option<&str>) -> Self {
        self.suggestion = suggestion.map(str::to_owned);
        self
    }
}
