//! Preference reads and edits of JSON with comments and trailing commas, as OpenCode reads
//! `opencode.jsonc`.
//!
//! Edits change only the text of the entry they touch, so comments, spacing, and every other
//! entry stay as the user wrote them.

use serde_json::{Map, Value};

use super::PreferenceDocument;

/// Deepest nesting a document may have.
const MAX_DEPTH: usize = 64;
/// Indentation added for each level of entries a new nested object brings.
const INDENT_STEP: &str = "  ";

/// The text of a JSONC document whose top level is an object.
pub(super) struct JsoncDocument {
    text: String,
}

/// A parsed value and the byte span of its text.
struct Node {
    start: usize,
    end: usize,
    kind: NodeKind,
}

enum NodeKind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    Scalar(Value),
}

/// One entry of an object.
struct Member {
    key: String,
    key_start: usize,
    value: Node,
    /// Position of the comma that follows the entry, if any.
    comma: Option<usize>,
}

impl PreferenceDocument for JsoncDocument {
    fn parse(contents: &str) -> Option<Self> {
        let text = if contents.trim().is_empty() {
            "{}\n".to_string()
        } else {
            contents.to_string()
        };
        parse_root(&text)?;
        Some(Self { text })
    }

    fn value_at(&self, path: &[&str]) -> Option<Value> {
        let root = parse_root(&self.text)?;
        node_at(&root, path).map(json_value)
    }

    fn insert_at(&mut self, path: &[&str], value: Value) -> bool {
        let Some(root) = parse_root(&self.text) else {
            return false;
        };
        let Some(edits) = insertion(&self.text, &root, path, &value) else {
            return false;
        };
        apply_edits(&mut self.text, edits);
        true
    }

    fn remove_at(&mut self, path: &[&str]) {
        if !self.remove_member(path) {
            return;
        }
        for depth in (1..path.len()).rev() {
            let parent = &path[..depth];
            let empty = parse_root(&self.text)
                .as_ref()
                .and_then(|root| node_at(root, parent))
                .is_some_and(
                    |node| matches!(&node.kind, NodeKind::Object(members) if members.is_empty()),
                );
            if !empty || !self.remove_member(parent) {
                return;
            }
        }
    }

    fn render(self) -> Option<String> {
        parse_root(&self.text)?;
        Some(self.text)
    }
}

impl JsoncDocument {
    /// Removes the entry at `path`, its comma, and the rest of its line when nothing else is on
    /// it. Returns whether an entry was removed.
    fn remove_member(&mut self, path: &[&str]) -> bool {
        let Some((leaf, parents)) = path.split_last() else {
            return false;
        };
        let Some(root) = parse_root(&self.text) else {
            return false;
        };
        let Some(NodeKind::Object(members)) = node_at(&root, parents).map(|node| &node.kind) else {
            return false;
        };
        let Some(index) = members.iter().rposition(|member| member.key == *leaf) else {
            return false;
        };
        let text = &self.text;
        let member = &members[index];
        let line_start = text[..member.key_start].rfind('\n').map_or(0, |at| at + 1);
        let alone_on_line = text[line_start..member.key_start].trim().is_empty();
        let mut end = line_end(
            text,
            member.comma.map_or(member.value.end, |comma| comma + 1),
        );
        let mut start = member.key_start;
        if alone_on_line {
            let newline = if text[end..].starts_with("\r\n") {
                2
            } else {
                usize::from(text[end..].starts_with('\n'))
            };
            if newline > 0 {
                start = line_start;
                end += newline;
            }
        }
        let mut edits = vec![(start, end, String::new())];
        // The last entry loses its comma when the entry after it goes, unless the removed entry
        // had one too, which shows the file keeps trailing commas.
        if member.comma.is_none()
            && let Some(comma) = index
                .checked_sub(1)
                .and_then(|previous| members[previous].comma)
        {
            edits.push((comma, comma + 1, String::new()));
        }
        apply_edits(&mut self.text, edits);
        true
    }
}

