//! Which app window comes forward when the app is raised from outside, such as on a second launch.
//!
//! Every tab window is equal to the user. The host window, labelled `main`, also runs app-wide work and
//! hides instead of closing while other windows stay open, so a raise prefers a window already on screen.

use tauri::Manager;

const HOST_WINDOW_LABEL: &str = "main";
const TAB_WINDOW_LABEL_PREFIX: &str = "view-";

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
}

#[cfg(test)]
mod tests {
    use super::window_to_raise;

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
