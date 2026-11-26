//! Reads WordPress file headers the way WordPress core's `get_file_data()` does:
//! only the first 8 KiB of the file are read, `\r` is normalized to `\n`, and each
//! header matches `/^(?:[ \t]*<\?(?:php)?)?[ \t\/*#@]*{Name}:(.*)$/mi`. The captured
//! value is cleaned with `_cleanup_header_comment()` (strip `*/` / `?>` and trailing text, trim).

use crate::text::{icu_lines, trim_whitespace_and_newlines};
use regex::Regex;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::LazyLock;

pub const READ_LIMIT: usize = 8 * 1024;

pub const PLUGIN_HEADERS: &[&str] = &[
    "Plugin Name",
    "Plugin URI",
    "Version",
    "Description",
    "Author",
    "Author URI",
    "Text Domain",
    "Domain Path",
    "Network",
    "Requires at least",
    "Requires PHP",
    "Update URI",
    "Requires Plugins",
    "License",
    "License URI",
    "Tested up to",
];

pub const THEME_HEADERS: &[&str] = &[
    "Theme Name",
    "Theme URI",
    "Description",
    "Author",
    "Author URI",
    "Version",
    "Template",
    "Status",
    "Tags",
    "Text Domain",
    "Domain Path",
    "Requires at least",
    "Tested up to",
    "Requires PHP",
    "License",
    "License URI",
    "Update URI",
];

/// Reads at most 8 KiB of `path` and parses the given headers. Missing headers are absent from the result.
pub fn read(path: &Path, headers: &[&str]) -> io::Result<BTreeMap<String, String>> {
    let mut data = Vec::with_capacity(READ_LIMIT);
    File::open(path)?.take(READ_LIMIT as u64).read_to_end(&mut data)?;
    Ok(parse(&String::from_utf8_lossy(&data), headers))
}

pub fn parse(contents: &str, headers: &[&str]) -> BTreeMap<String, String> {
    let text = contents.replace('\r', "\n");
    // ICU's multi-line `^`/`$` also break at U+000B, U+000C, U+0085, U+2028 and U+2029, so
    // the pattern is matched line by line on exactly those lines; the first matching line is
    // the regex's first match.
    let lines = icu_lines(&text);
    let mut result = BTreeMap::new();
    for header in headers {
        let pattern = format!(r"(?i)^(?:[ \t]*<\?(?:php)?)?[ \t/*#@]*{}:(.*)$", regex::escape(header));
        let Ok(regex) = Regex::new(&pattern) else {
            continue;
        };
        let Some(raw) = lines
            .iter()
            .find_map(|line| regex.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str()))
        else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let cleaned = cleanup_header_comment(raw);
        if !cleaned.is_empty() {
            result.insert((*header).to_string(), cleaned);
        }
    }
    result
}

static HEADER_COMMENT_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*(?:\*/|\?>).*").expect("valid"));

/// `_cleanup_header_comment()`: `trim( preg_replace( '/\s*(?:\*\/|\?>).*/', '', $str ) )`.
pub(crate) fn cleanup_header_comment(value: &str) -> String {
    let stripped = HEADER_COMMENT_END.replace_all(value, "");
    trim_whitespace_and_newlines(&stripped).to_string()
}
