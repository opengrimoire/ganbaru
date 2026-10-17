//! Isolated child-webview preview state and bounded browser controls.

use crate::db;
use crate::vault;
use base64::{Engine as _, engine::general_purpose};
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::repository::resources::{
    self, ChatResourceKind, ChatResourceRead, StoreBrowserArtifact,
};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, WebviewUrl};

mod artifacts;
mod capture;
mod navigation;
mod repository;
mod webview;

use artifacts::{persist_browser_artifact, persist_browser_artifact_with_pool};
use capture::{capture_browser_png, capture_recording_frames, recording_archive};
#[cfg(test)]
use navigation::navigation_allowed;
use navigation::{
    is_loopback, js_string, loopback_urls, validate_bounds, validate_navigation, validate_tab_id,
};
use repository::{persist_tab, require_thread};
pub(crate) use webview::evaluate_script;
use webview::{
    create_child_browser_webview, navigate_existing, owned_tab, resize_existing, run_script,
};

const MAX_BROWSER_URL_BYTES: usize = 8_192;
const MAX_BROWSER_RESULT_BYTES: usize = 2 * 1024 * 1024;
const BROWSER_EVALUATION_TIMEOUT: Duration = Duration::from_secs(15);
const BROWSER_CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);
const RECORDING_FRAME_INTERVAL: Duration = Duration::from_millis(500);
const MAX_RECORDING_DURATION: Duration = Duration::from_secs(15);
const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECORDING_BYTES: usize = 192 * 1024 * 1024;
static NEXT_ARTIFACT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenBrowserTabRequest {
    pub thread_id: ChatThreadId,
    pub tab_id: String,
    pub url: String,
    pub bounds: BrowserTabBounds,
    pub external_navigation_confirmed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabRead {
    pub thread_id: ChatThreadId,
    pub tab_id: String,
    pub current_url: String,
    pub title: String,
    pub visible: bool,
    pub loading: bool,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub external_origin: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPreviewServer {
    pub url: String,
    pub source_label: String,
}

#[derive(Clone, Debug)]
struct RuntimeBrowserTab {
    read: BrowserTabRead,
    webview_label: String,
    allowed_url: Arc<Mutex<reqwest::Url>>,
}

struct ActiveRecording {
    thread_id: ChatThreadId,
    tab_id: String,
    started: Instant,
    stop: tokio::sync::oneshot::Sender<()>,
    completed: tokio::sync::oneshot::Receiver<ChatResult<Vec<Vec<u8>>>>,
}

struct BrowserArtifactPayload<'a> {
    kind: ChatResourceKind,
    display_name: &'a str,
    mime_type: &'a str,
    duration_milliseconds: Option<u64>,
    frame_count: Option<u64>,
    bytes: &'a [u8],
}

#[derive(Clone, Default)]
pub struct ChatBrowserManager {
    tabs: Arc<Mutex<HashMap<String, RuntimeBrowserTab>>>,
    recording: Arc<Mutex<Option<ActiveRecording>>>,
}

impl ChatBrowserManager {
    fn cancel_recording(&self, matches: impl FnOnce(&ActiveRecording) -> bool) {
        if let Ok(mut active) = self.recording.lock() {
            if active.as_ref().is_some_and(matches) {
                if let Some(recording) = active.take() {
                    let _ = recording.stop.send(());
                }
            }
        }
    }

    fn read(&self, tab_id: &str) -> ChatResult<RuntimeBrowserTab> {
        self.tabs
            .lock()
            .map_err(|_| browser_state_error())?
            .get(tab_id)
            .cloned()
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Browser preview tab was not found",
                    true,
                )
            })
    }

    fn update(&self, tab_id: &str, update: impl FnOnce(&mut BrowserTabRead)) {
        if let Ok(mut tabs) = self.tabs.lock() {
            if let Some(tab) = tabs.get_mut(tab_id) {
                update(&mut tab.read);
            }
        }
    }

    pub(crate) fn thread_reads(&self, thread_id: &ChatThreadId) -> ChatResult<Vec<BrowserTabRead>> {
        let mut reads = self
            .tabs
            .lock()
            .map_err(|_| browser_state_error())?
            .values()
            .filter(|tab| &tab.read.thread_id == thread_id)
            .map(|tab| tab.read.clone())
            .collect::<Vec<_>>();
        reads.sort_by(|left, right| left.tab_id.cmp(&right.tab_id));
        Ok(reads)
    }

    fn active_thread_tab(&self, thread_id: &ChatThreadId) -> ChatResult<RuntimeBrowserTab> {
        let tabs = self.tabs.lock().map_err(|_| browser_state_error())?;
        tabs.values()
            .find(|tab| &tab.read.thread_id == thread_id && tab.read.visible)
            .or_else(|| tabs.values().find(|tab| &tab.read.thread_id == thread_id))
            .cloned()
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Open the Browser panel before using preview tools",
                    true,
                )
            })
    }

    pub fn close_thread<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        thread_id: &ChatThreadId,
    ) {
        self.cancel_recording(|recording| &recording.thread_id == thread_id);
        let labels = if let Ok(mut tabs) = self.tabs.lock() {
            let ids = tabs
                .iter()
                .filter_map(|(id, tab)| (&tab.read.thread_id == thread_id).then_some(id.clone()))
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| tabs.remove(&id).map(|tab| tab.webview_label))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for label in labels {
            if let Some(webview) = app.get_webview(&label) {
                let _ = webview.close();
            }
        }
    }

    pub fn close_all<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) {
        self.cancel_recording(|_| true);
        let labels = if let Ok(mut tabs) = self.tabs.lock() {
            std::mem::take(&mut *tabs)
                .into_values()
                .map(|tab| tab.webview_label)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for label in labels {
            if let Some(webview) = app.get_webview(&label) {
                let _ = webview.close();
            }
        }
    }
}

