//! The summary line of a release commit, written with tokens the pipeline fills in once
//! the version is known: `{name}`, `{slug}`, `{version}`, `{previous}`. The commit marker
//! that recovery searches for is always appended after it.

use regex::Regex;
use std::sync::LazyLock;

pub const TOKENS: [&str; 4] = ["{name}", "{slug}", "{version}", "{previous}"];
pub const MAXIMUM_LENGTH: usize = 300;
pub const PRESETS: [&str; 4] = [
    "Release {name} {version}",
    "Release {version}",
    "Tagging version {version}",
    "Update to {version}",
];
pub const DEFAULT_VERSION: &str = "Release {name} {version}";
pub const DEFAULT_README_UPDATE: &str = "Update readme for {name} {version}";

static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[a-z]+\}").expect("valid"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").expect("valid"));

/// Why the template can't be used, or None.
pub fn problem(template: &str) -> Option<String> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Some("Enter a commit message.".into());
    }
    if trimmed.chars().count() > MAXIMUM_LENGTH {
        return Some(format!("Keep the commit message under {MAXIMUM_LENGTH} characters."));
    }
    if let Some(unknown) = TOKEN
        .find_iter(trimmed)
        .map(|m| m.as_str())
        .find(|t| !TOKENS.contains(t))
    {
        return Some(format!("{unknown} is not a token. Use {}.", TOKENS.join(", ")));
    }
    None
}

pub fn render(template: &str, name: &str, slug: &str, version: Option<&str>, previous: Option<&str>) -> String {
    let text = template
        .replace("{name}", name)
        .replace("{slug}", slug)
        .replace("{version}", version.unwrap_or(""))
        .replace("{previous}", previous.unwrap_or(""));
    SPACES.replace_all(&text, " ").trim().to_owned()
}