/// The text edits that set `value` at `path`, or `None` when a value in the way is not an object.
fn insertion(
    text: &str,
    root: &Node,
    path: &[&str],
    value: &Value,
) -> Option<Vec<(usize, usize, String)>> {
    let mut object = root;
    for (depth, key) in path.iter().enumerate() {
        let NodeKind::Object(members) = &object.kind else {
            return None;
        };
        let rest = &path[depth + 1..];
        match members.iter().rev().find(|member| member.key == **key) {
            Some(member) if rest.is_empty() => {
                if matches!(member.value.kind, NodeKind::Object(_) | NodeKind::Array(_)) {
                    return None;
                }
                let replacement = serde_json::to_string(value).ok()?;
                return Some(vec![(member.value.start, member.value.end, replacement)]);
            }
            Some(member) => {
                if !matches!(member.value.kind, NodeKind::Object(_)) {
                    return None;
                }
                object = &member.value;
            }
            None => return new_member_edits(text, object, members, key, nested(rest, value)),
        }
    }
    None
}

/// `value` wrapped in one object per key of `keys`.
fn nested(keys: &[&str], value: &Value) -> Value {
    keys.iter().rev().fold(value.clone(), |inner, key| {
        let mut object = Map::new();
        object.insert((*key).to_string(), inner);
        Value::Object(object)
    })
}

/// The edits that add `"key": value` as the last entry of `object`, matching how the object
/// already lays out its entries.
fn new_member_edits(
    text: &str,
    object: &Node,
    members: &[Member],
    key: &str,
    value: Value,
) -> Option<Vec<(usize, usize, String)>> {
    let multiline = text[object.start..object.end].contains('\n') || members.is_empty();
    let indent = match members.first() {
        Some(first) if multiline => line_indent(text, first.key_start).to_string(),
        _ => format!("{}{INDENT_STEP}", line_indent(text, object.start)),
    };
    let key = serde_json::to_string(key).ok()?;
    let value = if multiline {
        serde_json::to_string_pretty(&value)
            .ok()?
            .replace('\n', &format!("\n{indent}"))
    } else {
        serde_json::to_string(&value).ok()?
    };
    let entry = format!("{key}: {value}");
    let Some(last) = members.last() else {
        let mut edits = vec![(
            object.start + 1,
            object.start + 1,
            format!("\n{indent}{entry}"),
        )];
        if !text[object.start..object.end].contains('\n') {
            let closing = line_indent(text, object.start).to_string();
            edits.push((object.end - 1, object.end - 1, format!("\n{closing}")));
        }
        return Some(edits);
    };
    let separator = if multiline {
        format!("\n{indent}")
    } else {
        " ".to_string()
    };
    Some(match last.comma {
        Some(comma) => {
            let at = if multiline {
                line_end(text, comma + 1)
            } else {
                comma + 1
            };
            vec![(at, at, format!("{separator}{entry}"))]
        }
        None => {
            let end = last.value.end;
            let at = if multiline { line_end(text, end) } else { end };
            vec![
                (end, end, ",".to_string()),
                (at, at, format!("{separator}{entry}")),
            ]
        }
    })
}

/// Applies non-overlapping `(start, end, replacement)` edits, starting from the last one so the
/// earlier positions stay valid. Insertions at the same position keep their listed order.
fn apply_edits(text: &mut String, edits: Vec<(usize, usize, String)>) {
    let mut edits = edits.into_iter().enumerate().collect::<Vec<_>>();
    edits.sort_by_key(|(index, (start, end, _))| std::cmp::Reverse((*start, *end, *index)));
    for (_, (start, end, replacement)) in edits {
        text.replace_range(start..end, &replacement);
    }
}

/// The whitespace that starts the line holding `position`.
fn line_indent(text: &str, position: usize) -> &str {
    let line_start = text[..position].rfind('\n').map_or(0, |at| at + 1);
    let line = &text[line_start..position];
    &line[..line.len() - line.trim_start().len()]
}

/// The position after the spaces and one comment that follow `position` on its line.
fn line_end(text: &str, position: usize) -> usize {
    let bytes = text.as_bytes();
    let mut at = position;
    while matches!(bytes.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    if text[at..].starts_with("//") {
        return text[at..]
            .find('\n')
            .map_or(text.len(), |offset| at + offset);
    }
    if text[at..].starts_with("/*")
        && let Some(offset) = text[at + 2..].find("*/")
        && !text[at..at + 2 + offset].contains('\n')
    {
        return at + 2 + offset + 2;
    }
    at
}

/// The node at `path` under `root`, following object entries; the last duplicate key wins, as in
/// JSON parsers.
fn node_at<'a>(root: &'a Node, path: &[&str]) -> Option<&'a Node> {
    path.iter().try_fold(root, |node, key| match &node.kind {
        NodeKind::Object(members) => members
            .iter()
            .rev()
            .find(|member| member.key == *key)
            .map(|member| &member.value),
        _ => None,
    })
}

