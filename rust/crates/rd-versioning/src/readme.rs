//! Parser for WordPress readme files following the WordPress.org plugin directory
//! parser (`plugin-directory/readme/class-parser.php`) where it matters for releases:
//!
//! - The name is the first non-blank line trimmed of `#`, `=` and whitespace.
//! - Header lines follow; keys are case-insensitive and split on the first `:`, with
//!   ` \t*-` trimmed from keys and values. Parsing stops at the first non-blank line that
//!   has no `:` or starts with `#`/`=`, and at an unknown key that follows a blank line
//!   (that line starts the short description). Aliases: `tested` → `tested up to`,
//!   `requires` → `requires at least`.
//! - Sections start with `== Title ==` or a Markdown `## Title`.
//!
//! It does not reproduce every rule of the official readme validator; ReleaseDock checks
//! the release-relevant fields and points users to the validator for the rest.

use crate::text::{icu_lines, trim_chars, trim_whitespace, trim_whitespace_and_newlines};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

/// Parsed WordPress `readme.txt`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordPressReadme {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Header fields keyed by their lowercased field name (e.g. `stable tag`).
    pub fields: BTreeMap<String, String>,
    /// Section bodies keyed by lowercased section title (e.g. `changelog`).
    pub sections: BTreeMap<String, String>,
    /// Text between the header block and the first section, whitespace-collapsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    /// Size of the file in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_count: Option<usize>,
    /// Lines in the header block with an unknown key. A short description containing
    /// ":" lands here, because WordPress.org reads it as a header too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_header_lines: Option<Vec<String>>,
}

impl WordPressReadme {
    pub fn new(name: Option<String>, fields: BTreeMap<String, String>, sections: BTreeMap<String, String>) -> Self {
        Self {
            name,
            fields,
            sections,
            short_description: None,
            byte_count: None,
            unknown_header_lines: None,
        }
    }

    fn field(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }

    /// Stable tag as WordPress.org sanitizes it.
    pub fn stable_tag(&self) -> Option<String> {
        self.field("stable tag").map(sanitize_stable_tag)
    }

    pub fn raw_stable_tag(&self) -> Option<&str> {
        self.field("stable tag")
    }

    pub fn tested_up_to(&self) -> Option<&str> {
        self.field("tested up to")
    }

    pub fn requires_at_least(&self) -> Option<&str> {
        self.field("requires at least")
    }

    pub fn requires_php(&self) -> Option<&str> {
        self.field("requires php")
    }

    pub fn contributors(&self) -> Vec<String> {
        list(self.field("contributors"))
    }

    pub fn tags(&self) -> Vec<String> {
        list(self.field("tags"))
    }

    pub fn license(&self) -> Option<&str> {
        self.field("license")
    }

    /// Whether the changelog section has an entry heading that mentions `version`
    /// (`= 1.2.3 =`, `= 1.2.3 - 2026-01-01 =`, `### 1.2.3`, `**1.2.3**` …).
    pub fn changelog_mentions(&self, version: &str) -> bool {
        let Some(changelog) = self.sections.get("changelog") else {
            return false;
        };
        // The version must not be immediately followed by another version character,
        // so `1.2` does not match a `1.2.3` heading. Swift uses the ICU pattern
        // `(?m)^…v?{version}(?![0-9A-Za-z.\-+])`; without look-ahead, "not followed by"
        // is "followed by another character, or the end of the line", matched per ICU line.
        let pattern = format!(
            r"^[ \t]*(?:=+|#+|\*+)?[ \t]*v?{}(?:[^0-9A-Za-z.\-+]|$)",
            regex::escape(version)
        );
        let Ok(regex) = Regex::new(&pattern) else {
            return false;
        };
        icu_lines(changelog).iter().any(|line| regex.is_match(line))
    }

    /// The `== Upgrade Notice ==` entry for `version`.
    pub fn upgrade_notice(&self, version: &str) -> Option<String> {
        self.entry("upgrade notice", version)
    }

    /// The `== Changelog ==` entry for `version`.
    pub fn changelog_entry(&self, version: &str) -> Option<String> {
        self.entry("changelog", version)
    }

    /// The text under the `= version =` (or `= version - date =`, `### version`) heading of
    /// a section, up to the next heading.
    pub fn entry(&self, section_title: &str, version: &str) -> Option<String> {
        let section = self.sections.get(section_title)?;
        let mut collecting = false;
        let mut lines: Vec<&str> = Vec::new();
        for line in section.split('\n') {
            let trimmed = trim_whitespace(line);
            let is_heading = (trimmed.starts_with('=') && trimmed.ends_with('=') && trimmed.chars().count() > 2)
                || trimmed.starts_with('#');
            if is_heading {
                if collecting {
                    break;
                }
                let mut title = trim_chars(trimmed, "=#*[] \t");
                if let Some(rest) = title.strip_prefix('v') {
                    title = rest;
                }
                // "1.2.3 - 2026-11-24", "1.2.3 (2026-11-24)": the version is the first word.
                let first = title
                    .split([' ', '\t', '('])
                    .find(|word| !word.is_empty())
                    .unwrap_or(title);
                collecting = first == version;
                continue;
            }
            if collecting {
                lines.push(line);
            }
        }
        let text = lines.join("\n");
        let text = trim_whitespace_and_newlines(&text);
        if text.is_empty() { None } else { Some(text.to_string()) }
    }
}

