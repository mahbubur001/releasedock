//! JSON text in the layout of Swift's `JSONEncoder` with
//! `[.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]`:
//!
//! - two-space indentation and `"key" : value` (a space on both sides of the colon);
//! - keys sorted case-insensitively (Foundation compares them with `.caseInsensitive`),
//!   ties broken by the exact key;
//! - empty arrays and objects written as `[\n\n<indent>]` / `{\n\n<indent>}`, as Foundation does;
//! - `/` is not escaped; no trailing newline.

use serde_json::Value;
use std::fmt::Write;

pub fn to_pretty_sorted(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let _ = write!(out, "{n}");
        }
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
            out.push_str("{\n");
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                write_string(out, key);
                out.push_str(" : ");
                write_value(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// serde_json's escaping (`"`, `\`, control characters) already leaves `/` unescaped.
fn write_string(out: &mut String, s: &str) {
    out.push_str(&Value::String(s.to_owned()).to_string());
}

#[cfg(test)]
mod tests {
    use super::to_pretty_sorted;
    use serde_json::json;

    #[test]
    fn matches_foundation_pretty_printing() {
        let value = json!({"b": [1, "a/b"], "A": {}, "a": [], "c": {"y": null, "x": true}});
        assert_eq!(
            to_pretty_sorted(&value),
            "{\n  \"A\" : {\n\n  },\n  \"a\" : [\n\n  ],\n  \"b\" : [\n    1,\n    \"a/b\"\n  ],\n  \"c\" : {\n    \"x\" : true,\n    \"y\" : null\n  }\n}"
        );
    }
}
