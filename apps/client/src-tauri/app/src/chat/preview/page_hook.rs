//! Snapshot and wait scripts that run through a hook installed before page scripts.
//!
//! Preview pages are untrusted and share the main world with evaluated scripts, so a page can
//! patch `JSON.stringify`, `String.prototype.slice`, or DOM getters to inflate or break a
//! result. `page_hook.js` captures those intrinsics before any page script runs and bounds the
//! result in the page. When the hook is missing on a document, snapshots and waits fail
//! instead of falling back to the page's own objects.

use super::persistence::MAX_PREVIEW_TITLE_CHARS;
use super::{MAX_BROWSER_RESULT_BYTES, MAX_BROWSER_URL_BYTES};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use std::sync::OnceLock;

const PAGE_HOOK_SOURCE: &str = include_str!("page_hook.js");
/// Random bytes in the hook's property name, chosen once per app run.
const HOOK_NAME_RANDOM_BYTES: usize = 16;
/// In-page budget for the engine-encoded snapshot, below `MAX_BROWSER_RESULT_BYTES` so the
/// Rust check never sees a result the page already fitted.
const SNAPSHOT_RESULT_BUDGET_BYTES: usize = MAX_BROWSER_RESULT_BYTES - 64 * 1024;
/// Characters of visible text and of HTML a snapshot reads before fitting the budget.
const SNAPSHOT_FIELD_CHARS: usize = 1_000_000;
/// Halvings of the HTML and then the text before a snapshot drops both.
const SNAPSHOT_FIT_ROUNDS: u32 = 48;

const _: () = assert!(SNAPSHOT_RESULT_BUDGET_BYTES < MAX_BROWSER_RESULT_BYTES);

static HOOK_NAME: OnceLock<String> = OnceLock::new();

/// Whether a selector matched when a wait polled the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectorMatch {
    Matched,
    Absent,
}

fn hook_name() -> ChatResult<&'static str> {
    if let Some(name) = HOOK_NAME.get() {
        return Ok(name);
    }
    let generated = format!(
        "ganbaruPreview{}",
        crate::chat::internal_mcp::secure_random_hex(HOOK_NAME_RANDOM_BYTES)?
    );
    Ok(HOOK_NAME.get_or_init(|| generated))
}

/// Initialization script that installs the hook in each top-level preview document.
pub(super) fn initialization_script() -> ChatResult<String> {
    let config = serde_json::json!({
        "name": hook_name()?,
        "resultBudgetBytes": SNAPSHOT_RESULT_BUDGET_BYTES,
        "fieldChars": SNAPSHOT_FIELD_CHARS,
        "urlChars": MAX_BROWSER_URL_BYTES,
        "titleChars": MAX_PREVIEW_TITLE_CHARS,
        "fitRounds": SNAPSHOT_FIT_ROUNDS,
    });
    Ok(format!("({})({config});", PAGE_HOOK_SOURCE.trim_end()))
}

fn hook_reference() -> ChatResult<String> {
    let name = serde_json::to_string(hook_name()?).map_err(|_| hook_unavailable_error())?;
    Ok(format!("window[{name}]"))
}

/// Script returning the bounded snapshot JSON as a string.
pub(super) fn snapshot_script() -> ChatResult<String> {
    Ok(format!("{}.snapshot()", hook_reference()?))
}

/// Script reporting whether `selector` matches an element.
pub(super) fn matches_script(selector: &str) -> ChatResult<String> {
    let selector = serde_json::to_string(selector).map_err(|_| hook_unavailable_error())?;
    Ok(format!("{}.matches({selector})", hook_reference()?))
}

/// Accepts only a snapshot the hook produced. A missing hook or a thrown script yields an
/// empty result on WebKitGTK and `null` on WebView2, and a page that ran out of budget
/// returns `null`.
pub(super) fn snapshot_result(result: String) -> ChatResult<String> {
    if result.starts_with('"') {
        Ok(result)
    } else {
        Err(hook_unavailable_error())
    }
}

/// Interprets the engine-encoded result of `matches_script`.
pub(super) fn selector_match(result: &str) -> ChatResult<SelectorMatch> {
    match result {
        "\"matched\"" => Ok(SelectorMatch::Matched),
        "\"absent\"" => Ok(SelectorMatch::Absent),
        "\"invalid\"" => Err(ChatError::new(
            ChatErrorCode::Validation,
            "The selector is not a valid CSS selector",
            false,
        )),
        _ => Err(hook_unavailable_error()),
    }
}

fn hook_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "This preview page cannot be inspected. Reload it and try again",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_scripts_reach_the_page_only_through_the_run_hook() {
        let name = hook_name().unwrap();
        assert!(name.len() > "ganbaruPreview".len() + HOOK_NAME_RANDOM_BYTES);
        assert_eq!(hook_name().unwrap(), name, "one name per app run");
        let reference = format!("window[\"{name}\"]");
        assert_eq!(
            snapshot_script().unwrap(),
            format!("{reference}.snapshot()")
        );
        assert_eq!(
            matches_script("a\"]); fetch('x');//").unwrap(),
            format!("{reference}.matches(\"a\\\"]); fetch('x');//\")")
        );

        let init = initialization_script().unwrap();
        assert!(init.starts_with("("));
        assert!(init.contains("(config) =>"));
        assert!(init.ends_with("});"));
        for setting in [
            format!("\"name\":\"{name}\""),
            format!("\"resultBudgetBytes\":{SNAPSHOT_RESULT_BUDGET_BYTES}"),
            format!("\"fieldChars\":{SNAPSHOT_FIELD_CHARS}"),
            format!("\"urlChars\":{MAX_BROWSER_URL_BYTES}"),
            format!("\"titleChars\":{MAX_PREVIEW_TITLE_CHARS}"),
            format!("\"fitRounds\":{SNAPSHOT_FIT_ROUNDS}"),
        ] {
            assert!(init.contains(&setting), "missing {setting}");
        }
    }

    #[test]
    fn missing_hooks_and_thrown_scripts_fail_instead_of_returning_empty_results() {
        assert_eq!(
            snapshot_result("\"{\\\"url\\\":\\\"http://127.0.0.1:5173/\\\"}\"".to_string())
                .unwrap(),
            "\"{\\\"url\\\":\\\"http://127.0.0.1:5173/\\\"}\""
        );
        for failed in ["", "null", "undefined", "{}"] {
            assert_eq!(
                snapshot_result(failed.to_string()).unwrap_err().code,
                ChatErrorCode::DriverUnavailable
            );
        }

        assert_eq!(
            selector_match("\"matched\"").unwrap(),
            SelectorMatch::Matched
        );
        assert_eq!(selector_match("\"absent\"").unwrap(), SelectorMatch::Absent);
        assert_eq!(
            selector_match("\"invalid\"").unwrap_err().code,
            ChatErrorCode::Validation
        );
        for failed in ["", "null", "true", "\"true\""] {
            assert_eq!(
                selector_match(failed).unwrap_err().code,
                ChatErrorCode::DriverUnavailable
            );
        }
    }
}
