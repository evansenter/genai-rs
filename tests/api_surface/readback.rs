//! Reading what a deserialized value kept, from its `Debug` output, and the
//! JSON-path helpers that trace a finding to the key behind it.
//!
//! Every response struct keeps unmodeled keys in a flattened `extra` map (as
//! does every known variant of the step and content unions, #491), and every
//! growable enum has an `Unknown` variant. Both show in `{:?}`, which is
//! the one view of a value that tells modeled from unmodeled: serializing it
//! again puts `extra` keys back beside the modeled ones.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The keys of every non-empty `extra` map in a `{:?}` rendering.
///
/// Matches any field whose name ends in `extra`, so `arguments_extra` (the
/// unmodeled `arguments` keys of a call step or delta) counts too. Matches
/// `extra: {"key": ..}` (a derived `Debug` of a JSON map) and
/// `extra: ["key", ..]` (the redacting impls, which list keys only).
pub fn debug_extra_keys(debug: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for (at, _) in debug.match_indices("extra: ") {
        let rest = &debug[at + "extra: ".len()..];
        let (open, close) = match rest.as_bytes().first() {
            Some(b'{') => (b'{', b'}'),
            Some(b'[') => (b'[', b']'),
            _ => continue,
        };
        let body = &rest[1..];
        let mut depth = 0usize;
        let mut chars = body.char_indices().peekable();
        let mut expect_key = true;
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    let start = i + 1;
                    let mut end = start;
                    let mut escaped = false;
                    for (j, d) in chars.by_ref() {
                        if escaped {
                            escaped = false;
                        } else if d == '\\' {
                            escaped = true;
                        } else if d == '"' {
                            end = j;
                            break;
                        }
                    }
                    if depth == 0 && expect_key {
                        keys.push(body[start..end].replace("\\\"", "\""));
                        expect_key = false;
                    }
                }
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' if depth > 0 => depth -= 1,
                c if depth == 0 && c as u32 == u32::from(close) => break,
                ',' if depth == 0 => expect_key = true,
                _ => {}
            }
        }
        let _ = open;
    }
    keys
}

/// The wire value of every `Unknown { <field>: "value", .. }` variant in a
/// `{:?}` rendering.
pub fn debug_unknown_values(debug: &str) -> Vec<String> {
    let mut values = Vec::new();
    for (at, _) in debug.match_indices("Unknown { ") {
        let rest = &debug[at + "Unknown { ".len()..];
        if let Some(colon) = rest.find(": \"") {
            let value_start = colon + 3;
            if let Some(len) = rest[value_start..].find('"') {
                values.push(rest[value_start..value_start + len].to_string());
            }
        }
    }
    values
}

/// Every object-key path (`$.steps`) and list-element path (`$.steps[0]`)
/// in `value`. The element paths matter: a key on an object that sits
/// directly in a list (`$.steps[0].content[0].annotations[0].title`) has an
/// element as its parent, and a key counts as dropped only under a kept
/// parent.
pub fn key_paths(value: &Value) -> BTreeSet<String> {
    fn walk(v: &Value, path: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    let p = format!("{path}.{k}");
                    out.insert(p.clone());
                    walk(child, &p, out);
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    let p = format!("{path}[{i}]");
                    out.insert(p.clone());
                    walk(child, &p, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(value, "$", &mut out);
    out
}

/// Removes the value at a `key_paths` path (an object key or a list
/// element); false if it is not there.
pub fn remove_path(value: &mut Value, path: &str) -> bool {
    let Some(rest) = path.strip_prefix('$') else {
        return false;
    };
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                let mut index = String::from("[");
                for d in chars.by_ref() {
                    index.push(d);
                    if d == ']' {
                        break;
                    }
                }
                segments.push(index);
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    let Some((last, parents)) = segments.split_last() else {
        return false;
    };
    let mut node = value;
    for segment in parents {
        node = match segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Some(index) => match index.parse::<usize>().ok().and_then(|i| node.get_mut(i)) {
                Some(n) => n,
                None => return false,
            },
            None => match node.get_mut(segment.as_str()) {
                Some(n) => n,
                None => return false,
            },
        };
    }
    match last.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(index) => match (index.parse::<usize>(), node.as_array_mut()) {
            (Ok(i), Some(items)) if i < items.len() => {
                items.remove(i);
                true
            }
            _ => false,
        },
        None => node
            .as_object_mut()
            .is_some_and(|map| map.remove(last.as_str()).is_some()),
    }
}

/// The string value at each path, for finding where an `Unknown` came from.
pub fn string_values(value: &Value) -> BTreeMap<String, String> {
    fn walk(v: &Value, path: &str, out: &mut BTreeMap<String, String>) {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    walk(child, &format!("{path}.{k}"), out);
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    walk(child, &format!("{path}[{i}]"), out);
                }
            }
            Value::String(s) => {
                out.insert(path.to_string(), s.clone());
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    walk(value, "$", &mut out);
    out
}
