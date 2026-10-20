//! URL scheme rules for bookmark, embed, and link preview blocks.

/// Schemes that run script or read local data when followed. Notes never stores or exports them as web reference destinations.
const BLOCKED_SCHEMES: &[&str] = &["javascript", "data", "vbscript", "file"];

/// Schemes that load remote content, the only ones embed and link preview blocks keep.
const WEB_SCHEMES: &[&str] = &["http", "https"];

/// Lowercase URI scheme that starts `value`, following RFC 3986 scheme syntax.
///
/// Surrounding whitespace and the tab and newline characters that URL parsers strip are ignored, so `" Java\tScript:x"` reports `javascript`. A value without a scheme, such as `example.com/a:b`, returns `None`.
pub fn uri_scheme(value: &str) -> Option<String> {
    let mut scheme = String::new();
    for character in value
        .trim()
        .chars()
        .filter(|character| !matches!(character, '\t' | '\n' | '\r'))
    {
        match character {
            ':' if !scheme.is_empty() => return Some(scheme),
            'a'..='z' | 'A'..='Z' => scheme.push(character.to_ascii_lowercase()),
            '0'..='9' | '+' | '-' | '.' if !scheme.is_empty() => scheme.push(character),
            _ => return None,
        }
    }
    None
}

/// Whether `value` is a `host:port` draft such as `localhost:3000` rather than a scheme.
fn is_host_port_draft(value: &str) -> bool {
    let Some((_, rest)) = value.trim().split_once(':') else {
        return false;
    };
    let port_len = rest.chars().take_while(char::is_ascii_digit).count();
    port_len > 0 && matches!(rest[port_len..].chars().next(), None | Some('/'))
}

fn has_blocked_scheme(value: &str) -> bool {
    uri_scheme(value).is_some_and(|scheme| BLOCKED_SCHEMES.contains(&scheme.as_str()))
}

/// Whether a bookmark may store `url`.
///
/// Bookmarks keep web, email, telephone, and application links such as `obsidian://`, and drafts the editor saves while the user types. Only control characters and script or local-data schemes are rejected.
pub fn is_storable_bookmark_url(url: &str) -> bool {
    !url.chars().any(char::is_control) && !has_blocked_scheme(url)
}

/// Whether an embed or link preview block may store `url`.
///
/// Accepts an empty value, `http` and `https` URLs, and the scheme-less or `host:port` drafts the editor saves while the user types. Any other explicit scheme is rejected because these blocks describe loadable web content.
pub fn is_storable_web_content_url(url: &str) -> bool {
    if url.chars().any(char::is_control) {
        return false;
    }
    let Some(scheme) = uri_scheme(url) else {
        return true;
    };
    if BLOCKED_SCHEMES.contains(&scheme.as_str()) {
        return false;
    }
    WEB_SCHEMES.contains(&scheme.as_str()) || is_host_port_draft(url)
}

/// Whether exported documents may write `url` as a followable link for a `block_type` web reference.
///
/// Bookmarks export any explicit scheme other than the blocked ones. Embeds and link previews export only `http` and `https`. Scheme-less drafts never become links, so an exported file cannot point at paths relative to itself.
pub fn is_exportable_web_reference_url(block_type: &str, url: &str) -> bool {
    if url.chars().any(char::is_control) {
        return false;
    }
    let Some(scheme) = uri_scheme(url) else {
        return false;
    };
    if BLOCKED_SCHEMES.contains(&scheme.as_str()) {
        return false;
    }
    block_type == "bookmark" || WEB_SCHEMES.contains(&scheme.as_str())
}