pub(crate) async fn mcp_navigate_browser(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    url: &str,
) -> ChatResult<BrowserTabRead> {
    let url = validate_navigation(url, false)?;
    let tab = app
        .state::<ChatBrowserManager>()
        .active_thread_tab(thread_id)?;
    navigate_existing(app, &tab, url.clone())?;
    app.state::<ChatBrowserManager>()
        .update(&tab.read.tab_id, |read| {
            read.current_url = url.to_string();
            read.loading = true;
            read.external_origin = false;
        });
    let read = app
        .state::<ChatBrowserManager>()
        .read(&tab.read.tab_id)?
        .read;
    persist_tab(pool, &read).await?;
    Ok(read)
}

pub(crate) async fn mcp_resize_browser(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    width: u32,
    height: u32,
) -> ChatResult<BrowserTabRead> {
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return Err(ChatError::validation(
            "viewport",
            "Browser viewport is invalid",
        ));
    }
    let tab = app
        .state::<ChatBrowserManager>()
        .active_thread_tab(thread_id)?;
    app.get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?
        .set_size(tauri::PhysicalSize::new(width, height))
        .map_err(|_| browser_unavailable_error())?;
    app.state::<ChatBrowserManager>()
        .update(&tab.read.tab_id, |read| {
            read.viewport_width = width;
            read.viewport_height = height;
        });
    let read = app
        .state::<ChatBrowserManager>()
        .read(&tab.read.tab_id)?
        .read;
    persist_tab(pool, &read).await?;
    Ok(read)
}

pub(crate) fn mcp_active_tab_id(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
) -> ChatResult<String> {
    Ok(app
        .state::<ChatBrowserManager>()
        .active_thread_tab(thread_id)?
        .read
        .tab_id)
}

pub(crate) async fn mcp_screenshot_browser(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    vault_root: &std::path::Path,
    thread_id: &ChatThreadId,
) -> ChatResult<ChatResourceRead> {
    let tab = app
        .state::<ChatBrowserManager>()
        .active_thread_tab(thread_id)?;
    let bytes = capture_browser_png(app, &tab).await?;
    persist_browser_artifact_with_pool(
        pool,
        vault_root,
        &tab,
        BrowserArtifactPayload {
            kind: ChatResourceKind::BrowserScreenshot,
            display_name: "Browser screenshot",
            mime_type: "image/png",
            duration_milliseconds: None,
            frame_count: Some(1),
            bytes: &bytes,
        },
    )
    .await
}

