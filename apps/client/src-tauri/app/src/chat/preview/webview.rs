use super::navigation::*;
use super::*;

pub(super) async fn create_child_browser_webview(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: &OpenBrowserTabRequest,
    url: reqwest::Url,
    bounds: BrowserTabBounds,
) -> ChatResult<BrowserTabRead> {
    use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
    let label = preview_label(&request.tab_id);
    let allowed_url = Arc::new(Mutex::new(url.clone()));
    let navigation_url = allowed_url.clone();
    let manager = app.state::<ChatBrowserManager>().inner().clone();
    let loading_tab = request.tab_id.clone();
    let loading_pool = pool.clone();
    let title_manager = manager.clone();
    let title_tab = request.tab_id.clone();
    let title_pool = pool.clone();
    let builder = WebviewBuilder::new(label.clone(), WebviewUrl::External(url.clone()))
        .on_navigation(move |candidate| {
            navigation_url
                .lock()
                .is_ok_and(|allowed| navigation_allowed(&allowed, candidate))
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_page_load(move |_, payload| {
            manager.update(&loading_tab, |read| {
                read.current_url = payload.url().to_string();
                read.loading = payload.event() == PageLoadEvent::Started;
                read.external_origin = !is_loopback(payload.url());
            });
            persist_runtime_tab(&manager, &loading_pool, &loading_tab);
        })
        .on_document_title_changed(move |_, title| {
            title_manager.update(&title_tab, |read| read.title = title);
            persist_runtime_tab(&title_manager, &title_pool, &title_tab);
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
            },
        );
    persist_tab(pool, &read).await?;
    Ok(read)
}

pub(super) fn persist_runtime_tab(manager: &ChatBrowserManager, pool: &SqlitePool, tab_id: &str) {
    let Ok(tab) = manager.read(tab_id) else {
        return;
    };
    let pool = pool.clone();
    tauri::async_runtime::spawn(async move {
        let _ = persist_tab(&pool, &tab.read).await;
    });
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
