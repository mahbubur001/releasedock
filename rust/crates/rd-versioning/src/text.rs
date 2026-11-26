//! Foundation and ICU text semantics the Swift code relies on, so trimming, splitting and
//! regex anchoring behave the same in Rust.

/// `CharacterSet.newlines`: U+000A–U+000D, U+0085, U+2028, U+2029. These are also exactly
/// the characters ICU (`NSRegularExpression`) treats as line terminators for `.`, `^` and `$`.
pub(crate) fn is_newline(c: char) -> bool {
    matches!(c, '\n'..='\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `CharacterSet.whitespaces`: Unicode general category Zs plus tab.
pub(crate) fn is_whitespace(c: char) -> bool {
    c == '\t' || (c.is_whitespace() && !is_newline(c))
}

/// `trimmingCharacters(in: .whitespaces)`.
pub(crate) fn trim_whitespace(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`. Rust's `White_Space` property is the
/// same set (Z* plus U+0009–U+000D and U+0085).
pub(crate) fn trim_whitespace_and_newlines(s: &str) -> &str {
    s.trim()
}

/// `trimmingCharacters(in: .newlines)`.
pub(crate) fn trim_newlines(s: &str) -> &str {
    s.trim_matches(is_newline)
}

/// `trimmingCharacters(in: CharacterSet(charactersIn: set))`.
pub(crate) fn trim_chars<'a>(s: &'a str, set: &str) -> &'a str {
    s.trim_matches(|c| set.contains(c))
}

/// The lines an ICU multi-line regex sees: split at every line terminator, with `\r\n` as one
/// terminator, keeping the empty line after a final terminator (ICU's `^` matches there).
pub(crate) fn icu_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_newline(c) {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|&(_, next)| next == '\n') {
            chars.next();
            end += 1;
        }
        start = end;
    }
    lines.push(&text[start..]);
    lines
}

/// `String.split(separator: "\n")` on Swift Characters: empty pieces are dropped, and a
/// `\r\n` pair is a single Character, so it does not split there.
pub(crate) fn split_lf_characters(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut previous = '\0';
    for (index, c) in text.char_indices() {
        if c == '\n' && previous != '\r' {
            pieces.push(&text[start..index]);
            start = index + 1;
        }
        previous = c;
    }
    pieces.push(&text[start..]);
    pieces.retain(|p| !p.is_empty());
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icu_lines_treat_crlf_as_one_terminator() {
        assert_eq!(icu_lines("a\r\nb\u{2028}c\n"), ["a", "b", "c", ""]);
        assert_eq!(icu_lines("a\r\rb"), ["a", "", "b"]);
    }

    #[test]
    fn swift_split_keeps_crlf_together() {
        assert_eq!(split_lf_characters("a\r\nb\nc\n\n"), ["a\r\nb", "c"]);
    }

    #[test]
    fn whitespace_sets_match_foundation() {
        assert_eq!(trim_whitespace("\u{a0}\tx\u{85}"), "x\u{85}");
        assert_eq!(trim_whitespace_and_newlines("\u{85}\u{2028} x\u{3000}"), "x");
        assert_eq!(trim_whitespace("\u{180e}x"), "\u{180e}x");
    }
}