#[tauri::command]
pub async fn chat_browser_status(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<Vec<BrowserTabRead>> {
    let mut reads = app
        .state::<ChatBrowserManager>()
        .tabs
        .lock()
        .map_err(|_| browser_state_error())?
        .values()
        .filter(|tab| tab.read.thread_id == thread_id)
        .map(|tab| tab.read.clone())
        .collect::<Vec<_>>();
    let pool = chat_pool(&app, db_url).await?;
    let stored = sqlx::query_as::<_, (String, String, String, Option<i64>, Option<i64>)>(
        "SELECT id, current_url, title, viewport_width, viewport_height
         FROM chat_preview_tabs WHERE thread_id = ? ORDER BY position, id",
    )
    .bind(thread_id.as_str())
    .fetch_all(&pool)
    .await
    .map_err(|_| persistence_error())?;
    for (tab_id, current_url, title, width, height) in stored {
        if reads.iter().any(|read| read.tab_id == tab_id) {
            continue;
        }
        let external_origin = reqwest::Url::parse(&current_url)
            .ok()
            .is_some_and(|url| !is_loopback(&url));
        reads.push(BrowserTabRead {
            thread_id: thread_id.clone(),
            tab_id,
            current_url,
            title,
            visible: false,
            loading: false,
            viewport_width: u32::try_from(width.unwrap_or(800)).unwrap_or(800),
            viewport_height: u32::try_from(height.unwrap_or(600)).unwrap_or(600),
            external_origin,
        });
    }
    reads.sort_by(|left, right| left.tab_id.cmp(&right.tab_id));
    Ok(reads)
}

#[tauri::command]
pub async fn chat_browser_discover_servers(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<Vec<DiscoveredPreviewServer>> {
    let pool = chat_pool(&app, db_url).await?;
    let working_folder_id =
        sqlx::query_scalar::<_, String>("SELECT working_folder_id FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_optional(&pool)
            .await
            .map_err(|_| persistence_error())?
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
            })?;
    let working_folder_id =
        ProjectWorkingFolderId::new(working_folder_id).map_err(|_| persistence_error())?;
    let registry = app.state::<super::terminal::ChatTerminalRegistry>();
    let terminals = registry.list(&thread_id, &working_folder_id)?;
    let mut candidates = HashMap::<String, String>::new();
    for terminal in terminals {
        let snapshot = registry.snapshot(&terminal.id)?;
        for chunk in snapshot.scrollback {
            let Ok(bytes) = general_purpose::STANDARD.decode(chunk.data_base64) else {
                continue;
            };
            for candidate in loopback_urls(&bytes) {
                candidates
                    .entry(candidate)
                    .or_insert_with(|| terminal.name.clone());
            }
        }
    }
    let mut discovered = Vec::new();
    for (url, source_label) in candidates.into_iter().take(64) {
        let Ok(parsed) = reqwest::Url::parse(&url) else {
            continue;
        };
        let Some(port) = parsed.port_or_known_default() else {
            continue;
        };
        let address = format!("127.0.0.1:{port}");
        if tokio::time::timeout(
            Duration::from_millis(250),
            tokio::net::TcpStream::connect(address),
        )
        .await
        .is_ok_and(|result| result.is_ok())
        {
            discovered.push(DiscoveredPreviewServer { url, source_label });
        }
    }
    discovered.sort_by(|left, right| left.url.cmp(&right.url));
    discovered.dedup_by(|left, right| left.url == right.url);
    Ok(discovered)
}

#[tauri::command]
pub async fn chat_browser_open(
    app: tauri::AppHandle,
    db_url: String,
    request: OpenBrowserTabRequest,
) -> ChatResult<BrowserTabRead> {
    let url = validate_navigation(&request.url, request.external_navigation_confirmed)?;
    let bounds = validate_bounds(&request.bounds)?;
    validate_tab_id(&request.tab_id)?;
    let pool = chat_pool(&app, db_url).await?;
    require_thread(&pool, &request.thread_id).await?;
    if let Ok(existing) = app.state::<ChatBrowserManager>().read(&request.tab_id) {
        if existing.read.thread_id != request.thread_id {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Browser preview tab belongs to another thread",
                true,
            ));
        }
        navigate_existing(&app, &existing, url.clone())?;
        resize_existing(&app, &existing, &bounds)?;
        if let Some(webview) = app.get_webview(&existing.webview_label) {
            webview.show().map_err(|_| browser_unavailable_error())?;
        }
        app.state::<ChatBrowserManager>()
            .update(&request.tab_id, |read| {
                read.current_url = url.to_string();
                read.visible = true;
                read.loading = true;
                read.viewport_width = bounds.width.round() as u32;
                read.viewport_height = bounds.height.round() as u32;
            });
        persist_tab(
            &pool,
            &app.state::<ChatBrowserManager>()
                .read(&request.tab_id)?
                .read,
        )
        .await?;
        return Ok(app
            .state::<ChatBrowserManager>()
            .read(&request.tab_id)?
            .read);
    }
    create_child_browser_webview(&app, &pool, &request, url, bounds).await
}