fn list(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or("")
        .split(',')
        .map(trim_whitespace)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn header_alias(key: &str) -> Option<&'static str> {
    match key {
        "tested" => Some("tested up to"),
        "requires" => Some("requires at least"),
        _ => None,
    }
}

const KNOWN_HEADERS: [&str; 9] = [
    "tested up to",
    "requires at least",
    "requires php",
    "tags",
    "contributors",
    "donate link",
    "stable tag",
    "license",
    "license uri",
];

pub fn parse(contents: &str) -> WordPressReadme {
    let text = contents
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{FEFF}', "");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut index = 0;

    // Name line.
    let mut name = None;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if trim_whitespace(line).is_empty() {
            continue;
        }
        let trimmed = trim_chars(line, "#= \t\0\u{0B}");
        name = (!trimmed.is_empty()).then(|| trimmed.to_string());
        break;
    }

    // Header block.
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut unknown_header_lines: Vec<String> = Vec::new();
    let mut last_line_was_blank = false;
    const KEY_TRIM: &str = " \t*-\r\n";
    const VALUE_TRIM: &str = " \t*-\r\n<>";
    while index < lines.len() {
        let line = lines[index];
        let trimmed = trim_whitespace(line);
        let colon = line
            .find(':')
            .filter(|_| !line.starts_with('#') && !line.starts_with('='));
        let Some(colon) = colon else {
            if trimmed.is_empty() {
                last_line_was_blank = true;
                index += 1;
                continue;
            }
            break;
        };
        let key = trim_chars(&line[..colon], KEY_TRIM).to_lowercase();
        let key = header_alias(&key).map(str::to_string).unwrap_or(key);
        let value = trim_chars(&line[colon + 1..], VALUE_TRIM);
        if KNOWN_HEADERS.contains(&key.as_str()) {
            fields.entry(key).or_insert_with(|| value.to_string());
        } else if last_line_was_blank {
            break; // The short description, as on WordPress.org.
        } else {
            unknown_header_lines.push(trimmed.to_string());
        }
        last_line_was_blank = false;
        index += 1;
    }

    // Sections. Text before the first section is the short description.
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut current_section: Option<String> = None;
    let mut body: Vec<&str> = Vec::new();
    let mut intro: Vec<&str> = Vec::new();
    fn flush<'a>(
        current: Option<&str>,
        body: &mut Vec<&'a str>,
        intro: &mut Vec<&'a str>,
        sections: &mut BTreeMap<String, String>,
    ) {
        match current {
            Some(section) => {
                if !sections.contains_key(section) {
                    let joined = body.join("\n");
                    sections.insert(section.to_string(), trim_whitespace_and_newlines(&joined).to_string());
                }
            }
            None => *intro = body.clone(),
        }
        body.clear();
    }
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        let trimmed = trim_whitespace(line);
        if let Some(title) = section_title(trimmed) {
            flush(current_section.as_deref(), &mut body, &mut intro, &mut sections);
            current_section = Some(title.to_lowercase());
            continue;
        }
        body.push(line);
    }
    flush(current_section.as_deref(), &mut body, &mut intro, &mut sections);
    let short = intro.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    WordPressReadme {
        name,
        fields,
        sections,
        short_description: (!short.is_empty()).then_some(short),
        byte_count: Some(contents.len()),
        unknown_header_lines: (!unknown_header_lines.is_empty()).then_some(unknown_header_lines),
    }
}

/// Reads and parses a readme file; invalid UTF-8 is replaced like Swift's `String(decoding:)`.
pub fn parse_file(path: &Path) -> io::Result<WordPressReadme> {
    let data = std::fs::read(path)?;
    Ok(parse(&String::from_utf8_lossy(&data)))
}

/// `sanitize_stable_tag()`: trim quotes, strip a leading `tags/` or `/tags/`, drop
/// characters outside `[A-Za-z0-9_.-]`, and prefix `0` to a leading `.`.
pub fn sanitize_stable_tag(raw: &str) -> String {
    let mut tag = trim_chars(raw, "\"' \t");
    for prefix in ["/tags/", "tags/"] {
        if let Some(rest) = tag.strip_prefix(prefix) {
            tag = rest;
            break;
        }
    }
    let mut tag: String = tag
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect();
    if tag.starts_with('.') {
        tag.insert(0, '0');
    }
    tag
}

fn section_title(line: &str) -> Option<&str> {
    if line.starts_with("==") && !line.starts_with("===") && line.ends_with("==") && line.chars().count() > 4 {
        let title = trim_chars(line, "= \t");
        return (!title.is_empty()).then_some(title);
    }
    if line.starts_with("## ") && !line.starts_with("### ") {
        let title = trim_chars(line, "#= \t");
        return (!title.is_empty()).then_some(title);
    }
    None
}
