//! Model access to preview tabs: loopback pages, plus external origins the user allowed per thread.

use super::navigation::is_loopback;
use super::*;
use crate::chat::device_state::{read_active_device_scope, update_active_device_scope};
use std::collections::{BTreeMap, BTreeSet};

/// Most external origins one thread may hold for model access.
pub(super) const MAX_PREVIEW_MODEL_ORIGINS_PER_THREAD: usize = 64;
/// Most threads that may hold external origin grants on one device and vault.
pub(super) const MAX_PREVIEW_MODEL_ORIGIN_THREADS: usize = 512;

/// One external origin the user let the model use in one thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewModelOriginGrant {
    pub thread_id: String,
    pub origin: String,
}

/// Returns the ASCII origin used as a grant key, or `None` for URLs without a tuple origin.
pub(super) fn model_origin(url: &reqwest::Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

/// Decides whether the model may read or act on a tab. Every URL that describes the tab (the
/// live webview URL, the navigation allowance, and the last committed document) must be
/// loopback or carry an origin the user granted to this thread, so a navigation in flight
/// never exposes the document it is leaving.
pub(super) fn model_may_access(urls: &[&reqwest::Url], grants: &BTreeSet<String>) -> bool {
    !urls.is_empty()
        && urls.iter().all(|url| {
            is_loopback(url) || model_origin(url).is_some_and(|origin| grants.contains(&origin))
        })
}

fn denied_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "The model can use this preview page only after the user allows its site in the Browser panel",
        true,
    )
}

fn grant_storage_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Browser site access could not be read or saved",
        true,
    )
}

fn thread_grants(app: &tauri::AppHandle, thread_id: &ChatThreadId) -> BTreeSet<String> {
    read_active_device_scope(app)
        .ok()
        .and_then(|mut scope| scope.preview_model_origins.remove(thread_id.as_str()))
        .unwrap_or_default()
}

fn parse_url(value: &str) -> ChatResult<reqwest::Url> {
    reqwest::Url::parse(value).map_err(|_| denied_error())
}

/// Refuses the tab unless the model may use what it shows right now. Fails closed when the
/// webview or its URL is unavailable.
pub(super) fn require_model_access(
    app: &tauri::AppHandle,
    tab: &RuntimeBrowserTab,
) -> ChatResult<()> {
    let live = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?
        .url()
        .map_err(|_| browser_unavailable_error())?;
    let live = parse_url(live.as_str())?;
    let allowed = tab
        .allowed_url
        .lock()
        .map_err(|_| browser_state_error())?
        .clone();
    let committed = tab
        .committed_url
        .lock()
        .map_err(|_| browser_state_error())?
        .clone();
    let grants = thread_grants(app, &tab.read.thread_id);
    if model_may_access(&[&live, &allowed, &committed], &grants) {
        Ok(())
    } else {
        Err(denied_error())
    }
}

/// Resolves the thread's active tab once and checks model access to it. Tools then act on
/// the returned tab ID, so a later tab switch cannot retarget them.
pub(crate) fn mcp_model_tab(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
) -> ChatResult<String> {
    let tab = app
        .state::<ChatBrowserManager>()
        .active_thread_tab(thread_id)?;
    require_model_access(app, &tab)?;
    Ok(tab.read.tab_id)
}

/// Rechecks model access to a tab already resolved for this thread.
pub(crate) fn mcp_require_tab_access(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    tab_id: &str,
) -> ChatResult<()> {
    require_model_access(app, &owned_tab(app, thread_id, tab_id)?)
}

/// Thread tab reads for the model. Pages it may not use show only scheme and host, with no
/// page-controlled title.
pub(crate) fn mcp_status_reads(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
) -> ChatResult<Vec<BrowserTabRead>> {
    let grants = thread_grants(app, thread_id);
    let reads = app.state::<ChatBrowserManager>().thread_reads(thread_id)?;
    Ok(reads
        .into_iter()
        .map(|read| redact_for_model(read, &grants))
        .collect())
}

