// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Surgical edits to a `settings.json` text (T19-005).
//!
//! Given the old and the new value of a settings tree, [`update_value_in_json_text`]
//! produces the smallest text edits that turn the file into the new tree while
//! leaving comments, trailing commas and untouched formatting alone.
//!
//! The file is read with a small comment- and trailing-comma-tolerant scanner
//! ([`scan`]) that records the byte span of every object member, so each edit
//! is expressed as "replace this span" or "insert before this member". Only
//! object paths are supported: arrays are always replaced as a whole value.

use serde::Serialize;
use serde_json::Value;
use std::ops::Range;

// ───────────────────────────────── scanner ─────────────────────────────────

/// A parsed JSON value: where it sits in the text and what it contains.
struct Node {
    range: Range<usize>,
    shape: Shape,
}

enum Shape {
    Object(Vec<Member>),
    Array(Vec<Node>),
    Scalar,
}

/// One `"key": value` entry of an object.
struct Member {
    /// The decoded key.
    key: String,
    key_start: usize,
    value: Node,
}

impl Member {
    /// Span of the whole entry, from the key's quote to the end of the value.
    fn span(&self) -> Range<usize> {
        self.key_start..self.value.range.end
    }
}

struct Scanner<'a> {
    src: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.at).copied()
    }

    fn rest(&self) -> &'a str {
        &self.src[self.at..]
    }

    /// Step over one (possibly multi-byte) character.
    fn bump_char(&mut self) {
        self.at += self.rest().chars().next().map_or(1, char::len_utf8);
    }

    /// Skip whitespace and `//` / `/* */` comments.
    fn skip_trivia(&mut self) {
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.at += rest.len() - trimmed.len();
            if trimmed.starts_with("//") {
                self.at += trimmed.find('\n').unwrap_or(trimmed.len());
            } else if trimmed.starts_with("/*") {
                self.at += trimmed.find("*/").map_or(trimmed.len(), |end| end + 2);
            } else {
                return;
            }
        }
    }

    /// Consume a string literal starting at the opening quote.
    fn string(&mut self) -> Range<usize> {
        let start = self.at;
        self.at += 1;
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    self.at += 1;
                    break;
                }
                b'\\' => {
                    self.at += 1;
                    if self.peek().is_some() {
                        self.bump_char();
                    }
                }
                _ => self.bump_char(),
            }
        }
        start..self.at
    }

    fn value(&mut self) -> Node {
        self.skip_trivia();
        let start = self.at;
        let shape = match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                self.string();
                Shape::Scalar
            }
            _ => {
                while let Some(byte) = self.peek() {
                    let ends = matches!(byte, b',' | b'}' | b']' | b':')
                        || byte.is_ascii_whitespace()
                        || self.rest().starts_with("//")
                        || self.rest().starts_with("/*");
                    if ends {
                        break;
                    }
                    self.bump_char();
                }
                Shape::Scalar
            }
        };
        Node {
            range: start..self.at,
            shape,
        }
    }

    fn object(&mut self) -> Shape {
        self.at += 1;
        let mut members = Vec::new();
        loop {
            self.skip_trivia();
            match self.peek() {
                None => break,
                Some(b'}') => {
                    self.at += 1;
                    break;
                }
                Some(b',') => self.at += 1,
                Some(b'"') => {
                    let key_range = self.string();
                    self.skip_trivia();
                    if self.peek() == Some(b':') {
                        self.at += 1;
                    }
                    let value = self.value();
                    members.push(Member {
                        key: decode_key(&self.src[key_range.clone()]),
                        key_start: key_range.start,
                        value,
                    });
                }
                // Stray text between members: skip it so scanning always advances.
                Some(_) => self.bump_char(),
            }
        }
        Shape::Object(members)
    }

    fn array(&mut self) -> Shape {
        self.at += 1;
        let mut items = Vec::new();
        loop {
            self.skip_trivia();
            match self.peek() {
                None => break,
                Some(b']') => {
                    self.at += 1;
                    break;
                }
                Some(b',') => self.at += 1,
                Some(b'}') | Some(b':') => self.bump_char(),
                Some(_) => items.push(self.value()),
            }
        }
        Shape::Array(items)
    }
}

fn decode_key(raw: &str) -> String {
    serde_json::from_str(raw).unwrap_or_else(|_| raw.trim_matches('"').to_string())
}

/// Scan the document's top-level value.
fn scan(text: &str) -> Node {
    Scanner { src: text, at: 0 }.value()
}

