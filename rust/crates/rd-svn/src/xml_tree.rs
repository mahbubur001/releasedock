//! Port of `Support/XMLTree.swift`: a minimal owned XML DOM for parsing `svn --xml` output.

use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
#[error("could not parse XML: {0}")]
pub struct XmlError(String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmlElement {
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub children: Vec<XmlElement>,
    /// Character data directly inside this element (not its children), concatenated.
    pub text: String,
}

impl XmlElement {
    pub fn child(&self, name: &str) -> Option<&XmlElement> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn all(&self, name: &str) -> Vec<&XmlElement> {
        self.children.iter().filter(|c| c.name == name).collect()
    }

    /// Depth-first search for descendants with `name`.
    pub fn descendants(&self, name: &str) -> Vec<&XmlElement> {
        let mut result = Vec::new();
        self.collect_descendants(name, &mut result);
        result
    }

    fn collect_descendants<'a>(&'a self, name: &str, result: &mut Vec<&'a XmlElement>) {
        for child in &self.children {
            if child.name == name {
                result.push(child);
            }
            child.collect_descendants(name, result);
        }
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    pub fn trimmed_text(&self) -> &str {
        self.text.trim()
    }

    /// Parses UTF-8 XML. External entities are never resolved.
    pub fn parse(data: &[u8]) -> Result<XmlElement, XmlError> {
        let text = std::str::from_utf8(data).map_err(|e| XmlError(e.to_string()))?;
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let document = roxmltree::Document::parse_with_options(text, options).map_err(|e| XmlError(e.to_string()))?;
        Ok(Self::build(document.root_element()))
    }

    fn build(node: roxmltree::Node<'_, '_>) -> XmlElement {
        let mut element = XmlElement {
            name: node.tag_name().name().to_owned(),
            attributes: node
                .attributes()
                .map(|a| (a.name().to_owned(), a.value().to_owned()))
                .collect(),
            children: Vec::new(),
            text: String::new(),
        };
        for child in node.children() {
            if child.is_element() {
                element.children.push(Self::build(child));
            } else if child.is_text() {
                element.text.push_str(child.text().unwrap_or_default());
            }
        }
        element
    }
}
