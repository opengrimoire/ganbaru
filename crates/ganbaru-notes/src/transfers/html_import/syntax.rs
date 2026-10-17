#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HtmlNode {
    Element(HtmlElement),
    Text { text: String, line: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HtmlElement {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) children: Vec<HtmlNode>,
    pub(crate) line: i64,
}

impl HtmlElement {
    fn new(name: impl Into<String>, attrs: Vec<(String, String)>, line: i64) -> Self {
        Self {
            name: name.into(),
            attrs,
            children: Vec::new(),
            line,
        }
    }

    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }

    pub(crate) fn has_attr(&self, name: &str) -> bool {
        self.attrs.iter().any(|(key, _)| key == name)
    }
}

pub(crate) fn parse_html_nodes(input: &str) -> Vec<HtmlNode> {
    let mut stack = vec![HtmlElement::new("#root", Vec::new(), 1)];
    let mut cursor = 0;
    let mut line = 1;
    while cursor < input.len() {
        if input[cursor..].starts_with("<!--") {
            let end = input[cursor + 4..]
                .find("-->")
                .map(|index| cursor + 7 + index)
                .unwrap_or(input.len());
            line += count_newlines(&input[cursor..end]);
            cursor = end;
            continue;
        }
        if input.as_bytes()[cursor] != b'<' {
            let end = input[cursor..]
                .find('<')
                .map(|index| cursor + index)
                .unwrap_or(input.len());
            let text = decode_html_entities(&input[cursor..end]);
            if !text.is_empty() {
                append_node(&mut stack, HtmlNode::Text { text, line });
            }
            line += count_newlines(&input[cursor..end]);
            cursor = end;
            continue;
        }
        let Some(tag_end) = find_tag_end(input, cursor + 1) else {
            let text = decode_html_entities(&input[cursor..]);
            append_node(&mut stack, HtmlNode::Text { text, line });
            break;
        };
        let tag_line = line;
        let raw_tag = &input[cursor + 1..tag_end];
        line += count_newlines(&input[cursor..=tag_end]);
        cursor = tag_end + 1;
        let trimmed = raw_tag.trim();
        if trimmed.is_empty() || trimmed.starts_with('!') || trimmed.starts_with('?') {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix('/') {
            close_element(&mut stack, normalized_name(name));
            continue;
        }
        let self_closing = trimmed.ends_with('/');
        let content = trimmed.trim_end_matches('/').trim();
        let (name, attrs) = parse_start_tag(content);
        if name.is_empty() {
            continue;
        }
        let node = HtmlElement::new(name, attrs, tag_line);
        if self_closing || is_void_element(&node.name) {
            append_node(&mut stack, HtmlNode::Element(node));
        } else {
            stack.push(node);
        }
    }
    while stack.len() > 1 {
        let node = stack.pop().expect("html parser stack must contain child");
        append_node(&mut stack, HtmlNode::Element(node));
    }
    stack.pop().map(|root| root.children).unwrap_or_default()
}

fn append_node(stack: &mut [HtmlElement], node: HtmlNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    }
}

fn close_element(stack: &mut Vec<HtmlElement>, name: String) {
    if stack.len() <= 1 {
        return;
    }
    let Some(top) = stack.last() else {
        return;
    };
    if top.name != name {
        return;
    }
    let node = stack
        .pop()
        .expect("html parser stack must contain top element");
    append_node(stack, HtmlNode::Element(node));
}

fn find_tag_end(input: &str, start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in input.as_bytes()[start..].iter().enumerate() {
        match (*byte, quote) {
            (b'\'' | b'"', None) => quote = Some(*byte),
            (value, Some(current)) if value == current => quote = None,
            (b'>', None) => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn parse_start_tag(input: &str) -> (String, Vec<(String, String)>) {
    let mut cursor = 0;
    let bytes = input.as_bytes();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let name_start = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b'=' {
        cursor += 1;
    }
    let name = normalized_name(&input[name_start..cursor]);
    (name, parse_attrs(&input[cursor..]))
}

fn parse_attrs(input: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let bytes = input.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && !matches!(bytes[cursor], b'=' | b'/' | b'>')
        {
            cursor += 1;
        }
        let name = normalized_name(&input[name_start..cursor]);
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut value = String::new();
        if cursor < bytes.len() && bytes[cursor] == b'=' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && matches!(bytes[cursor], b'\'' | b'"') {
                let quote = bytes[cursor];
                cursor += 1;
                let value_start = cursor;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                value = decode_html_entities(&input[value_start..cursor]);
                if cursor < bytes.len() {
                    cursor += 1;
                }
            } else {
                let value_start = cursor;
                while cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && !matches!(bytes[cursor], b'/' | b'>')
                {
                    cursor += 1;
                }
                value = decode_html_entities(&input[value_start..cursor]);
            }
        }
        if !name.is_empty() {
            attrs.push((name, value));
        }
    }
    attrs
}

fn normalized_name(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('/')
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn count_newlines(value: &str) -> i64 {
    value.bytes().filter(|byte| *byte == b'\n').count() as i64
}

pub(crate) fn decode_html_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative_start) = value[cursor..].find('&') {
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        let Some(relative_end) = value[start..].find(';') else {
            cursor = start;
            break;
        };
        let end = start + relative_end;
        let entity = &value[start + 1..end];
        if let Some(decoded) = decode_entity(entity) {
            output.push(decoded);
            cursor = end + 1;
        } else {
            output.push('&');
            cursor = start + 1;
        }
    }
    output.push_str(&value[cursor..]);
    output
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}
