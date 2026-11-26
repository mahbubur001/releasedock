//! Foundation string semantics the Swift validation code relies on, so counts, casing and
//! formatting match what the Swift app shows.

use unicode_segmentation::UnicodeSegmentation;

/// Swift `String.count`: the number of grapheme clusters (Characters).
pub(crate) fn character_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Swift `String.prefix(n)`: the first `n` Characters.
pub(crate) fn prefix(s: &str, n: usize) -> &str {
    match s.grapheme_indices(true).nth(n) {
        Some((index, _)) => &s[..index],
        None => s,
    }
}

/// Swift `String.lowercased()`: per-scalar lowercase mapping, without the context-sensitive
/// final-sigma rule Rust's `str::to_lowercase` applies.
pub(crate) fn lowercased(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// `CharacterSet.whitespaces`: Unicode general category Zs plus tab (no newlines).
pub(crate) fn is_whitespace(c: char) -> bool {
    c == '\t' || (c.is_whitespace() && !matches!(c, '\n'..='\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

/// `trimmingCharacters(in: .whitespaces)`.
pub(crate) fn trim_whitespace(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// `NSString.capitalized`: the first cased letter of every word is uppercased and the rest
/// lowercased. A word starts after any character that is not a cased letter; an apostrophe
/// after a letter does not start a new word (Foundation: `don't` → `Don't`, `1st` → `1St`,
/// `foo-bar` → `Foo-Bar`, `license uri` → `License Uri`).
pub(crate) fn capitalized(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut previous_cased = false;
    for c in s.chars() {
        if c.is_lowercase() || c.is_uppercase() {
            if previous_cased {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            previous_cased = true;
        } else {
            if !(previous_cased && matches!(c, '\'' | '\u{2019}')) {
                previous_cased = false;
            }
            out.push(c);
        }
    }
    out
}

/// `NSString.pathExtension`: the part of the last path component after its last `.`; empty
/// when there is none, when the name starts with its only `.` (`.htaccess`) or ends with `.`.
pub(crate) fn path_extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index > 0 => &name[index + 1..],
        _ => "",
    }
}

pub(crate) use rd_support::byte_count::byte_count_file_style;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalized_matches_foundation() {
        // Values printed by Foundation's `String.capitalized`.
        let cases = [
            ("frequently asked questions", "Frequently Asked Questions"),
            ("license uri", "License Uri"),
            ("foo-bar baz", "Foo-Bar Baz"),
            ("don't stop", "Don't Stop"),
            ("1st thing", "1St Thing"),
            ("a.b c_d", "A.B C_D"),
            ("élan", "Élan"),
            ("requires php", "Requires Php"),
            ("CHANGELOG", "Changelog"),
        ];
        for (input, expected) in cases {
            assert_eq!(capitalized(input), expected, "{input}");
        }
    }

    #[test]
    fn path_extension_matches_foundation() {
        let cases = [
            (".htaccess", ""),
            ("a/.png", ""),
            ("file.tar.gz", "gz"),
            ("noext", ""),
            ("dir.d/file", ""),
            ("x.", ""),
            ("a/b.PNG", "PNG"),
        ];
        for (input, expected) in cases {
            assert_eq!(path_extension(input), expected, "{input}");
        }
    }

    #[test]
    fn byte_counts_match_foundation() {
        let cases = [
            (0, "Zero KB"),
            (1, "1 byte"),
            (999, "999 bytes"),
            (1000, "1 KB"),
            (1500, "2 KB"),
            (123_456, "123 KB"),
            (1_234_567, "1.2 MB"),
            (10_000_000, "10 MB"),
            (1_234_567_890, "1.23 GB"),
        ];
        for (bytes, expected) in cases {
            assert_eq!(byte_count_file_style(bytes), expected, "{bytes}");
        }
    }

    #[test]
    fn characters_are_grapheme_clusters() {
        assert_eq!(character_count("e\u{301}"), 1);
        assert_eq!(character_count("👍🏽x"), 2);
        assert_eq!(prefix("ab\u{301}cd", 2), "ab\u{301}");
        assert_eq!(prefix("ab", 5), "ab");
        assert_eq!(lowercased("ΣΑΣ"), "σασ");
        assert_eq!(trim_whitespace(" \t7.1\u{a0}"), "7.1");
    }
}