pub(super) fn redact_for_model(
    mut read: BrowserTabRead,
    grants: &BTreeSet<String>,
) -> BrowserTabRead {
    let permitted =
        reqwest::Url::parse(&read.current_url).is_ok_and(|url| model_may_access(&[&url], grants));
    if !permitted {
        read.current_url = reqwest::Url::parse(&read.current_url)
            .ok()
            .and_then(|url| model_origin(&url))
            .unwrap_or_default();
        read.title = String::new();
    }
    read
}

/// Adds an origin to a thread's grants within the named limits.
pub(super) fn insert_grant(
    grants: &mut BTreeMap<String, BTreeSet<String>>,
    thread_id: &str,
    origin: String,
) -> ChatResult<()> {
    if !grants.contains_key(thread_id) && grants.len() >= MAX_PREVIEW_MODEL_ORIGIN_THREADS {
        return Err(ChatError::validation(
            "origin",
            "Too many Chat threads have browser site access. Revoke some in Settings first",
        ));
    }
    let origins = grants.entry(thread_id.to_string()).or_default();
    if !origins.contains(&origin) && origins.len() >= MAX_PREVIEW_MODEL_ORIGINS_PER_THREAD {
        return Err(ChatError::validation(
            "origin",
            "This thread already allows the most sites it can. Revoke some first",
        ));
    }
    origins.insert(origin);
    Ok(())
}

/// Removes an origin from a thread's grants, dropping the thread when none remain.
pub(super) fn remove_grant(
    grants: &mut BTreeMap<String, BTreeSet<String>>,
    thread_id: &str,
    origin: &str,
) {
    if let Some(origins) = grants.get_mut(thread_id) {
        origins.remove(origin);
        if origins.is_empty() {
            grants.remove(thread_id);
        }
    }
}

/// Lists every external origin the model may use, for Settings.
#[tauri::command]
pub async fn chat_browser_model_origins(
    app: tauri::AppHandle,
) -> ChatResult<Vec<PreviewModelOriginGrant>> {
    let scope = read_active_device_scope(&app).map_err(|_| grant_storage_error())?;
    Ok(scope
        .preview_model_origins
        .into_iter()
        .flat_map(|(thread_id, origins)| {
            origins
                .into_iter()
                .map(move |origin| PreviewModelOriginGrant {
                    thread_id: thread_id.clone(),
                    origin,
                })
        })
        .collect())
}

