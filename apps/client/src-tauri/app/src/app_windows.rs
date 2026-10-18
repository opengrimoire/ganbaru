//! Which app window comes forward when the app is raised from outside, such as on a second launch.
//!
//! Every tab window is equal to the user. The host window, labelled `main`, also runs app-wide work and
//! hides instead of closing while other windows stay open, so a raise prefers a window already on screen.

use serde::Deserialize;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const HOST_WINDOW_LABEL: &str = "main";
const TAB_WINDOW_LABEL_PREFIX: &str = "view-";
/// Longest label suffix after [`TAB_WINDOW_LABEL_PREFIX`], matching the frontend label pattern.
const TAB_WINDOW_LABEL_MAX_SUFFIX: usize = 64;
/// Longest tab window address accepted. A tab carries its location and title, far below this.
const TAB_WINDOW_ADDRESS_MAX_BYTES: usize = 64 * 1024;
/// Query keys a tab window address may carry: the first tab's kind and the tab itself.
const TAB_WINDOW_QUERY_KEYS: &[&str] = &["view", "tab"];
/// Largest absolute logical coordinate accepted for a new window's position.
const TAB_WINDOW_POSITION_LIMIT: f64 = 1_000_000.0;
const TAB_WINDOW_WIDTH: f64 = 960.0;
const TAB_WINDOW_HEIGHT: f64 = 720.0;
const TAB_WINDOW_MIN_WIDTH: f64 = 280.0;
const TAB_WINDOW_MIN_HEIGHT: f64 = 180.0;
/// Base the address is resolved against, so only an app-relative address keeps this origin.
const TAB_WINDOW_ADDRESS_BASE: &str = "http://localhost/";
/// Tells a window, named by the label in the payload, that the app brought it on screen. A hidden host
/// window resumes saving its tabs on it, without waiting for focus the window manager may deny.
const WINDOW_RAISED_EVENT: &str = "app-window-raised";

/// Label of the window to raise among `(label, visible)` pairs: the host window when it is on screen, else
/// the first tab window on screen in label order, else the host window, which then shows again.
fn window_to_raise<'a>(windows: impl IntoIterator<Item = (&'a str, bool)>) -> &'a str {
    let mut visible_tab_windows: Vec<&str> = Vec::new();
    for (label, visible) in windows {
        if !visible {
            continue;
        }
        if label == HOST_WINDOW_LABEL {
            return label;
        }
        if label.starts_with(TAB_WINDOW_LABEL_PREFIX) {
            visible_tab_windows.push(label);
        }
    }
    visible_tab_windows
        .into_iter()
        .min()
        .unwrap_or(HOST_WINDOW_LABEL)
}

/// Tell the window labelled `label` that the app brought it on screen.
pub(crate) fn announce_window_raised(app: &tauri::AppHandle, label: &str) {
    if let Err(error) = app.emit(WINDOW_RAISED_EVENT, label) {
        eprintln!("failed to announce raised window {label}: {error}");
    }
}

/// Show, restore, and focus the app window the user most likely expects, per [`window_to_raise`].
pub(crate) fn raise_app_window(app: &tauri::AppHandle) {
    let windows = app.webview_windows();
    let label = window_to_raise(
        windows
            .iter()
            .map(|(label, window)| (label.as_str(), window.is_visible().unwrap_or(false))),
    );
    let Some(window) = windows.get(label) else {
        return;
    };
    if let Err(error) = window.show() {
        eprintln!("failed to show window {label}: {error}");
    }
    if let Err(error) = window.unminimize() {
        eprintln!("failed to restore window {label}: {error}");
    }
    if let Err(error) = window.set_focus() {
        eprintln!("failed to focus window {label}: {error}");
    }
    announce_window_raised(app, label);
}

/// Logical screen point where a new tab window opens.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TabWindowPosition {
    x: f64,
    y: f64,
}