fn json_value(node: &Node) -> Value {
    match &node.kind {
        NodeKind::Object(members) => Value::Object(
            members
                .iter()
                .map(|member| (member.key.clone(), json_value(&member.value)))
                .collect(),
        ),
        NodeKind::Array(items) => Value::Array(items.iter().map(json_value).collect()),
        NodeKind::Scalar(value) => value.clone(),
    }
}

/// The document's top-level object, or `None` when the text is not valid JSONC.
fn parse_root(text: &str) -> Option<Node> {
    let mut parser = Parser { text, position: 0 };
    parser.skip_trivia()?;
    let root = parser.value(0)?;
    parser.skip_trivia()?;
    (parser.position == text.len() && matches!(root.kind, NodeKind::Object(_))).then_some(root)
}

struct Parser<'a> {
    text: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.position).copied()
    }

    /// Skips whitespace and comments; `None` for an unterminated block comment or a stray slash.
    fn skip_trivia(&mut self) -> Option<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.position += 1,
                Some(b'/') => {
                    let rest = &self.text[self.position..];
                    if rest.starts_with("//") {
                        self.position += rest.find('\n').unwrap_or(rest.len());
                    } else {
                        let comment = rest.strip_prefix("/*")?;
                        self.position += 2 + comment.find("*/")? + 2;
                    }
                }
                _ => return Some(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Node> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek()? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => {
                let start = self.position;
                let end = self.string_end()?;
                let value = serde_json::from_str(&self.text[start..end]).ok()?;
                Some(Node {
                    start,
                    end,
                    kind: NodeKind::Scalar(value),
                })
            }
            _ => self.literal(),
        }
    }

    /// A number, `true`, `false`, or `null`.
    fn literal(&mut self) -> Option<Node> {
        let start = self.position;
        let length = self.text.as_bytes()[start..]
            .iter()
            .position(|byte| {
                matches!(
                    byte,
                    b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r' | b'/'
                )
            })
            .unwrap_or(self.text.len() - start);
        let end = start + length;
        let value: Value = serde_json::from_str(&self.text[start..end]).ok()?;
        self.position = end;
        Some(Node {
            start,
            end,
            kind: NodeKind::Scalar(value),
        })
    }

    /// Moves past the string starting at the current quote and returns its end.
    fn string_end(&mut self) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let mut at = self.position + 1;
        loop {
            match bytes.get(at)? {
                b'\\' => at += 2,
                b'"' => {
                    self.position = at + 1;
                    return Some(self.position);
                }
                _ => at += 1,
            }
        }
    }

    fn object(&mut self, depth: usize) -> Option<Node> {
        let start = self.position;
        self.position += 1;
        let mut members: Vec<Member> = Vec::new();
        loop {
            self.skip_trivia()?;
            match self.peek()? {
                b'}' => {
                    self.position += 1;
                    return Some(Node {
                        start,
                        end: self.position,
                        kind: NodeKind::Object(members),
                    });
                }
                b'"' if members.last().is_none_or(|member| member.comma.is_some()) => {}
                _ => return None,
            }
            let key_start = self.position;
            let key_end = self.string_end()?;
            let key = serde_json::from_str(&self.text[key_start..key_end]).ok()?;
            self.skip_trivia()?;
            if self.peek()? != b':' {
                return None;
            }
            self.position += 1;
            self.skip_trivia()?;
            let value = self.value(depth + 1)?;
            self.skip_trivia()?;
            let comma = (self.peek() == Some(b',')).then(|| {
                self.position += 1;
                self.position - 1
            });
            members.push(Member {
                key,
                key_start,
                value,
                comma,
            });
        }
    }

    fn array(&mut self, depth: usize) -> Option<Node> {
        let start = self.position;
        self.position += 1;
        let mut items = Vec::new();
        let mut separated = true;
        loop {
            self.skip_trivia()?;
            if self.peek()? == b']' {
                self.position += 1;
                return Some(Node {
                    start,
                    end: self.position,
                    kind: NodeKind::Array(items),
                });
            }
            if !separated {
                return None;
            }
            items.push(self.value(depth + 1)?);
            self.skip_trivia()?;
            separated = self.peek() == Some(b',');
            if separated {
                self.position += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edited(contents: &str, edit: impl FnOnce(&mut JsoncDocument)) -> String {
        let mut document = JsoncDocument::parse(contents).unwrap();
        edit(&mut document);
        document.render().unwrap()
    }

    #[test]
    fn reads_skip_comments_and_trailing_commas() {
        let document = JsoncDocument::parse(
            "// OpenCode\n{\n  /* sharing */ \"share\": \"disabled\", // off\n  \"compaction\": { \"auto\": false, },\n  \"list\": [1, 2,],\n  \"share\": \"auto\",\n}\n",
        )
        .unwrap();
        assert_eq!(document.value_at(&["share"]), Some(json!("auto")));
        assert_eq!(
            document.value_at(&["compaction", "auto"]),
            Some(json!(false))
        );
        assert_eq!(document.value_at(&["list"]), Some(json!([1, 2])));
        assert_eq!(document.value_at(&["missing"]), None);
    }

    #[test]
    fn invalid_documents_do_not_parse() {
        for contents in [
            "[]",
            "{ \"a\": 1 \"b\": 2 }",
            "{ \"a\": 1 } x",
            "{ /* open",
            "{ , }",
            "{ \"a\": [1 2] }",
            "{ \"a\": tru }",
            "{ \"a\": / }",
        ] {
            assert!(JsoncDocument::parse(contents).is_none(), "{contents}");
        }
        let deep = format!("{{\"a\": {}{}}}", "[".repeat(100), "]".repeat(100));
        assert!(JsoncDocument::parse(&deep).is_none());
        assert!(JsoncDocument::parse("  ").is_some());
    }

    #[test]
    fn replacing_a_value_keeps_its_comment() {
        let contents =
            "{\n  // Share links\n  \"share\": \"manual\", // default\n  \"snapshots\": true\n}\n";
        let result = edited(contents, |document| {
            assert!(document.insert_at(&["share"], json!("disabled")));
        });
        assert_eq!(
            result,
            "{\n  // Share links\n  \"share\": \"disabled\", // default\n  \"snapshots\": true\n}\n"
        );
    }

    #[test]
    fn new_entries_follow_the_object_layout() {
        let contents = "{\n    \"$schema\": \"https://opencode.ai/config.json\" // schema\n}\n";
        let result = edited(contents, |document| {
            assert!(document.insert_at(&["compaction", "keep", "tokens"], json!(15000)));
        });
        assert_eq!(
            result,
            "{\n    \"$schema\": \"https://opencode.ai/config.json\", // schema\n    \"compaction\": {\n      \"keep\": {\n        \"tokens\": 15000\n      }\n    }\n}\n"
        );
        let result = edited("{ \"a\": 1, }", |document| {
            assert!(document.insert_at(&["b"], json!(true)));
        });
        assert_eq!(result, "{ \"a\": 1, \"b\": true }");
        let result = edited("{\"a\": 1}", |document| {
            assert!(document.insert_at(&["b"], json!(true)));
        });
        assert_eq!(result, "{\"a\": 1, \"b\": true}");
        let result = edited("", |document| {
            assert!(document.insert_at(&["update"], json!("notify")));
        });
        assert_eq!(result, "{\n  \"update\": \"notify\"\n}\n");
    }

    #[test]
    fn removals_take_the_line_comma_and_empty_parents() {
        let contents = "{\n  \"share\": \"auto\",\n  // Compaction\n  \"compaction\": {\n    \"auto\": false // off\n  }\n}\n";
        let result = edited(contents, |document| {
            document.remove_at(&["compaction", "auto"]);
        });
        assert_eq!(result, "{\n  \"share\": \"auto\"\n  // Compaction\n}\n");
        let result = edited("{\n  \"a\": 1,\n  \"b\": 2,\n}\n", |document| {
            document.remove_at(&["b"]);
        });
        assert_eq!(result, "{\n  \"a\": 1,\n}\n");
        let result = edited("{\"a\": 1, \"b\": 2}", |document| {
            document.remove_at(&["a"]);
            document.remove_at(&["missing"]);
        });
        assert_eq!(result, "{\"b\": 2}");
    }

    #[test]
    fn values_in_the_way_block_edits() {
        let contents = "{ \"compaction\": true, \"tool_output\": { \"max_lines\": { } } }";
        let mut document = JsoncDocument::parse(contents).unwrap();
        assert!(!document.insert_at(&["compaction", "auto"], json!(true)));
        assert!(!document.insert_at(&["tool_output", "max_lines"], json!(10)));
        assert_eq!(document.render().unwrap(), contents);
    }
}