#[tauri::command]
pub async fn chat_browser_navigate(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    tab_id: String,
    url: String,
    external_navigation_confirmed: bool,
) -> ChatResult<BrowserTabRead> {
    let url = validate_navigation(&url, external_navigation_confirmed)?;
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    navigate_existing(&app, &tab, url.clone())?;
    app.state::<ChatBrowserManager>().update(&tab_id, |read| {
        read.current_url = url.to_string();
        read.loading = true;
        read.external_origin = !is_loopback(&url);
    });
    let read = app.state::<ChatBrowserManager>().read(&tab_id)?.read;
    persist_tab(&chat_pool(&app, db_url).await?, &read).await?;
    Ok(read)
}

#[tauri::command]
pub async fn chat_browser_resize(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    tab_id: String,
    bounds: BrowserTabBounds,
) -> ChatResult<BrowserTabRead> {
    let bounds = validate_bounds(&bounds)?;
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    resize_existing(&app, &tab, &bounds)?;
    app.state::<ChatBrowserManager>().update(&tab_id, |read| {
        read.viewport_width = bounds.width.round() as u32;
        read.viewport_height = bounds.height.round() as u32;
    });
    let read = app.state::<ChatBrowserManager>().read(&tab_id)?.read;
    persist_tab(&chat_pool(&app, db_url).await?, &read).await?;
    Ok(read)
}

#[tauri::command]
pub async fn chat_browser_set_visible(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    tab_id: String,
    visible: bool,
) -> ChatResult<BrowserTabRead> {
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    let webview = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?;
    if visible {
        webview.show()
    } else {
        webview.hide()
    }
    .map_err(|_| browser_unavailable_error())?;
    app.state::<ChatBrowserManager>()
        .update(&tab_id, |read| read.visible = visible);
    let read = app.state::<ChatBrowserManager>().read(&tab_id)?.read;
    persist_tab(&chat_pool(&app, db_url).await?, &read).await?;
    Ok(read)
}

#[tauri::command]
pub async fn chat_browser_back(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<()> {
    run_script(&app, &thread_id, &tab_id, "history.back()")
}

#[tauri::command]
pub async fn chat_browser_forward(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<()> {
    run_script(&app, &thread_id, &tab_id, "history.forward()")
}

#[tauri::command]
pub async fn chat_browser_refresh(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<()> {
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    app.get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?
        .reload()
        .map_err(|_| browser_unavailable_error())
}

#[tauri::command]
pub async fn chat_browser_snapshot(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<String> {
    evaluate_script(
        &app,
        &thread_id,
        &tab_id,
        "JSON.stringify({url:location.href,title:document.title,text:(document.body?.innerText??'').slice(0,1000000),html:(document.documentElement?.outerHTML??'').slice(0,1000000)})",
    )
    .await
}

#[tauri::command]
pub async fn chat_browser_screenshot(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<ChatResourceRead> {
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    let bytes = capture_browser_png(&app, &tab).await?;
    persist_browser_artifact(
        &app,
        db_url,
        &tab,
        BrowserArtifactPayload {
            kind: ChatResourceKind::BrowserScreenshot,
            display_name: "Browser screenshot",
            mime_type: "image/png",
            duration_milliseconds: None,
            frame_count: Some(1),
            bytes: &bytes,
        },
    )
    .await
}

#[tauri::command]
pub async fn chat_browser_recording_start(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
    approved: bool,
) -> ChatResult<()> {
    if !approved {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Browser recording requires approval",
            true,
        ));
    }
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    let manager = app.state::<ChatBrowserManager>();
    let mut active = manager
        .recording
        .lock()
        .map_err(|_| browser_state_error())?;
    if active.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Another browser recording is already active",
            true,
        ));
    }
    let (stop, stop_receiver) = tokio::sync::oneshot::channel();
    let (completed_sender, completed) = tokio::sync::oneshot::channel();
    let recording_app = app.clone();
    let recording_tab = tab.clone();
    tauri::async_runtime::spawn(async move {
        let result = capture_recording_frames(recording_app, recording_tab, stop_receiver).await;
        let _ = completed_sender.send(result);
    });
    *active = Some(ActiveRecording {
        thread_id,
        tab_id,
        started: Instant::now(),
        stop,
        completed,
    });
    Ok(())
}

