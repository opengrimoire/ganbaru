//! WebKitGTK permission and script dialog signals, through the safe webkit2gtk bindings.

use super::*;
use webkit2gtk::glib::object::{Cast, ObjectExt};
use webkit2gtk::{
    DeviceInfoPermissionRequest, GeolocationPermissionRequest, MediaKeySystemPermissionRequest,
    NotificationPermissionRequest, PermissionRequest, PermissionRequestExt,
    PointerLockPermissionRequest, ScriptDialog, ScriptDialogType, UserMediaPermissionRequest,
    UserMediaPermissionRequestExt, WebViewExt, WebsiteDataAccessPermissionRequest,
};

/// GType name of the clipboard request, which the bindings do not wrap yet.
const CLIPBOARD_PERMISSION_REQUEST_TYPE: &str = "WebKitClipboardPermissionRequest";

/// A WebKit request held for the user. Dropping it unanswered refuses it.
pub(super) struct HeldRequest(Option<Request>);

enum Request {
    Permission(PermissionRequest),
    Dialog(ScriptDialog),
}

impl HeldRequest {
    pub(super) fn answer(mut self, answer: PageAnswer) {
        if let Some(request) = self.0.take() {
            answer_request(request, answer);
        }
    }
}

impl Drop for HeldRequest {
    fn drop(&mut self) {
        if let Some(request) = self.0.take() {
            contain_native_callback((), || answer_request(request, PageAnswer::REFUSE));
        }
    }
}

fn answer_request(request: Request, answer: PageAnswer) {
    match request {
        Request::Permission(request) => {
            if answer.allow {
                request.allow();
            } else {
                request.deny();
            }
        }
        Request::Dialog(dialog) => {
            match dialog.dialog_type() {
                ScriptDialogType::Confirm | ScriptDialogType::BeforeUnloadConfirm => {
                    dialog.confirm_set_confirmed(answer.allow);
                }
                ScriptDialogType::Prompt if answer.allow => {
                    let text = answer.text.unwrap_or_else(|| {
                        dialog
                            .prompt_get_default_text()
                            .map(|text| text.to_string())
                            .unwrap_or_default()
                    });
                    dialog.prompt_set_text(&text);
                }
                _ => {}
            }
            dialog.close();
        }
    }
}

fn permission_kind(request: &PermissionRequest) -> PermissionRequestKind {
    if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
        return PermissionRequestKind::Named(media_capture_permission(
            media.is_for_video_device(),
            media.is_for_audio_device(),
        ));
    }
    let named = if request.is::<GeolocationPermissionRequest>() {
        PagePermission::Location
    } else if request.is::<NotificationPermissionRequest>() {
        PagePermission::Notifications
    } else if request.is::<PointerLockPermissionRequest>() {
        PagePermission::PointerLock
    } else if request.is::<MediaKeySystemPermissionRequest>() {
        PagePermission::ProtectedMedia
    } else if request.is::<WebsiteDataAccessPermissionRequest>() {
        PagePermission::StorageAccess
    } else if request.is::<DeviceInfoPermissionRequest>() {
        return PermissionRequestKind::DeviceInfo;
    } else if request.type_().name() == CLIPBOARD_PERMISSION_REQUEST_TYPE {
        PagePermission::Clipboard
    } else {
        return PermissionRequestKind::Unnamed;
    };
    PermissionRequestKind::Named(named)
}

fn dialog_kind(dialog: &ScriptDialog) -> PageDialogKind {
    match dialog.dialog_type() {
        ScriptDialogType::Confirm => PageDialogKind::Confirm,
        ScriptDialogType::Prompt => PageDialogKind::Prompt,
        ScriptDialogType::BeforeUnloadConfirm => PageDialogKind::BeforeUnload,
        _ => PageDialogKind::Alert,
    }
}

/// Handles a permission request synchronously when it is already decided, or holds it.
/// Returning false leaves the request to WebKit, which denies it.
fn on_permission_request(
    route: &PageRequestRoute,
    webview: &webkit2gtk::WebView,
    request: &PermissionRequest,
) -> bool {
    let uri = webview.uri();
    match route.permission(uri.as_deref(), permission_kind(request)) {
        PermissionAdmission::Allow => request.allow(),
        PermissionAdmission::Deny => request.deny(),
        PermissionAdmission::EngineDefault => return false,
        PermissionAdmission::Ask(id) => hold(
            route,
            id,
            HeldRequest(Some(Request::Permission(request.clone()))),
        ),
    }
    true
}

/// Holds a dialog for the user, or dismisses it during a model action. Returning true keeps
/// WebKit from drawing its own dialog; a held dialog completes when it is closed.
fn on_script_dialog(
    route: &PageRequestRoute,
    webview: &webkit2gtk::WebView,
    dialog: &ScriptDialog,
) -> bool {
    let uri = webview.uri();
    let message = dialog.message().map(|text| text.to_string());
    let default_text = match dialog.dialog_type() {
        ScriptDialogType::Prompt => dialog
            .prompt_get_default_text()
            .map(|text| text.to_string()),
        _ => None,
    };
    match route.dialog(
        uri.as_deref(),
        dialog_kind(dialog),
        message.as_deref().unwrap_or_default(),
        default_text.as_deref().unwrap_or_default(),
    ) {
        DialogAdmission::Dismiss => {
            if matches!(
                dialog.dialog_type(),
                ScriptDialogType::Confirm | ScriptDialogType::BeforeUnloadConfirm
            ) {
                dialog.confirm_set_confirmed(false);
            }
        }
        DialogAdmission::Ask(id) => hold(
            route,
            id,
            HeldRequest(Some(Request::Dialog(dialog.clone()))),
        ),
    }
    true
}

pub(super) fn install(webview: &tauri::Webview, route: PageRequestRoute) -> ChatResult<()> {
    webview
        .with_webview(move |platform| {
            let view = platform.inner();
            let permission_route = route.clone();
            view.connect_permission_request(move |webview, request| {
                contain_native_callback(false, || {
                    on_permission_request(&permission_route, webview, request)
                })
            });
            let dialog_route = route;
            view.connect_script_dialog(move |webview, dialog| {
                contain_native_callback(true, || on_script_dialog(&dialog_route, webview, dialog))
            });
        })
        .map_err(|_| browser_unavailable_error())
}
