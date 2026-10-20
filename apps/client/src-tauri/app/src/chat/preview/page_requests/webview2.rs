//! WebView2 permission and script dialog events, through the typed COM bindings.

use super::*;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{PermissionRequestedEventHandler, ScriptDialogOpeningEventHandler, take_pwstr};
use windows::core::{HSTRING, Interface, PWSTR};

/// A WebView2 request held for the user through its deferral. Dropping it unanswered
/// refuses it.
pub(super) struct HeldRequest(Option<Request>);

enum Request {
    Permission {
        args: ICoreWebView2PermissionRequestedEventArgs,
        deferral: ICoreWebView2Deferral,
    },
    Dialog {
        args: ICoreWebView2ScriptDialogOpeningEventArgs,
        deferral: ICoreWebView2Deferral,
        prompt_default: Option<String>,
    },
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
        Request::Permission { args, deferral } => {
            let state = if answer.allow {
                COREWEBVIEW2_PERMISSION_STATE_ALLOW
            } else {
                COREWEBVIEW2_PERMISSION_STATE_DENY
            };
            // SAFETY: `args` and `deferral` are live typed COM interfaces taken from this
            // event on the WebView thread, which is the thread that runs this answer.
            // Completing the deferral resumes the request with the state just set.
            unsafe {
                let _ = args.SetState(state);
                let _ = deferral.Complete();
            }
        }
        Request::Dialog {
            args,
            deferral,
            prompt_default,
        } => {
            if answer.allow {
                if let Some(default_text) = prompt_default {
                    let text = HSTRING::from(answer.text.unwrap_or(default_text));
                    // SAFETY: `args` is the live typed event argument on its WebView thread,
                    // and `text` is an owned wide string that outlives the call.
                    let _ = unsafe { args.SetResultText(&text) };
                }
                // SAFETY: `args` is the live typed event argument on its WebView thread.
                let _ = unsafe { args.Accept() };
            }
            // SAFETY: `deferral` is the live typed deferral taken from this event on its
            // WebView thread. A dialog completed without `Accept` is cancelled.
            let _ = unsafe { deferral.Complete() };
        }
    }
}

/// Reads a string a WebView2 getter allocates, freeing it afterwards.
fn read_string(getter: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> Option<String> {
    let mut value = PWSTR::null();
    getter(&mut value).ok()?;
    Some(take_pwstr(value))
}

fn permission_kind(kind: COREWEBVIEW2_PERMISSION_KIND) -> PermissionRequestKind {
    let named = match kind {
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => PagePermission::Camera,
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => PagePermission::Microphone,
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => PagePermission::Location,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => PagePermission::Notifications,
        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS => PagePermission::Sensors,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => PagePermission::Clipboard,
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => PagePermission::FileAccess,
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS => PagePermission::LocalFonts,
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES => PagePermission::Midi,
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT => PagePermission::WindowManagement,
        COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY => return PermissionRequestKind::EngineDefault,
        _ => return PermissionRequestKind::Unnamed,
    };
    PermissionRequestKind::Named(named)
}

fn dialog_kind(kind: COREWEBVIEW2_SCRIPT_DIALOG_KIND) -> PageDialogKind {
    match kind {
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM => PageDialogKind::Confirm,
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT => PageDialogKind::Prompt,
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD => PageDialogKind::BeforeUnload,
        _ => PageDialogKind::Alert,
    }
}

fn on_permission_requested(
    route: &PageRequestRoute,
    args: &ICoreWebView2PermissionRequestedEventArgs,
) -> windows::core::Result<()> {
    let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
    // SAFETY: `args` is the live typed event argument for this callback on the WebView
    // thread, and `kind` is writable storage of the exact type the getter fills.
    unsafe { args.PermissionKind(&mut kind) }?;
    // SAFETY: as above; the getter allocates the string that `read_string` frees.
    let uri = read_string(|value| unsafe { args.Uri(value) });
    if let Ok(args) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
        // SAFETY: `args` is the same live event argument through a newer interface.
        // Answers stay in Ganbaru's thread memory instead of the shared profile.
        let _ = unsafe { args.SetSavesInProfile(false) };
    }
    let state = match route.permission(uri.as_deref(), permission_kind(kind)) {
        PermissionAdmission::Allow => COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        PermissionAdmission::Deny => COREWEBVIEW2_PERMISSION_STATE_DENY,
        PermissionAdmission::EngineDefault => return Ok(()),
        PermissionAdmission::Ask(id) => {
            // SAFETY: `args` is the live typed event argument on the WebView thread. The
            // returned deferral keeps the request open until it is completed.
            match unsafe { args.GetDeferral() } {
                Ok(deferral) => {
                    let request = Request::Permission {
                        args: args.clone(),
                        deferral,
                    };
                    hold(route, id, HeldRequest(Some(request)));
                    return Ok(());
                }
                Err(_) => {
                    route.withdraw(id);
                    COREWEBVIEW2_PERMISSION_STATE_DENY
                }
            }
        }
    };
    // SAFETY: `args` is the live typed event argument on the WebView thread.
    unsafe { args.SetState(state) }
}