#[tauri::command]
pub async fn chat_browser_recording_stop(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<ChatResourceRead> {
    let recording = app
        .state::<ChatBrowserManager>()
        .recording
        .lock()
        .map_err(|_| browser_state_error())?
        .take()
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "No browser recording is active",
                true,
            )
        })?;
    if recording.thread_id != thread_id || recording.tab_id != tab_id {
        app.state::<ChatBrowserManager>()
            .recording
            .lock()
            .map_err(|_| browser_state_error())?
            .replace(recording);
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Browser recording belongs to another preview tab",
            true,
        ));
    }
    let duration = recording.started.elapsed().min(MAX_RECORDING_DURATION);
    let _ = recording.stop.send(());
    let frames = tokio::time::timeout(BROWSER_CAPTURE_TIMEOUT, recording.completed)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Timeout, "Browser recording timed out", true))?
        .map_err(|_| browser_unavailable_error())??;
    if frames.is_empty() {
        return Err(browser_unavailable_error());
    }
    let archive = recording_archive(&frames, duration)?;
    let tab = owned_tab(&app, &thread_id, &tab_id)?;
    persist_browser_artifact(
        &app,
        db_url,
        &tab,
        BrowserArtifactPayload {
            kind: ChatResourceKind::BrowserRecording,
            display_name: "Browser recording",
            mime_type: "application/zip",
            duration_milliseconds: Some(duration.as_millis() as u64),
            frame_count: Some(frames.len() as u64),
            bytes: &archive,
        },
    )
    .await
}

#[tauri::command]
pub async fn chat_browser_click(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
    selector: String,
) -> ChatResult<()> {
    let selector = js_string(&selector, "selector")?;
    run_script(
        &app,
        &thread_id,
        &tab_id,
        &format!("document.querySelector({selector})?.click()"),
    )
}

#[tauri::command]
pub async fn chat_browser_type(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
    selector: String,
    text: String,
) -> ChatResult<()> {
    let selector = js_string(&selector, "selector")?;
    let text = js_string(&text, "text")?;
    run_script(
        &app,
        &thread_id,
        &tab_id,
        &format!(
            "(()=>{{const e=document.querySelector({selector});if(e){{e.focus();e.value={text};e.dispatchEvent(new Event('input',{{bubbles:true}}));}}}})()"
        ),
    )
}

#[tauri::command]
pub async fn chat_browser_press(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
    key: String,
) -> ChatResult<()> {
    let key = js_string(&key, "key")?;
    run_script(
        &app,
        &thread_id,
        &tab_id,
        &format!(
            "document.activeElement?.dispatchEvent(new KeyboardEvent('keydown',{{key:{key},bubbles:true}}))"
        ),
    )
}

#[tauri::command]
pub async fn chat_browser_scroll(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
    x: f64,
    y: f64,
) -> ChatResult<()> {
    if !x.is_finite() || !y.is_finite() || x.abs() > 1_000_000.0 || y.abs() > 1_000_000.0 {
        return Err(ChatError::validation(
            "scroll",
            "Browser scroll distance is invalid",
        ));
    }
    run_script(&app, &thread_id, &tab_id, &format!("scrollBy({x},{y})"))
}

#[tauri::command]
pub async fn chat_browser_close(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    tab_id: String,
) -> ChatResult<()> {
    let pool = chat_pool(&app, db_url).await?;
    require_thread(&pool, &thread_id).await?;
    if let Ok(tab) = app.state::<ChatBrowserManager>().read(&tab_id) {
        if tab.read.thread_id != thread_id {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Browser preview tab belongs to another thread",
                true,
            ));
        }
        app.state::<ChatBrowserManager>()
            .cancel_recording(|recording| recording.tab_id == tab_id);
        if let Some(webview) = app.get_webview(&tab.webview_label) {
            webview.close().map_err(|_| browser_unavailable_error())?;
        }
        app.state::<ChatBrowserManager>()
            .tabs
            .lock()
            .map_err(|_| browser_state_error())?
            .remove(&tab_id);
    }
    let deleted = sqlx::query("DELETE FROM chat_preview_tabs WHERE id = ? AND thread_id = ?")
        .bind(&tab_id)
        .bind(thread_id.as_str())
        .execute(&pool)
        .await
        .map_err(|_| persistence_error())?;
    if deleted.rows_affected() == 0 {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Browser preview tab was not found",
            true,
        ));
    }
    Ok(())
}

#[cfg(desktop)]
async fn chat_pool(app: &tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app.clone(), db_url)
        .await
        .map_err(|_| persistence_error())
}

fn browser_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "Browser preview is unavailable",
        true,
    )
}

fn browser_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Browser preview state is unavailable",
        true,
    )
}

fn persistence_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Browser preview state could not be persisted",
        true,
    )
}

#[cfg(test)]
mod tests;
