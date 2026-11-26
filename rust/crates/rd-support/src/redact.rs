//! Removes anything that looks like a credential from free-form text before it is
//! persisted (release events, diagnostics, transcripts). Defence in depth: secrets are
//! never intentionally placed in such text.

use regex::Regex;
use std::sync::LazyLock;

static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // --password VALUE / --password=VALUE
        (r"(--password[= ])\S+", "${1}<redacted>"),
        // Authorization headers
        (r"(?i)(authorization:\s*(?:bearer|token|basic)\s+)\S+", "${1}<redacted>"),
        // GitHub tokens
        (r"\b(gh[pousr]_|github_pat_)[A-Za-z0-9_]{16,}", "${1}<redacted>"),
        // user:password@host in URLs
        (r"(://[^/\s:@]+:)[^@\s/]+@", "${1}<redacted>@"),
    ]
    .into_iter()
    .map(|(pattern, template)| (Regex::new(pattern).expect("valid redaction pattern"), template))
    .collect()
});

pub fn redact(text: &str) -> String {
    let mut result = text.to_owned();
    for (regex, template) in PATTERNS.iter() {
        result = regex.replace_all(&result, *template).into_owned();
    }
    result
}
