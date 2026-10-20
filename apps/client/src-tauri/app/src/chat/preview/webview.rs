use super::approvals::PendingPreviewApproval;
use super::model_access::thread_grants;
use super::navigation::*;
use super::persistence::{bounded_preview_title, bounded_preview_url, spawn_tab_persister};
use super::*;
use std::collections::BTreeSet;

pub(super) async fn create_child_browser_webview(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: &OpenBrowserTabRequest,
    url: reqwest::Url,
    bounds: BrowserTabBounds,
) -> ChatResult<BrowserTabRead> {
    use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
    let label = preview_label(&request.tab_id);
    let (working_folder_id, data_directory) =
        super::data_store::preview_data_directory(app, pool, &request.thread_id).await?;
    let allowed_url = Arc::new(Mutex::new(url.clone()));
    let navigation_url = allowed_url.clone();
    let committed_url = Arc::new(Mutex::new(url.clone()));
    let loaded_url = committed_url.clone();
    let manager = app.state::<ChatBrowserManager>().inner().clone();
    let loading_tab = request.tab_id.clone();
    let title_manager = manager.clone();
    let title_tab = request.tab_id.clone();
    let navigation_manager = manager.clone();
    let navigation_app = app.clone();
    let navigation_thread = request.thread_id.clone();
    let navigation_tab = request.tab_id.clone();
    let builder = WebviewBuilder::new(label.clone(), WebviewUrl::External(url.clone()))
        .data_directory(data_directory)
        .initialization_script(super::page_hook::initialization_script()?)
        .on_navigation(move |candidate| {
            let Ok(allowed) = navigation_url.lock().map(|allowed| allowed.clone()) else {
                return false;
            };
            if navigation_allowed(&allowed, candidate, &BTreeSet::new()) {
                return true;
            }
            let grants = thread_grants(&navigation_app, &navigation_thread);
            if navigation_allowed(&allowed, candidate, &grants) {
                return true;
            }
            if loopback_origin_key(&allowed).is_some()
                && loopback_approval_needed(candidate, &grants).is_some()
            {
                navigation_manager.request_approval(
                    &navigation_app,
                    &navigation_thread,
                    PendingPreviewApproval::Navigation {
                        tab_id: navigation_tab.clone(),
                        url: candidate.clone(),
                    },
                );
            }
            false
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_page_load(move |_, payload| {
            if payload.event() == PageLoadEvent::Finished
                && let Ok(mut committed) = loaded_url.lock()
            {
                *committed = payload.url().clone();
            }
            manager.update_from_page(&loading_tab, |read| {
                read.current_url = bounded_preview_url(payload.url());
                read.loading = payload.event() == PageLoadEvent::Started;
                read.external_origin = !is_loopback(payload.url());
            });
        })
        .on_document_title_changed(move |_, title| {
            title_manager.update_from_page(&title_tab, |read| {
                read.title = bounded_preview_title(&title);
            });
        });
    let main = app
        .get_window("main")
        .ok_or_else(browser_unavailable_error)?;
    main.add_child(
        builder,
        tauri::LogicalPosition::new(bounds.x, bounds.y),
        tauri::LogicalSize::new(bounds.width, bounds.height),
    )
    .map_err(|_| browser_unavailable_error())?;
    let read = BrowserTabRead {
        thread_id: request.thread_id.clone(),
        tab_id: request.tab_id.clone(),
        current_url: url.to_string(),
        title: String::new(),
        visible: true,
        loading: true,
        viewport_width: bounds.width.round() as u32,
        viewport_height: bounds.height.round() as u32,
        external_origin: !is_loopback(&url),
    };
    app.state::<ChatBrowserManager>()
        .tabs
        .lock()
        .map_err(|_| browser_state_error())?
        .insert(
            request.tab_id.clone(),
            RuntimeBrowserTab {
                read: read.clone(),
                webview_label: label,
                allowed_url,
                committed_url,
                persister: spawn_tab_persister(pool.clone(), read.clone()),
                working_folder_id,
            },
        );
    persist_tab(pool, &read).await?;
    Ok(read)
}

#[cfg(mobile)]
pub(super) async fn create_child_browser_webview(
    _app: &tauri::AppHandle,
    _pool: &SqlitePool,
    _request: &OpenBrowserTabRequest,
    _url: reqwest::Url,
    _bounds: BrowserTabBounds,
) -> ChatResult<BrowserTabRead> {
    Err(ChatError::unsupported(
        "Embedded browser preview is unavailable on this platform",
    ))
}

pub(super) fn owned_tab(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    tab_id: &str,
) -> ChatResult<RuntimeBrowserTab> {
    let tab = app.state::<ChatBrowserManager>().read(tab_id)?;
    if &tab.read.thread_id != thread_id {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Browser preview tab belongs to another thread",
            true,
        ));
    }
    Ok(tab)
}

pub(super) fn navigate_existing(
    app: &tauri::AppHandle,
    tab: &RuntimeBrowserTab,
    url: reqwest::Url,
) -> ChatResult<()> {
    let webview = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?;
    let previous = {
        let mut allowed = tab.allowed_url.lock().map_err(|_| browser_state_error())?;
        let previous = allowed.clone();
        *allowed = url.clone();
        previous
    };
    let result = webview
        .navigate(url)
        .map_err(|_| browser_unavailable_error());
    if result.is_err() {
        *tab.allowed_url.lock().map_err(|_| browser_state_error())? = previous;
    }
    result
}

pub(super) fn resize_existing(
    app: &tauri::AppHandle,
    tab: &RuntimeBrowserTab,
    bounds: &BrowserTabBounds,
) -> ChatResult<()> {
    let webview = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?;
    webview
        .set_position(tauri::LogicalPosition::new(bounds.x, bounds.y))
        .and_then(|_| webview.set_size(tauri::LogicalSize::new(bounds.width, bounds.height)))
        .map_err(|_| browser_unavailable_error())
}

pub(super) fn run_script(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    tab_id: &str,
    script: &str,
) -> ChatResult<()> {
    let tab = owned_tab(app, thread_id, tab_id)?;
    app.get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?
        .eval(script)
        .map_err(|_| browser_unavailable_error())
}

pub(crate) async fn evaluate_script(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    tab_id: &str,
    script: &str,
) -> ChatResult<String> {
    let tab = owned_tab(app, thread_id, tab_id)?;
    let webview = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    webview
        .eval_with_callback(script, move |result| {
            if let Ok(mut sender) = sender.lock() {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(result);
                }
            }
        })
        .map_err(|_| browser_unavailable_error())?;
    let result = tokio::time::timeout(BROWSER_EVALUATION_TIMEOUT, receiver)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Timeout, "Browser evaluation timed out", true))?
        .map_err(|_| browser_unavailable_error())?;
    if result.len() > MAX_BROWSER_RESULT_BYTES {
        return Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Browser result exceeds the supported limit",
            true,
        ));
    }
    Ok(result)
}
