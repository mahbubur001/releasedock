//! Foundation `CharacterSet` equivalents used when parsing ignore files.

/// `CharacterSet.whitespaces`: Unicode general category Zs plus tab.
pub(crate) fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// `CharacterSet.newlines`: U+000A–U+000D, U+0085, U+2028 and U+2029.
pub(crate) fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// `trimmingCharacters(in: .whitespaces)`.
pub(crate) fn trim_whitespace(text: &str) -> &str {
    text.trim_matches(is_whitespace)
}

/// `components(separatedBy: .newlines)`: every newline character separates, so `\r\n`
/// yields an empty component in between.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(is_newline)
}

/// Lexical normalization like `URL.standardizedFileURL`: makes the path absolute against
/// the current directory, removes `.` and resolves `..`.
pub(crate) fn standardize(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut result = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}