/// A request from the webview to open, or bring forward, a tab window.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTabWindowRequest {
    label: String,
    /// App-relative address such as `/?view=notes&tab=...`.
    address: String,
    focus: bool,
    position: Option<TabWindowPosition>,
    /// Native background shown before the page paints, as red, green, and blue.
    background: Option<[u8; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabWindowRequestError {
    InvalidLabel,
    AddressTooLong,
    AddressNotAppLocal,
    UnexpectedQueryKey,
    InvalidPosition,
}

impl std::fmt::Display for TabWindowRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidLabel => "tab window label is invalid",
            Self::AddressTooLong => "tab window address is too long",
            Self::AddressNotAppLocal => "tab window address must stay in the app",
            Self::UnexpectedQueryKey => "tab window address carries an unexpected parameter",
            Self::InvalidPosition => "tab window position is invalid",
        })
    }
}

fn validate_tab_window_label(label: &str) -> Result<(), TabWindowRequestError> {
    let suffix = label
        .strip_prefix(TAB_WINDOW_LABEL_PREFIX)
        .ok_or(TabWindowRequestError::InvalidLabel)?;
    let valid = !suffix.is_empty()
        && suffix.len() <= TAB_WINDOW_LABEL_MAX_SUFFIX
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(TabWindowRequestError::InvalidLabel)
    }
}

/// The app page a tab window loads: the app root with at most the tab window query keys, never another
/// origin, path, or fragment.
fn tab_window_url(address: &str) -> Result<WebviewUrl, TabWindowRequestError> {
    if address.len() > TAB_WINDOW_ADDRESS_MAX_BYTES {
        return Err(TabWindowRequestError::AddressTooLong);
    }
    let base =
        tauri::Url::parse(TAB_WINDOW_ADDRESS_BASE).expect("constant tab window base is valid");
    let url = base
        .join(address)
        .map_err(|_| TabWindowRequestError::AddressNotAppLocal)?;
    if url.origin() != base.origin()
        || url.path() != "/"
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(TabWindowRequestError::AddressNotAppLocal);
    }
    let mut seen: Vec<String> = Vec::new();
    for (key, _) in url.query_pairs() {
        if !TAB_WINDOW_QUERY_KEYS.contains(&key.as_ref()) || seen.iter().any(|known| *known == key)
        {
            return Err(TabWindowRequestError::UnexpectedQueryKey);
        }
        seen.push(key.into_owned());
    }
    let page = match url.query() {
        Some(query) if !query.is_empty() => format!("index.html?{query}"),
        _ => "index.html".to_string(),
    };
    Ok(WebviewUrl::App(page.into()))
}

fn validate_tab_window_position(
    position: Option<TabWindowPosition>,
) -> Result<Option<TabWindowPosition>, TabWindowRequestError> {
    match position {
        Some(point)
            if !point.x.is_finite()
                || !point.y.is_finite()
                || point.x.abs() > TAB_WINDOW_POSITION_LIMIT
                || point.y.abs() > TAB_WINDOW_POSITION_LIMIT =>
        {
            Err(TabWindowRequestError::InvalidPosition)
        }
        position => Ok(position),
    }
}

