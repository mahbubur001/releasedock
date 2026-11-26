//! JSON text exactly as Swift's `JSONEncoder` writes it with `.sortedKeys` (the store's `data`
//! and `value` columns), `[.prettyPrinted, .sortedKeys]` (the diagnostics bundle) and
//! `[.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]` (release manifests):
//!
//! - keys sorted by their UTF-8 bytes, which is what the records in the Swift app's database
//!   show (`"Requires PHP"` before `"Requires at least"`);
//! - `/` escaped as `\/` unless `.withoutEscapingSlashes` is set;
//! - a whole `Double` written without a fraction (`3`, not `3.0`);
//! - pretty printing with two-space indentation, `"key" : value`, and empty arrays and
//!   objects as `[\n\n<indent>]` / `{\n\n<indent>}`, as Foundation does.

use serde_json::{Number, Value};
use std::fmt::Write;

/// Compact JSON (`JSONEncoder` with `.sortedKeys`).
pub fn to_compact(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, true);
    out
}

/// Pretty-printed JSON (`JSONEncoder` with `[.prettyPrinted, .sortedKeys]`), no trailing newline.
pub fn to_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(0), true);
    out
}

/// Pretty-printed JSON with `/` left as is (`[.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]`).
pub fn to_pretty_unescaped_slashes(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(0), false);
    out
}

/// `depth` is None for compact output.
fn write_value(out: &mut String, value: &Value, depth: Option<usize>, escape_slashes: bool) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s, escape_slashes),
        Value::Array(items) => {
            let items: Vec<&Value> = items.iter().collect();
            write_container(out, ('[', ']'), &items, depth, |out, item, depth| {
                write_value(out, item, depth, escape_slashes);
            });
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            write_container(out, ('{', '}'), &entries, depth, |out, (key, item), depth| {
                write_string(out, key, escape_slashes);
                out.push_str(if depth.is_some() { " : " } else { ":" });
                write_value(out, item, depth, escape_slashes);
            });
        }
    }
}

fn write_container<T: Copy>(
    out: &mut String,
    (open, close): (char, char),
    items: &[T],
    depth: Option<usize>,
    write_item: impl Fn(&mut String, T, Option<usize>),
) {
    out.push(open);
    match depth {
        None => {
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_item(out, *item, None);
            }
        }
        Some(depth) => {
            out.push('\n');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                write_item(out, *item, Some(depth + 1));
            }
            out.push('\n');
            indent(out, depth);
        }
    }
    out.push(close);
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_number(out: &mut String, n: &Number) {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => {
            let _ = write!(out, "{}", f as i64);
        }
        _ => {
            let _ = write!(out, "{n}");
        }
    }
}

/// serde_json's escaping (`"`, `\`, control characters), plus `\/` unless disabled.
fn write_string(out: &mut String, s: &str, escape_slashes: bool) {
    let escaped = Value::String(s.to_owned()).to_string();
    out.push_str(&if escape_slashes {
        escaped.replace('/', "\\/")
    } else {
        escaped
    });
}

#[cfg(test)]
mod tests {
    use super::{to_compact, to_pretty};
    use serde_json::json;

    #[test]
    fn matches_foundation_compact_sorted_output() {
        let value = json!({"b": [1, 2.0, 0.5, "a/b"], "Requires at least": 1, "Requires PHP": "7.4", "a": {}});
        assert_eq!(
            to_compact(&value),
            r#"{"Requires PHP":"7.4","Requires at least":1,"a":{},"b":[1,2,0.5,"a\/b"]}"#
        );
    }

    #[test]
    fn matches_foundation_pretty_printing() {
        let value = json!({"b": [1, "a/b"], "a": [], "c": {"y": null, "x": true}});
        assert_eq!(
            to_pretty(&value),
            "{\n  \"a\" : [\n\n  ],\n  \"b\" : [\n    1,\n    \"a\\/b\"\n  ],\n  \"c\" : {\n    \"x\" : true,\n    \"y\" : null\n  }\n}"
        );
    }
}
