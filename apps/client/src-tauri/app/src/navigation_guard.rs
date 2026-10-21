//! Keeps app webviews on the bundled app document.
//!
//! Main-frame page loads that leave the app origin are sent back to the last app URL the
//! webview showed. Page load events report only the main frame on every desktop backend, so
//! framed content such as the Music loopback host and its YouTube frames is unaffected.
//! WebView2 also reports navigation requests for the main frame only, so on Windows the same
//! rule refuses the navigation before it starts. WebKitGTK and WKWebView report subframe
//! navigations through the same hook, so there the guard relies on the page load repair.
//! Chat preview webviews own their navigation policy and are exempt.

use crate::chat::preview::PREVIEW_LABEL_PREFIX;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, Runtime, Url, Webview};

/// Plugin name registered with the Tauri builder.
pub const PLUGIN_NAME: &str = "app-navigation-guard";

/// Origins that serve the bundled app on Linux, macOS, and Windows.
const APP_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Whether the backend's navigation hook reports only main-frame navigations.
const NAVIGATION_HOOK_IS_MAIN_FRAME_ONLY: bool = cfg!(windows);

/// Scheme, host, and port of an origin the main frame may show.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AppOrigin {
    scheme: String,
    host: Option<String>,
    port: Option<u16>,
}

impl AppOrigin {
    fn of(url: &Url) -> Self {
        Self {
            scheme: url.scheme().to_owned(),
            host: url.host_str().map(str::to_owned),
            port: url.port_or_known_default(),
        }
    }
}

/// Returns the origins the main frame may show, adding the dev server only in dev builds.
fn allowed_origins(dev_url: Option<&Url>, is_dev: bool) -> Vec<AppOrigin> {
    let mut origins: Vec<AppOrigin> = APP_ORIGINS
        .iter()
        .filter_map(|origin| Url::parse(origin).ok())
        .map(|url| AppOrigin::of(&url))
        .collect();
    if is_dev && let Some(dev_url) = dev_url {
        origins.push(AppOrigin::of(dev_url));
    }
    origins
}

/// Whether `url` is an app document. Opaque `about:blank` is allowed because webviews start
/// there before their first load.
fn is_app_document(origins: &[AppOrigin], url: &Url) -> bool {
    if url.as_str() == "about:blank" {
        return true;
    }
    let origin = AppOrigin::of(url);
    origin.host.is_some() && origins.contains(&origin)
}

/// Whether the guard leaves this webview's navigation to its owner.
fn is_exempt(label: &str) -> bool {
    label.starts_with(PREVIEW_LABEL_PREFIX)
}

/// Last app URL per webview label, used as the recovery target.
#[derive(Default)]
struct LastAppUrls(Mutex<HashMap<String, Url>>);

impl LastAppUrls {
    fn record(&self, label: &str, url: &Url) {
        if url.scheme() == "about" {
            return;
        }
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(label.to_owned(), url.clone());
    }

    fn recovery_target(&self, label: &str, fallback: &Url) -> Url {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(label)
            .cloned()
            .unwrap_or_else(|| fallback.clone())
    }
}

fn webview_origins<R: Runtime>(webview: &Webview<R>) -> Vec<AppOrigin> {
    allowed_origins(webview.config().build.dev_url.as_ref(), tauri::is_dev())
}

/// Default recovery target when a webview never showed an app URL.
fn app_index<R: Runtime>(webview: &Webview<R>) -> Option<Url> {
    if tauri::is_dev()
        && let Some(dev_url) = webview.config().build.dev_url.as_ref()
    {
        return Some(dev_url.clone());
    }
    let base = if cfg!(windows) {
        "http://tauri.localhost/"
    } else {
        "tauri://localhost/"
    };
    Url::parse(base).ok()
}