/// Open a tab window, or bring an open one forward when `focus` is set. Rust builds the window so no
/// webview holds the authority to create windows or choose what they load.
#[tauri::command]
pub async fn app_window_open_tab(
    app: tauri::AppHandle,
    request: OpenTabWindowRequest,
) -> Result<(), String> {
    validate_tab_window_label(&request.label).map_err(|error| error.to_string())?;
    let url = tab_window_url(&request.address).map_err(|error| error.to_string())?;
    let position =
        validate_tab_window_position(request.position).map_err(|error| error.to_string())?;

    if let Some(existing) = app.get_webview_window(&request.label) {
        if request.focus {
            existing.show().map_err(|error| error.to_string())?;
            existing.unminimize().map_err(|error| error.to_string())?;
            existing.set_focus().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    let title = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == HOST_WINDOW_LABEL)
        .map_or_else(
            || app.package_info().name.clone(),
            |window| window.title.clone(),
        );
    let mut builder = WebviewWindowBuilder::new(&app, &request.label, url)
        .title(title)
        .inner_size(TAB_WINDOW_WIDTH, TAB_WINDOW_HEIGHT)
        .min_inner_size(TAB_WINDOW_MIN_WIDTH, TAB_WINDOW_MIN_HEIGHT)
        .decorations(false)
        .visible(true)
        .focused(request.focus);
    builder = match position {
        Some(point) => builder.position(point.x, point.y),
        None => builder.center(),
    };
    if let Some([red, green, blue]) = request.background {
        builder = builder.background_color(tauri::window::Color(red, green, blue, u8::MAX));
    }
    builder
        .build()
        .map(|_| ())
        .map_err(|error| format!("create tab window {}: {error}", request.label))
}

#[cfg(test)]
mod tests {
    use super::{
        TabWindowPosition, TabWindowRequestError, tab_window_url, validate_tab_window_label,
        validate_tab_window_position, window_to_raise,
    };
    use tauri::WebviewUrl;

    fn page(address: &str) -> Result<String, TabWindowRequestError> {
        tab_window_url(address).map(|url| match url {
            WebviewUrl::App(path) => path.to_string_lossy().into_owned(),
            other => panic!("expected an app URL, got {other:?}"),
        })
    }

    #[test]
    fn tab_window_labels_follow_the_frontend_pattern() {
        assert!(validate_tab_window_label("view-0f1e2d3c-aaaa").is_ok());
        for label in [
            "main",
            "view-",
            "view-a/b",
            "pomodoro-overlay-1",
            "chat-preview-1",
        ] {
            assert_eq!(
                validate_tab_window_label(label),
                Err(TabWindowRequestError::InvalidLabel),
                "{label}"
            );
        }
        let long = format!("view-{}", "a".repeat(65));
        assert_eq!(
            validate_tab_window_label(&long),
            Err(TabWindowRequestError::InvalidLabel)
        );
    }

    #[test]
    fn tab_window_addresses_load_only_the_app_root() {
        assert_eq!(page("/").as_deref(), Ok("index.html"));
        assert_eq!(
            page("/?view=notes&tab=%7B%22id%22%3A1%7D").as_deref(),
            Ok("index.html?view=notes&tab=%7B%22id%22%3A1%7D")
        );
        for address in [
            "https://example.com/",
            "//example.com/?view=notes",
            "http://localhost:1420/",
            "/other",
            "/#fragment",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                page(address),
                Err(TabWindowRequestError::AddressNotAppLocal),
                "{address}"
            );
        }
        assert_eq!(
            page("/?ganbaruWindow=pomodoroOverlay"),
            Err(TabWindowRequestError::UnexpectedQueryKey)
        );
        assert_eq!(
            page("/?view=a&view=b"),
            Err(TabWindowRequestError::UnexpectedQueryKey)
        );
        let long = format!("/?tab={}", "a".repeat(super::TAB_WINDOW_ADDRESS_MAX_BYTES));
        assert_eq!(page(&long), Err(TabWindowRequestError::AddressTooLong));
    }

    #[test]
    fn tab_window_positions_must_be_finite_and_bounded() {
        assert!(validate_tab_window_position(None).is_ok());
        assert!(
            validate_tab_window_position(Some(TabWindowPosition {
                x: -40.0,
                y: 1200.0
            }))
            .is_ok()
        );
        for (x, y) in [(f64::NAN, 0.0), (0.0, f64::INFINITY), (2_000_000.0, 0.0)] {
            assert_eq!(
                validate_tab_window_position(Some(TabWindowPosition { x, y })).map(|_| ()),
                Err(TabWindowRequestError::InvalidPosition)
            );
        }
    }

    #[test]
    fn raises_the_host_window_while_it_is_on_screen() {
        assert_eq!(window_to_raise([("view-b", true), ("main", true)]), "main");
    }

    #[test]
    fn raises_a_tab_window_on_screen_while_the_host_is_hidden() {
        assert_eq!(
            window_to_raise([("view-b", true), ("main", false), ("view-a", true)]),
            "view-a"
        );
    }

    #[test]
    fn shows_the_host_again_when_no_window_is_on_screen() {
        assert_eq!(
            window_to_raise([("main", false), ("view-a", false)]),
            "main"
        );
    }

    #[test]
    fn ignores_windows_that_hold_no_tabs() {
        assert_eq!(
            window_to_raise([("main", false), ("pomodoro-overlay-1", true)]),
            "main"
        );
    }
}