/// Result of walking a key path through nested objects.
struct Located<'n> {
    /// How many path segments were resolved.
    matched: usize,
    /// The value reached after `matched` segments (the root when `matched == 0`).
    node: &'n Node,
    /// The entry holding `node`, if any segment was resolved.
    member: Option<&'n Member>,
}

fn locate<'n>(root: &'n Node, path: &[impl AsRef<str>]) -> Located<'n> {
    let mut found = Located {
        matched: 0,
        node: root,
        member: None,
    };
    for key in path {
        let Shape::Object(members) = &found.node.shape else {
            break;
        };
        let Some(member) = members.iter().find(|m| m.key == key.as_ref()) else {
            break;
        };
        found = Located {
            matched: found.matched + 1,
            node: &member.value,
            member: Some(member),
        };
    }
    found
}

// ─────────────────────────────── text helpers ──────────────────────────────

/// Column (in bytes) of `offset` within its line.
fn column_of(text: &str, offset: usize) -> usize {
    offset - text[..offset].rfind('\n').map_or(0, |nl| nl + 1)
}

/// Pretty-print `value` with `indent_size` spaces per level; every line after
/// the first is additionally shifted right by `indent_prefix_len` spaces.
pub fn to_pretty_json(
    value: &impl Serialize,
    indent_size: usize,
    indent_prefix_len: usize,
) -> String {
    let indent = " ".repeat(indent_size);
    let mut buffer = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(
        &mut buffer,
        serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes()),
    );
    value
        .serialize(&mut serializer)
        .expect("serde_json values always serialize");
    let pretty = String::from_utf8(buffer).expect("serde_json emits UTF-8");
    pretty.replace('\n', &format!("\n{}", " ".repeat(indent_prefix_len)))
}

/// Nest `value` under `keys`, outermost key first.
fn nest(keys: &[impl AsRef<str>], value: Option<&Value>) -> Value {
    keys.iter()
        .rev()
        .fold(value.cloned().unwrap_or(Value::Null), |inner, key| {
            let mut object = serde_json::Map::new();
            object.insert(key.as_ref().to_string(), inner);
            Value::Object(object)
        })
}

// ─────────────────────────────── editing API ───────────────────────────────

/// Walk `old_value` and `new_value` together and apply the minimal edits to
/// `text`. Unchanged subtrees are skipped, changed leaves are replaced in
/// place, removed keys are deleted and new keys are inserted. Every edit is
/// applied to `text` immediately and also appended to `edits` (each range
/// refers to the text as it was right before that edit).
pub fn update_value_in_json_text<'a>(
    text: &mut String,
    key_path: &mut Vec<&'a str>,
    tab_size: usize,
    old_value: &'a Value,
    new_value: &'a Value,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    let mut apply = |text: &mut String, (range, replacement): (Range<usize>, String)| {
        text.replace_range(range.clone(), &replacement);
        edits.push((range, replacement));
    };

    let (Value::Object(old_map), Value::Object(new_map)) = (old_value, new_value) else {
        if old_value != new_value {
            let mut value = new_value.clone();
            if let Some(map) = value.as_object_mut() {
                map.retain(|_, v| !v.is_null());
            }
            let edit = replace_value_in_json_text(text, key_path, tab_size, Some(&value));
            apply(text, edit);
        }
        return;
    };

    for (key, old_child) in old_map {
        key_path.push(key);
        match new_map.get(key) {
            Some(new_child) => {
                update_value_in_json_text(text, key_path, tab_size, old_child, new_child, edits)
            }
            None => {
                let edit = replace_value_in_json_text(text, key_path, 0, None);
                text.replace_range(edit.0.clone(), &edit.1);
                edits.push(edit);
            }
        }
        key_path.pop();
    }
    for (key, new_child) in new_map.iter().filter(|(k, _)| !old_map.contains_key(*k)) {
        key_path.push(key);
        update_value_in_json_text(text, key_path, tab_size, &Value::Null, new_child, edits);
        key_path.pop();
    }
}

/// The edit that sets the value at `key_path` to `new_value`, or removes that
/// entry when `new_value` is `None`. Returns the byte range to replace and its
/// replacement text. Removing a path that does not exist is a no-op edit.
pub fn replace_value_in_json_text<T: AsRef<str>>(
    text: &str,
    key_path: &[T],
    tab_size: usize,
    new_value: Option<&Value>,
) -> (Range<usize>, String) {
    match new_value {
        Some(value) => set_value(text, key_path, tab_size, value),
        None => remove_entry(text, key_path),
    }
}

