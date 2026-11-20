//! Web fetch URL rules shared by every provider's organizational permission routing.

/// Largest URL a web fetch may name, so the approval shows it whole.
pub const MAX_WEB_FETCH_URL_BYTES: usize = 2048;

/// Whether `url` is a short http or https URL a person can read whole before allowing it.
pub fn fetchable_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    url.len() <= MAX_WEB_FETCH_URL_BYTES
        && (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetchable_urls_are_short_web_urls_without_control_characters() {
        assert!(fetchable_url("https://example.org/a"));
        assert!(fetchable_url("HTTP://example.org"));
        assert!(!fetchable_url("file:///etc/passwd"));
        assert!(!fetchable_url("javascript:alert(1)"));
        assert!(!fetchable_url("https://example.org/\nnext"));
        let long = format!(
            "https://example.org/{}",
            "a".repeat(MAX_WEB_FETCH_URL_BYTES)
        );
        assert!(!fetchable_url(&long));
    }
}
