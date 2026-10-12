use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;

/// Open an explicitly activated Notes link through the system after validating its destination.
#[tauri::command]
pub fn notes_open_external_url<R: Runtime>(app: AppHandle<R>, url: String) -> Result<(), String> {
    let parsed = validated_external_notes_url(&url)?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|error| format!("open external Notes link: {error}"))
}

/// Reuse rich-text bounds and scheme validation without admitting local links or URL credentials.
fn validated_external_notes_url(value: &str) -> Result<reqwest::Url, String> {
    ganbaru_notes::validation::validate_rich_text_url(value, "Notes link")?;
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| "External Notes links require a web or email URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("External Notes links require an uncredentialed web or email URL".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::validated_external_notes_url;

    #[test]
    fn external_notes_links_allow_only_bounded_uncredentialed_web_and_email_urls() {
        for value in [
            "https://example.com/tasks",
            "http://localhost:3000/",
            "mailto:team@example.com?subject=Tasks",
        ] {
            assert!(
                validated_external_notes_url(value).is_ok(),
                "valid link rejected: {value}"
            );
        }
        for value in [
            "javascript:alert(1)",
            "file:///tmp/private",
            "tauri://localhost/",
            "https://token@example.com/",
            "https://user:password@example.com/",
            "https://example.com/\nheader",
            "mailto:team@example",
            "#notes?page=11111111-1111-4111-8111-111111111111",
            "https://",
            "",
        ] {
            assert!(
                validated_external_notes_url(value).is_err(),
                "invalid link accepted: {value}"
            );
        }
        assert!(
            validated_external_notes_url(&format!("https://example.com/{}", "a".repeat(2048)))
                .is_err()
        );
    }
}