fn set_value(
    text: &str,
    key_path: &[impl AsRef<str>],
    tab_size: usize,
    value: &Value,
) -> (Range<usize>, String) {
    let root = scan(text);
    let at = locate(&root, key_path);

    // The whole path exists: overwrite the value.
    if at.matched == key_path.len() {
        let range = if key_path.is_empty() {
            0..text.len()
        } else {
            at.node.range.clone()
        };
        return (
            range,
            to_pretty_json(value, tab_size, tab_size * at.matched),
        );
    }

    let missing_key = key_path[at.matched].as_ref();
    let remaining = &key_path[at.matched + 1..];

    // The deepest existing object has entries: insert in front of the first.
    if let Shape::Object(members) = &at.node.shape {
        if let Some(first) = members.first() {
            let anchor = first.key_start;
            let key = serde_json::to_string(missing_key).expect("string key");
            let entry_value = nest(remaining, Some(value));
            let multiline = text[..anchor].contains('\n');
            let inserted = if multiline {
                let column = column_of(text, anchor);
                let unit = column / (at.matched + 1);
                format!(
                    "{key}: {},\n{}",
                    to_pretty_json(&entry_value, unit, column),
                    " ".repeat(column)
                )
            } else {
                let compact = serde_json::to_string(&entry_value).expect("json value");
                format!("{key}: {compact}, ")
            };
            return (anchor..anchor, inserted);
        }
    }

    // Nothing to anchor on (empty object or a non-object value): rewrite the
    // container as an object holding the new path. The root container always
    // spans the whole text.
    let container = if at.matched == 0 {
        0..text.len()
    } else {
        at.node.range.clone()
    };
    let rebuilt = nest(&key_path[at.matched..], Some(value));
    let mut replacement = to_pretty_json(&rebuilt, tab_size, tab_size * at.matched);
    if at.matched == 0 {
        replacement.push('\n');
    }
    let is_object = matches!(at.node.shape, Shape::Object(_));
    if is_object || at.matched == 0 {
        replacement.insert_str(1, &keep_line_comments(&text[container.clone()]));
    }
    (container, replacement)
}

/// The `//` comments of `old`, so they survive the container being rewritten.
/// A comment on its own line keeps that line (with indentation); a trailing
/// comment is re-attached on the opening line. Only comments terminated by a
/// line break are kept.
fn keep_line_comments(old: &str) -> String {
    let mut kept = String::new();
    for line in old.split_inclusive('\n') {
        let Some(line) = line.strip_suffix('\n') else {
            continue;
        };
        let Some(marker) = line.find("//") else {
            continue;
        };
        if line[..marker].trim().is_empty() {
            kept.push('\n');
            kept.push_str(line);
        } else {
            kept.push_str(&line[marker..]);
        }
    }
    kept
}

fn remove_entry(text: &str, key_path: &[impl AsRef<str>]) -> (Range<usize>, String) {
    let root = scan(text);
    let at = locate(&root, key_path);
    let Some(member) = at.member.filter(|_| at.matched == key_path.len()) else {
        return (0..0, String::new());
    };
    let span = member.span();

    // Prefer taking the comma in front of the entry (only whitespace between).
    let before = text[..span.start].trim_end();
    if before.ends_with(',') {
        return (before.len() - 1..span.end, String::new());
    }
    // Otherwise take the comma behind it plus the whitespace that follows.
    let after = &text[span.end..];
    let lead = after.len() - after.trim_start().len();
    let mut end = span.end;
    if after[lead..].starts_with(',') {
        end += lead + 1;
        let tail = &text[end..];
        end += tail.len() - tail.trim_start().len();
    }
    (span.start..end, String::new())
}

// ────────────────────────────── read-only queries ──────────────────────────

/// Guess the indentation width of a JSON document: the most common offset of
/// the first entry of its top-level containers (ties favour the narrower
/// width). Falls back to 2 when no container spans several lines.
pub fn infer_json_indent_size(text: &str) -> usize {
    const MAX_INDENT: usize = 64;

    let root = scan(text);
    let mut containers = vec![&root];
    if let Shape::Array(items) = &root.shape {
        containers.extend(items);
    }

    let mut tally = [0u32; MAX_INDENT];
    for container in containers {
        let first_child = match &container.shape {
            Shape::Object(members) => members.first().map(|m| m.key_start),
            Shape::Array(items) => items.first().map(|n| n.range.start),
            Shape::Scalar => None,
        };
        let Some(child) = first_child else { continue };
        let on_later_line = text[container.range.start..child].contains('\n');
        let indent = column_of(text, child).saturating_sub(column_of(text, container.range.start));
        if on_later_line && (1..MAX_INDENT).contains(&indent) {
            tally[indent] += 1;
        }
    }

    let mut best = (0, 2);
    for (indent, &count) in tally.iter().enumerate() {
        if count > best.0 {
            best = (count, indent);
        }
    }
    best.1
}

