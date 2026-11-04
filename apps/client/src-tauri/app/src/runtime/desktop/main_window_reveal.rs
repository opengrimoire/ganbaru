//! Linux reveal of the hidden main window that starts maximized.
//!
//! GTK maps a hidden window at its requested size and the window manager maximizes it afterwards.
//! A WebView shown together with the window paints its first frame at the requested size, so the
//! page fills only part of the maximized window until it relayouts. The WebView therefore stays
//! hidden while the window maps and appears once the window has its maximized size, painting its
//! first frame at the final size over the native background.

use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Longest wait for the window manager to resize the mapped window before the WebView appears
/// anyway, for window managers that keep the requested size.
const MAXIMIZE_SETTLE_LIMIT: Duration = Duration::from_millis(750);

/// Shows `window` with its WebView hidden until the window manager has resized it.
pub(super) fn show_after_initial_maximize<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), String> {
    window
        .with_webview(|platform| {
            let webview = platform.inner().upcast::<gtk::Widget>();
            match webview
                .toplevel()
                .and_then(|toplevel| toplevel.downcast::<gtk::Window>().ok())
            {
                Some(toplevel) => show_webview_after_resize(&toplevel, webview),
                None => {
                    eprintln!(
                        "the main window WebView has no toplevel window; showing it directly"
                    );
                    webview.show_all();
                }
            }
        })
        .map_err(|error| format!("show the main window: {error}"))
}

/// Maps `window` without `webview`, then shows `webview` after the first configure event with a
/// size other than the requested one, or after [`MAXIMIZE_SETTLE_LIMIT`].
fn show_webview_after_resize(window: &gtk::Window, webview: gtk::Widget) {
    let (requested_width, requested_height) = window.size();
    webview.hide();
    // `show_all` on the window, including a second launch raising it meanwhile, skips the WebView.
    webview.set_no_show_all(true);

    let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let reveal: Rc<dyn Fn()> = {
        let window = window.clone();
        let handler = Rc::clone(&handler);
        Rc::new(move || {
            let Some(id) = handler.borrow_mut().take() else {
                return;
            };
            window.disconnect(id);
            webview.set_no_show_all(false);
            webview.show_all();
            webview.grab_focus();
        })
    };

    let on_configure = Rc::clone(&reveal);
    let id = window.connect_configure_event(move |_, event| {
        let (width, height) = event.size();
        let resized = i32::try_from(width).ok() != Some(requested_width)
            || i32::try_from(height).ok() != Some(requested_height);
        if resized {
            // Allocation follows the configure event; show the WebView once it has the new size.
            let reveal = Rc::clone(&on_configure);
            glib::idle_add_local_once(move || reveal());
        }
        false
    });
    *handler.borrow_mut() = Some(id);

    window.show_all();
    glib::timeout_add_local_once(MAXIMIZE_SETTLE_LIMIT, move || reveal());
}
