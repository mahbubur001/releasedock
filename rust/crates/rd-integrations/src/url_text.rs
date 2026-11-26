//! The few `Foundation.URL` operations the services use, on URL strings.

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use url::Url;

/// `CharacterSet.urlPathAllowed`: everything else is percent-encoded.
const PATH_ALLOWED: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// `string.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed)`.
pub(crate) fn encode_path(text: &str) -> String {
    utf8_percent_encode(text, PATH_ALLOWED).to_string()
}

/// Percent-encodes every byte that is not in `allowed` (ASCII only).
pub(crate) fn encode_allowing(text: &str, allowed: impl Fn(u8) -> bool) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if allowed(byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// `URL.lastPathComponent`: the last non-empty path segment, percent-decoded; empty when
/// the path has none.
pub(crate) fn last_path_component(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return String::new();
    };
    parsed
        .path()
        .split('/')
        .rfind(|segment| !segment.is_empty())
        .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
        .unwrap_or_default()
}

/// `URL.deletingLastPathComponent().absoluteString`: the URL up to and including the `/`
/// before its last path segment (`https://h/a/b` → `https://h/a/`).
pub(crate) fn deleting_last_path_component(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    let path = parsed.path().trim_end_matches('/').to_owned();
    let parent = match path.rfind('/') {
        Some(index) => path[..=index].to_owned(),
        None => "/".to_owned(),
    };
    parsed.set_path(&parent);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

/// `URL.host`, lowercased.
pub(crate) fn host(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(str::to_lowercase)
}

/// `String.trimmingTrailingSlash`: drops one trailing `/`.
pub(crate) fn trimming_trailing_slash(text: &str) -> &str {
    text.strip_suffix('/').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_components() {
        assert_eq!(last_path_component("https://h/a/b/"), "b");
        assert_eq!(last_path_component("file:///tmp/repo/sample%20plugin"), "sample plugin");
        assert_eq!(last_path_component("https://h"), "");
        assert_eq!(deleting_last_path_component("https://h/a/b"), "https://h/a/");
        assert_eq!(deleting_last_path_component("https://h/a/b/"), "https://h/a/");
        assert_eq!(
            deleting_last_path_component("file:///tmp/r/x/trunk"),
            "file:///tmp/r/x/"
        );
        assert_eq!(
            host("https://Plugins.SVN.wordpress.org/x").as_deref(),
            Some("plugins.svn.wordpress.org")
        );
        assert_eq!(host("file:///tmp/x"), None);
        assert_eq!(encode_path("v1.0 beta?#/x"), "v1.0%20beta%3F%23/x");
    }
}