/// Lets the model use the site the tab currently shows. The origin comes from the live
/// webview, never from a webview string, so a grant always names a page the user is looking at.
#[tauri::command]
pub async fn chat_browser_allow_model_origin(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<String> {
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    let live = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?
        .url()
        .map_err(|_| browser_unavailable_error())?;
    let live = reqwest::Url::parse(live.as_str())
        .map_err(|_| ChatError::validation("url", "Browser preview URL is invalid"))?;
    if is_loopback(&live) {
        return Err(ChatError::validation(
            "url",
            "Local previews are always available to the model",
        ));
    }
    let origin = model_origin(&live)
        .ok_or_else(|| ChatError::validation("url", "Browser preview URL is invalid"))?;
    let stored = origin.clone();
    let mut outcome = Ok(());
    update_active_device_scope(&app, |scope| {
        outcome = insert_grant(&mut scope.preview_model_origins, thread_id.as_str(), stored);
        Ok(())
    })
    .map_err(|_| grant_storage_error())?;
    outcome.map(|()| origin)
}

/// Stops the model from using an origin in a thread.
#[tauri::command]
pub async fn chat_browser_revoke_model_origin(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    origin: String,
) -> ChatResult<()> {
    update_active_device_scope(&app, |scope| {
        remove_grant(
            &mut scope.preview_model_origins,
            thread_id.as_str(),
            &origin,
        );
        Ok(())
    })
    .map_err(|_| grant_storage_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> reqwest::Url {
        value.parse().expect("test URL should parse")
    }

    fn granted(origins: &[&str]) -> BTreeSet<String> {
        origins.iter().map(|origin| (*origin).to_string()).collect()
    }

    #[test]
    fn loopback_pages_are_always_available_to_the_model() {
        let page = url("http://127.0.0.1:5173/app");
        assert!(model_may_access(&[&page, &page, &page], &BTreeSet::new()));
        let named = url("http://localhost:3000/");
        assert!(model_may_access(&[&named], &BTreeSet::new()));
    }

    #[test]
    fn external_pages_without_a_grant_are_denied() {
        let page = url("https://github.com/settings/tokens");
        assert!(!model_may_access(&[&page, &page, &page], &BTreeSet::new()));
        let other = granted(&["https://example.com"]);
        assert!(!model_may_access(&[&page], &other));
    }

    #[test]
    fn a_loopback_allowance_does_not_expose_the_external_page_still_shown() {
        let allowed = url("http://127.0.0.1:5173/");
        let current = url("https://mail.example.com/inbox");
        assert!(!model_may_access(
            &[&allowed, &allowed, &current],
            &BTreeSet::new()
        ));
        assert!(!model_may_access(
            &[&current, &allowed, &allowed],
            &BTreeSet::new()
        ));
    }

    #[test]
    fn external_pages_with_a_grant_for_their_exact_origin_are_allowed() {
        let page = url("https://github.com/owner/repo/issues/1");
        let grants = granted(&["https://github.com"]);
        assert!(model_may_access(&[&page, &page, &page], &grants));
        assert!(!model_may_access(&[&url("http://github.com/")], &grants));
        assert!(!model_may_access(
            &[&url("https://github.com:8443/")],
            &grants
        ));
        assert!(!model_may_access(
            &[&url("https://gist.github.com/")],
            &grants
        ));
        assert!(!model_may_access(&[], &grants));
    }

    #[test]
    fn model_status_shows_only_scheme_and_host_for_pages_it_may_not_use() {
        let read = |current_url: &str| BrowserTabRead {
            thread_id: ChatThreadId::new("thread:test").expect("thread ID should be valid"),
            tab_id: "preview:thread:test:main".into(),
            current_url: current_url.into(),
            title: "Inbox (3) - private subject".into(),
            visible: true,
            loading: false,
            viewport_width: 800,
            viewport_height: 600,
            external_origin: true,
        };
        let hidden = redact_for_model(
            read("https://mail.example.com/inbox?q=secret"),
            &BTreeSet::new(),
        );
        assert_eq!(hidden.current_url, "https://mail.example.com");
        assert!(hidden.title.is_empty());
        let shown = redact_for_model(
            read("https://mail.example.com/inbox"),
            &granted(&["https://mail.example.com"]),
        );
        assert_eq!(shown.current_url, "https://mail.example.com/inbox");
        assert_eq!(shown.title, "Inbox (3) - private subject");
    }

    #[test]
    fn origin_grants_are_bounded_per_thread_and_per_device() {
        let mut grants = BTreeMap::new();
        for index in 0..MAX_PREVIEW_MODEL_ORIGINS_PER_THREAD {
            insert_grant(
                &mut grants,
                "thread:a",
                format!("https://site{index}.example"),
            )
            .expect("grant within the limit should be stored");
        }
        insert_grant(&mut grants, "thread:a", "https://site0.example".into())
            .expect("an existing grant should be accepted again");
        let error = insert_grant(&mut grants, "thread:a", "https://one-more.example".into())
            .expect_err("a grant over the thread limit should be refused");
        assert_eq!(error.code, ChatErrorCode::Validation);
        for index in 1..MAX_PREVIEW_MODEL_ORIGIN_THREADS {
            insert_grant(
                &mut grants,
                &format!("thread:{index}"),
                "https://example.com".into(),
            )
            .expect("grant within the thread limit should be stored");
        }
        assert!(insert_grant(&mut grants, "thread:over", "https://example.com".into()).is_err());
        remove_grant(&mut grants, "thread:1", "https://example.com");
        assert!(!grants.contains_key("thread:1"));
        insert_grant(&mut grants, "thread:over", "https://example.com".into())
            .expect("revoking frees room for another thread");
    }
}
