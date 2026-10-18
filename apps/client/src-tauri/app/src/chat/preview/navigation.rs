use super::*;

pub(super) fn validate_navigation(
    value: &str,
    external_confirmed: bool,
) -> ChatResult<reqwest::Url> {
    if value.is_empty()
        || value.len() > MAX_BROWSER_URL_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "url",
            "Browser preview URL is invalid",
        ));
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| ChatError::validation("url", "Browser preview URL is invalid"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || is_app_protocol_host(&url)
    {
        return Err(ChatError::validation(
            "url",
            "Browser preview URL is not allowed",
        ));
    }
    if !is_loopback(&url) && !external_confirmed {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "External browser navigation requires explicit confirmation",
            true,
        ));
    }
    Ok(url)
}

/// Reports hosts that Tauri serves its own custom protocols from on Windows,
/// such as `http://tauri.localhost` and `http://ipc.localhost`. WebView2
/// intercepts those only on the scheme's default port, so a `.localhost`
/// subdomain with an explicit port stays an ordinary dev server.
pub(super) fn is_app_protocol_host(url: &reqwest::Url) -> bool {
    url.port().is_none()
        && url.host_str().is_some_and(|host| {
            host.trim_end_matches('.')
                .to_ascii_lowercase()
                .ends_with(".localhost")
        })
}

pub(super) fn is_loopback(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

pub(super) fn navigation_allowed(allowed: &reqwest::Url, candidate: &reqwest::Url) -> bool {
    if !matches!(candidate.scheme(), "http" | "https") || is_app_protocol_host(candidate) {
        return false;
    }
    if is_loopback(allowed) {
        return is_loopback(candidate);
    }
    allowed.scheme() == candidate.scheme()
        && allowed.host_str() == candidate.host_str()
        && allowed.port_or_known_default() == candidate.port_or_known_default()
}

pub(super) fn loopback_urls(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut urls = Vec::new();
    for prefix in ["http://", "https://"] {
        let mut remaining = text.as_ref();
        while let Some(index) = remaining.find(prefix) {
            let candidate = &remaining[index..];
            let end = candidate
                .find(|character: char| {
                    character.is_whitespace()
                        || matches!(character, '"' | '\'' | '<' | '>' | ')' | ']' | '}')
                })
                .unwrap_or(candidate.len());
            let value = candidate[..end].trim_end_matches(['.', ',', ';', ':']);
            if let Ok(parsed) = reqwest::Url::parse(value) {
                if is_loopback(&parsed) && parsed.port_or_known_default().is_some() {
                    let origin = format!(
                        "{}://{}:{}",
                        parsed.scheme(),
                        parsed.host_str().unwrap_or("localhost"),
                        parsed.port_or_known_default().unwrap_or_default()
                    );
                    urls.push(origin);
                }
            }
            remaining = &candidate[end.min(candidate.len())..];
            if end == 0 {
                break;
            }
        }
    }
    urls.sort();
    urls.dedup();
    urls
}

pub(super) fn validate_bounds(bounds: &BrowserTabBounds) -> ChatResult<BrowserTabBounds> {
    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || bounds.x < 0.0
        || bounds.y < 0.0
        || !(1.0..=16_384.0).contains(&bounds.width)
        || !(1.0..=16_384.0).contains(&bounds.height)
    {
        return Err(ChatError::validation(
            "bounds",
            "Browser preview bounds are invalid",
        ));
    }
    Ok(bounds.clone())
}

pub(super) fn validate_tab_id(tab_id: &str) -> ChatResult<()> {
    if tab_id.is_empty() || tab_id.len() > 1_024 || tab_id.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "tabId",
            "Browser preview tab ID is invalid",
        ));
    }
    Ok(())
}

pub(super) fn js_string(value: &str, field: &str) -> ChatResult<String> {
    if value.len() > 256 * 1024 || value.contains('\0') {
        return Err(ChatError::validation(
            field,
            "Browser interaction value is invalid",
        ));
    }
    serde_json::to_string(value)
        .map_err(|_| ChatError::validation(field, "Browser interaction value is invalid"))
}

pub(super) fn preview_label(tab_id: &str) -> String {
    format!("chat-preview-{:x}", Sha256::digest(tab_id.as_bytes()))
}