fn on_script_dialog_opening(
    route: &PageRequestRoute,
    args: &ICoreWebView2ScriptDialogOpeningEventArgs,
) -> windows::core::Result<()> {
    let mut kind = COREWEBVIEW2_SCRIPT_DIALOG_KIND::default();
    // SAFETY: `args` is the live typed event argument for this callback on the WebView
    // thread, and `kind` is writable storage of the exact type the getter fills.
    unsafe { args.Kind(&mut kind) }?;
    // SAFETY: as above; each getter allocates the string that `read_string` frees.
    let uri = read_string(|value| unsafe { args.Uri(value) });
    // SAFETY: as above.
    let message = read_string(|value| unsafe { args.Message(value) }).unwrap_or_default();
    let prompt_default = if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT {
        // SAFETY: as above.
        Some(read_string(|value| unsafe { args.DefaultText(value) }).unwrap_or_default())
    } else {
        None
    };
    match route.dialog(
        uri.as_deref(),
        dialog_kind(kind),
        &message,
        prompt_default.as_deref().unwrap_or_default(),
    ) {
        // Returning without `Accept` cancels the dialog.
        DialogAdmission::Dismiss => {}
        // SAFETY: `args` is the live typed event argument on the WebView thread. The
        // returned deferral keeps the dialog open until it is completed.
        DialogAdmission::Ask(id) => match unsafe { args.GetDeferral() } {
            Ok(deferral) => {
                let request = Request::Dialog {
                    args: args.clone(),
                    deferral,
                    prompt_default,
                };
                hold(route, id, HeldRequest(Some(request)));
            }
            Err(_) => route.withdraw(id),
        },
    }
    Ok(())
}

pub(super) fn install(webview: &tauri::Webview, route: PageRequestRoute) -> ChatResult<()> {
    webview
        .with_webview(move |platform| {
            // SAFETY: `controller()` returns a live typed COM controller owned by the
            // platform webview. The getter returns its typed CoreWebView2 interface.
            let core = match unsafe { platform.controller().CoreWebView2() } {
                Ok(core) => core,
                Err(error) => {
                    eprintln!("Could not route Chat preview page requests: {error}");
                    return;
                }
            };
            let permission_route = route.clone();
            let permissions = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                contain_native_callback(Ok(()), || match args {
                    Some(args) => on_permission_requested(&permission_route, &args),
                    None => Ok(()),
                })
            }));
            let dialog_route = route;
            let dialogs = ScriptDialogOpeningEventHandler::create(Box::new(move |_, args| {
                contain_native_callback(Ok(()), || match args {
                    Some(args) => on_script_dialog_opening(&dialog_route, &args),
                    None => Ok(()),
                })
            }));
            let mut token = 0_i64;
            // SAFETY: `core` is live on the WebView thread, the handlers are typed COM
            // objects that WebView2 retains and that own their state, and `token` is
            // writable storage for the registration token.
            if let Err(error) = unsafe { core.add_PermissionRequested(&permissions, &mut token) } {
                eprintln!("Could not route Chat preview permission requests: {error}");
            }
            // SAFETY: as above.
            let dialogs_routed = unsafe { core.add_ScriptDialogOpening(&dialogs, &mut token) };
            // Default dialogs are disabled only once the handler is in place, so a failed
            // registration keeps WebView2's own dialogs instead of silently cancelling.
            let disabled = dialogs_routed.and_then(|()| {
                // SAFETY: `core` is live on the WebView thread; `Settings` returns its typed
                // settings interface and the setter takes a plain flag.
                unsafe { core.Settings()?.SetAreDefaultScriptDialogsEnabled(false) }
            });
            if let Err(error) = disabled {
                eprintln!("Could not route Chat preview dialogs: {error}");
            }
        })
        .map_err(|_| browser_unavailable_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview2_permissions_map_to_named_prompts_or_engine_defaults() {
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_CAMERA),
            PermissionRequestKind::Named(PagePermission::Camera)
        );
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ),
            PermissionRequestKind::Named(PagePermission::Clipboard)
        );
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY),
            PermissionRequestKind::EngineDefault
        );
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS),
            PermissionRequestKind::Unnamed
        );
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION),
            PermissionRequestKind::Unnamed
        );
        assert_eq!(
            dialog_kind(COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD),
            PageDialogKind::BeforeUnload
        );
    }
}