/// Byte range of the value at `key_path` (object keys only), if present.
pub fn find_value_range(text: &str, key_path: &[&str]) -> Option<Range<usize>> {
    let root = scan(text);
    let at = locate(&root, key_path);
    (at.matched == key_path.len()).then(|| at.node.range.clone())
}

/// The key path of the nested object entry that contains `offset`, if any.
/// `None` when `offset` is not inside an entry (punctuation or whitespace
/// between entries, or outside the root object).
pub fn json_path_at_offset(text: &str, offset: usize) -> Option<Vec<String>> {
    let root = scan(text);
    let mut path = Vec::new();
    let mut node = &root;
    while node.range.contains(&offset) {
        let Shape::Object(members) = &node.shape else {
            break;
        };
        let Some(entry) = members.iter().find(|m| m.span().contains(&offset)) else {
            break;
        };
        path.push(entry.key.clone());
        node = &entry.value;
    }
    (!path.is_empty()).then_some(path)
}

/// 1-based `(line, column)` of a byte offset; the column counts UTF-8 bytes.
pub fn line_col_at(text: &str, byte_offset: usize) -> (usize, usize) {
    let head = text.get(..byte_offset.min(text.len())).unwrap_or(text);
    let line = head.matches('\n').count() + 1;
    let column = head.len() - head.rfind('\n').map_or(0, |nl| nl + 1) + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use unindent::Unindent;

    #[test]
    fn object_replace() {
        #[track_caller]
        fn check_object_replace(
            input: String,
            key_path: &[&str],
            value: Option<Value>,
            expected: String,
        ) {
            let result = replace_value_in_json_text(&input, key_path, 4, value.as_ref());
            let mut result_str = input;
            result_str.replace_range(result.0, &result.1);
            pretty_assertions::assert_eq!(expected, result_str);
        }
        check_object_replace(
            r#"{
                "a": 1,
                "b": 2
            }"#
            .unindent(),
            &["b"],
            Some(json!(3)),
            r#"{
                "a": 1,
                "b": 3
            }"#
            .unindent(),
        );
        check_object_replace(
            r#"{
                "a": 1,
                "b": 2
            }"#
            .unindent(),
            &["c"],
            Some(json!(3)),
            r#"{
                "c": 3,
                "a": 1,
                "b": 2
            }"#
            .unindent(),
        );
        check_object_replace(
            r#"{
                "name": "old_name",
                "id": 123
            }"#
            .unindent(),
            &["name"],
            Some(json!("new_name")),
            r#"{
                "name": "new_name",
                "id": 123
            }"#
            .unindent(),
        );
        check_object_replace(
            r#"{
                // This is a comment
                "a": 1,
                "b": 2 // Another comment
            }"#
            .unindent(),
            &["b"],
            Some(json!({"foo": "bar"})),
            r#"{
                // This is a comment
                "a": 1,
                "b": {
                    "foo": "bar"
                } // Another comment
            }"#
            .unindent(),
        );
        check_object_replace(
            r#"{}"#.to_string(),
            &["new_key"],
            Some(json!("value")),
            r#"{
                "new_key": "value"
            }
            "#
            .unindent(),
        );
        check_object_replace(
            r#"{
                "level1": {
                    "level2": {
                        "level3": {
                            "target": "old"
                        }
                    }
                }
            }"#
            .unindent(),
            &["level1", "level2", "level3", "target"],
            Some(json!("new")),
            r#"{
                "level1": {
                    "level2": {
                        "level3": {
                            "target": "new"
                        }
                    }
                }
            }"#
            .unindent(),
        );
        check_object_replace(
            r#"{
                "parent": {}
            }"#
            .unindent(),
            &["parent", "child"],
            Some(json!("value")),
            r#"{
                "parent": {
                    "child": "value"
                }
            }"#
            .unindent(),
        );
    }

    #[test]
    fn test_infer_json_indent_size() {
        let json_2_spaces = r#"{
  "key1": "value1",
  "nested": {
    "key2": "value2"
  }
}"#;
        assert_eq!(infer_json_indent_size(json_2_spaces), 2);

        let json_4_spaces = r#"{
    "key1": "value1",
    "nested": {
        "key2": "value2"
    }
}"#;
        assert_eq!(infer_json_indent_size(json_4_spaces), 4);

        let json_empty = r#"{}"#;
        assert_eq!(infer_json_indent_size(json_empty), 2);
    }

    #[test]
    fn update_value_in_json_text_updates_only_changed_leaves() {
        let mut text = r#"{
            // top-level comment
            "terminal": {
                "terminalFontSize": 14
            },
            "preferences": {
                "theme": "dark"
            }
        }"#
        .unindent();
        let old = json!({"terminal": {"terminalFontSize": 14}});
        let new = json!({"terminal": {"terminalFontSize": 20}});
        let mut edits = Vec::new();
        update_value_in_json_text(&mut text, &mut Vec::new(), 4, &old, &new, &mut edits);
        assert_eq!(edits.len(), 1);
        assert!(text.contains("\"terminalFontSize\": 20"));
        assert!(text.contains("\"theme\": \"dark\""));
        assert!(text.contains("// top-level comment"));
    }

    #[test]
    fn update_value_in_json_text_inserts_missing_key() {
        let mut text = "{}".to_string();
        let old = json!({});
        let new = json!({"terminal": {"terminalFontSize": 20}});
        let mut edits = Vec::new();
        update_value_in_json_text(&mut text, &mut Vec::new(), 4, &old, &new, &mut edits);
        assert!(!edits.is_empty());
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["terminal"]["terminalFontSize"], 20);
    }

    #[test]
    fn find_value_range_locates_nested_key() {
        let text = "{\n  \"terminal\": {\n    \"terminalFontSize\": \"gross\"\n  }\n}";
        let range = find_value_range(text, &["terminal", "terminalFontSize"]).unwrap();
        assert_eq!(&text[range], "\"gross\"");
    }

    #[test]
    fn find_value_range_missing_key_is_none() {
        let text = "{\"terminal\": {}}";
        assert!(find_value_range(text, &["terminal", "terminalFontSize"]).is_none());
    }

    #[test]
    fn json_path_at_offset_finds_nested_key() {
        let text = "{\n  \"terminal\": {\n    \"terminalFontSize\": 14\n  }\n}";
        let offset = text.find("14").unwrap();
        let path = json_path_at_offset(text, offset).unwrap();
        assert_eq!(path, vec!["terminal", "terminalFontSize"]);
    }

    #[test]
    fn json_path_at_offset_outside_any_pair_is_none() {
        let text = "{\"a\": 1}";
        assert!(json_path_at_offset(text, 0).is_none());
    }

    #[test]
    fn line_col_at_counts_lines() {
        let text = "{\n  \"a\": 1,\n  \"b\": 2\n}";
        let offset = text.find("\"b\"").unwrap();
        assert_eq!(line_col_at(text, offset), (3, 3));
    }

    fn edited(text: &str, path: &[&str], value: Option<Value>) -> String {
        let (range, replacement) = replace_value_in_json_text(text, path, 2, value.as_ref());
        let mut out = text.to_string();
        out.replace_range(range, &replacement);
        out
    }

    #[test]
    fn removes_entries_with_their_commas() {
        let text = "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}";
        for (key, expected) in [
            ("a", "{\n  \"b\": 2,\n  \"c\": 3\n}"),
            ("b", "{\n  \"a\": 1,\n  \"c\": 3\n}"),
            ("c", "{\n  \"a\": 1,\n  \"b\": 2\n}"),
        ] {
            assert_eq!(edited(text, &[key], None), expected, "removing {key}");
        }
        assert_eq!(edited(text, &["zzz"], None), text);
    }

    #[test]
    fn tolerates_comments_and_trailing_commas() {
        let text =
            "{\n  // keep\n  \"a\": \"//not a comment\", /* x */\n  \"b\": [1, {\"c\": 2}],\n}";
        let out = edited(text, &["a"], Some(json!("v")));
        assert_eq!(out, text.replace("\"//not a comment\"", "\"v\""));
        assert_eq!(
            find_value_range(text, &["b"]).map(|r| &text[r]),
            Some("[1, {\"c\": 2}]")
        );
    }

    #[test]
    fn empty_object_rewrite_keeps_line_comments() {
        let out = edited("{\n  // note\n}", &["a"], Some(json!(1)));
        assert!(out.contains("// note") && out.contains("\"a\": 1"), "{out}");
        serde_json::from_str::<Value>(&out.replace("// note", "")).unwrap();
    }

    #[test]
    fn overwrites_non_object_with_nested_path() {
        let out = edited("{\"a\": 1}", &["a", "b"], Some(json!(2)));
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed, json!({"a": {"b": 2}}));
    }

    #[test]
    fn scanner_survives_malformed_input() {
        for text in ["", "{", "{\"a\"", "{\"a\": ", "[1, 2", "}{", "\"é"] {
            let _ = infer_json_indent_size(text);
            let _ = find_value_range(text, &["a"]);
            let _ = json_path_at_offset(text, 1);
            let _ = edited(text, &["a"], Some(json!(1)));
        }
    }
}
