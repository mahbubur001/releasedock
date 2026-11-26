//! Release notes of one app update, read from the HTML in its update-feed description.
//! The publish script writes `<h3>Added</h3><ul><li>…</li></ul>` per Keep a Changelog
//! group; older entries are a single `<p>`, which becomes one untitled section.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Heading as written (Added, Changed, Fixed…); None for text outside any heading.
    pub title: Option<String>,
    /// Items as Markdown: `**bold**`, `` `code` `` and links survive from the HTML.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseNotes {
    pub sections: Vec<Section>,
}

static BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<(h[1-6]|li|p)\b[^>]*>(.*?)</(h[1-6]|li|p)\s*>").expect("valid"));

impl ReleaseNotes {
    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(|s| s.items.is_empty())
    }

    pub fn from_html(html: &str) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        for captures in BLOCK.captures_iter(html) {
            let (open, close) = (captures[1].to_lowercase(), captures[3].to_lowercase());
            if open != close {
                continue;
            }
            let content = markdown_from_inline_html(&captures[2]);
            if content.is_empty() {
                continue;
            }
            if open.starts_with('h') {
                sections.push(Section {
                    title: Some(content),
                    items: Vec::new(),
                });
            } else if let Some(last) = sections.last_mut() {
                last.items.push(content);
            } else {
                sections.push(Section {
                    title: None,
                    items: vec![content],
                });
            }
        }
        if sections.is_empty() {
            let plain = markdown_from_inline_html(html);
            if !plain.is_empty() {
                sections.push(Section {
                    title: None,
                    items: vec![plain],
                });
            }
        }
        sections.retain(|s| !s.items.is_empty());
        Self { sections }
    }
}

static INLINE: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?is)<b\b[^>]*>(.*?)</b\s*>", "**$1**"),
        (r"(?is)<strong\b[^>]*>(.*?)</strong\s*>", "**$1**"),
        (r"(?is)<code\b[^>]*>(.*?)</code\s*>", "`$1`"),
        (r#"(?is)<a\b[^>]*href="([^"]*)"[^>]*>(.*?)</a\s*>"#, "[$2]($1)"),
        (r"(?i)<br\s*/?>", " "),
        (r"<[^>]+>", ""),
    ]
    .into_iter()
    .map(|(p, t)| (Regex::new(p).expect("valid"), t))
    .collect()
});

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid"));

/// Inline HTML to Markdown: keeps bold, code and links, drops other tags, decodes entities.
pub fn markdown_from_inline_html(html: &str) -> String {
    let mut text = html.to_owned();
    for (regex, template) in INLINE.iter() {
        text = regex.replace_all(&text, *template).into_owned();
    }
    for (entity, character) in [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&#x27;", "'"),
        ("&nbsp;", " "),
        ("&amp;", "&"),
    ] {
        text = text.replace(entity, character);
    }
    WHITESPACE.replace_all(&text, " ").trim().to_owned()
}