fn repair_page_load<R: Runtime>(last_urls: &LastAppUrls, webview: &Webview<R>, url: &Url) {
    let label = webview.label();
    if is_exempt(label) {
        return;
    }
    if is_app_document(&webview_origins(webview), url) {
        last_urls.record(label, url);
        return;
    }
    let Some(fallback) = app_index(webview) else {
        eprintln!("[navigation-guard] no app URL to restore webview {label}");
        return;
    };
    let target = last_urls.recovery_target(label, &fallback);
    eprintln!("[navigation-guard] restoring webview {label} after it left the app origin");
    let webview = webview.clone();
    // The page load hook runs inside the webview's load signal; navigating from a task keeps
    // the recovery out of that signal.
    tauri::async_runtime::spawn(async move {
        if let Err(error) = webview.navigate(target) {
            eprintln!(
                "[navigation-guard] failed to restore webview {}: {error}",
                webview.label()
            );
        }
    });
}

/// Builds the plugin that keeps app webviews on the app document.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let last_urls = Arc::new(LastAppUrls::default());
    Builder::new(PLUGIN_NAME)
        .on_navigation(|webview, url| {
            !NAVIGATION_HOOK_IS_MAIN_FRAME_ONLY
                || is_exempt(webview.label())
                || is_app_document(&webview_origins(webview), url)
        })
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                repair_page_load(&last_urls, webview, payload.url());
            } else {
                let url = payload.url();
                if !is_exempt(webview.label()) && is_app_document(&webview_origins(webview), url) {
                    last_urls.record(webview.label(), url);
                }
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> Url {
        Url::parse(value).expect("test URL parses")
    }

    #[test]
    fn app_documents_are_limited_to_the_app_origin_and_blank_pages() {
        let origins = allowed_origins(None, false);
        for allowed in [
            "tauri://localhost/index.html?ganbaruWindow=pomodoroOverlay",
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html",
            "about:blank",
        ] {
            assert!(is_app_document(&origins, &url(allowed)), "{allowed}");
        }
        for denied in [
            "http://127.0.0.1:54321/",
            "http://localhost:1420/",
            "https://www.youtube-nocookie.com/embed/video",
            "https://www.youtube.com/",
            "https://evil.example/",
            "http://tauri.localhost.evil.example/",
            "tauri://evil/",
            "file:///etc/passwd",
            "javascript:1",
            "data:text/html,hi",
            "blob:tauri://localhost/id",
            "about:srcdoc",
        ] {
            assert!(!is_app_document(&origins, &url(denied)), "{denied}");
        }
    }

    #[test]
    fn dev_server_is_an_app_origin_only_in_dev_builds() {
        let dev_url = url("http://localhost:1420/__ganbaru_dev_ready");
        let release = allowed_origins(Some(&dev_url), false);
        let dev = allowed_origins(Some(&dev_url), true);
        let page = url("http://localhost:1420/index.html?ganbaruWindow=tab");
        assert!(!is_app_document(&release, &page));
        assert!(is_app_document(&dev, &page));
        assert!(!is_app_document(&dev, &url("http://localhost:1421/")));
        assert!(!is_app_document(&dev, &url("https://localhost:1420/")));
    }

    #[test]
    fn only_chat_preview_webviews_are_exempt() {
        assert!(is_exempt(&format!("{PREVIEW_LABEL_PREFIX}abc")));
        for label in ["main", "pomodoro-overlay", "tab-1", "chat-previewer"] {
            assert!(!is_exempt(label), "{label}");
        }
    }

    #[test]
    fn recovery_returns_the_last_app_url_of_the_same_webview() {
        let last_urls = LastAppUrls::default();
        let fallback = url("tauri://localhost/");
        assert_eq!(last_urls.recovery_target("main", &fallback), fallback);

        let overlay = url("tauri://localhost/index.html?ganbaruWindow=pomodoroOverlay");
        last_urls.record("overlay", &overlay);
        last_urls.record("overlay", &url("about:blank"));
        assert_eq!(last_urls.recovery_target("overlay", &fallback), overlay);
        assert_eq!(last_urls.recovery_target("main", &fallback), fallback);
    }
}
