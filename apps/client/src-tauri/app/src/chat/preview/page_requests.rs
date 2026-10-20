//! Permission requests and JavaScript dialogs raised by preview pages.
//!
//! Engines would otherwise answer these with their own defaults: WebKitGTK denies every
//! permission and draws its own dialogs, and WebView2 shows its own prompts and remembers
//! answers in the shared working-folder profile. Instead the Browser panel asks the user,
//! naming the page's origin, and remembers permission answers per thread and origin for the
//! app run. Dialogs raised while a model tool action runs on the tab are dismissed
//! automatically and reported to the model, so a page cannot stall the action or put a
//! dialog in front of the user that the model caused. Requests that cannot be named to the
//! user are denied.
//!
//! Native request objects live on the main thread in the platform module; this module keeps
//! the thread-safe bookkeeping and decides how each request is answered.

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use tauri::Emitter;

#[cfg(target_os = "linux")]
mod webkit;
#[cfg(target_os = "linux")]
use webkit as native;
#[cfg(windows)]
mod webview2;
#[cfg(windows)]
use webview2 as native;
#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", windows)))]
use unsupported as native;

/// Characters of a dialog message or prompt text shown to the user.
pub(super) const MAX_DIALOG_TEXT_CHARS: usize = 4_096;
/// Page requests one thread may have waiting before new ones are refused.
const MAX_PENDING_PAGE_REQUESTS_PER_THREAD: usize = 8;
/// Remembered permission answers per thread.
const MAX_REMEMBERED_PERMISSIONS_PER_THREAD: usize = 64;
/// Dismissed dialogs one model action reports individually; later ones are only counted.
const MAX_MODEL_ACTION_DIALOGS: usize = 8;
/// Characters of each dismissed dialog message reported to the model.
const MODEL_ACTION_DIALOG_MESSAGE_CHARS: usize = 500;

static NEXT_PAGE_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_MODEL_ACTION_ID: AtomicU64 = AtomicU64::new(1);

/// A page capability the user can grant, named the same way in the Browser panel. Sensors,
/// file access, local fonts, MIDI, and window management are reported by WebView2 only.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum PagePermission {
    Camera,
    Microphone,
    CameraAndMicrophone,
    ScreenCapture,
    Location,
    Notifications,
    Clipboard,
    PointerLock,
    ProtectedMedia,
    StorageAccess,
    Sensors,
    FileAccess,
    LocalFonts,
    Midi,
    WindowManagement,
}

impl PagePermission {
    /// Whether an answer for this permission also answers a request to list media devices.
    fn covers_device_info(self) -> bool {
        matches!(
            self,
            Self::Camera | Self::Microphone | Self::CameraAndMicrophone
        )
    }
}

/// What an engine asked for, before Ganbaru decides whether the user is asked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PermissionRequestKind {
    /// A permission the Browser panel can name.
    Named(PagePermission),
    /// Media device labels, answered from the origin's camera and microphone answers.
    DeviceInfo,
    /// Left to the engine because it is not sensitive, such as WebView2 autoplay.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    EngineDefault,
    /// Denied without asking, because the user could not judge it from a name.
    Unnamed,
}

/// Names a media capture request. A request for neither camera nor microphone is a display
/// capture, which is how WebKit reports `getDisplayMedia`.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(super) fn media_capture_permission(video: bool, audio: bool) -> PagePermission {
    match (video, audio) {
        (true, true) => PagePermission::CameraAndMicrophone,
        (true, false) => PagePermission::Camera,
        (false, true) => PagePermission::Microphone,
        (false, false) => PagePermission::ScreenCapture,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PageDialogKind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
}

/// How a permission request is answered right away, or that the user is asked.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum PermissionAdmission {
    Allow,
    Deny,
    EngineDefault,
    Ask(u64),
}

/// Whether a dialog is dismissed right away or shown to the user.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum DialogAdmission {
    Dismiss,
    Ask(u64),
}

/// The user's answer, delivered to the native request on the main thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PageAnswer {
    /// Allow a permission, or accept a dialog (OK, or Leave for `beforeunload`).
    pub(super) allow: bool,
    /// Text a prompt returns when accepted.
    pub(super) text: Option<String>,
}

impl PageAnswer {
    pub(super) const REFUSE: Self = Self {
        allow: false,
        text: None,
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PagePrompt {
    Permission(PagePermission),
    Dialog {
        kind: PageDialogKind,
        message: String,
        default_text: String,
    },
}

#[derive(Clone, Debug)]
struct PendingPageRequest {
    thread_id: ChatThreadId,
    tab_id: String,
    origin: String,
    prompt: PagePrompt,
}

/// A dialog dismissed during a model action, as reported to the model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissedDialog {
    pub kind: PageDialogKind,
    pub message: String,
}

/// Dialogs dismissed while one model action ran.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelActionDialogs {
    pub dialogs: Vec<DismissedDialog>,
    pub count: u32,
}

impl ModelActionDialogs {
    /// The preview tool result for an accepted action, naming any dismissed dialogs.
    pub(crate) fn tool_result(&self) -> serde_json::Value {
        if self.count == 0 {
            return serde_json::json!({ "accepted": true });
        }
        serde_json::json!({
            "accepted": true,
            "dismissedDialogs": self.dialogs,
            "dismissedDialogCount": self.count,
        })
    }

    fn record(&mut self, kind: PageDialogKind, message: &str) {
        self.count = self.count.saturating_add(1);
        if self.dialogs.len() < MAX_MODEL_ACTION_DIALOGS {
            self.dialogs.push(DismissedDialog {
                kind,
                message: message
                    .chars()
                    .take(MODEL_ACTION_DIALOG_MESSAGE_CHARS)
                    .collect(),
            });
        }
    }
}

/// Thread-safe bookkeeping for page requests, remembered answers, and model actions.
#[derive(Default)]
pub(super) struct PageRequestState {
    pending: BTreeMap<u64, PendingPageRequest>,
    remembered: HashMap<(ChatThreadId, String, PagePermission), bool>,
    model_actions: HashMap<u64, (String, ModelActionDialogs)>,
}

impl PageRequestState {
    fn pending_for_thread(&self, thread_id: &ChatThreadId) -> usize {
        self.pending
            .values()
            .filter(|request| &request.thread_id == thread_id)
            .count()
    }

    fn insert_pending(&mut self, request: PendingPageRequest) -> Option<u64> {
        if self.pending_for_thread(&request.thread_id) >= MAX_PENDING_PAGE_REQUESTS_PER_THREAD {
            return None;
        }
        let id = NEXT_PAGE_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.pending.insert(id, request);
        Some(id)
    }

    /// Decides how a permission request from `origin` in a thread's tab is answered.
    pub(super) fn admit_permission(
        &mut self,
        thread_id: &ChatThreadId,
        tab_id: &str,
        origin: Option<String>,
        kind: PermissionRequestKind,
    ) -> PermissionAdmission {
        let permission = match kind {
            PermissionRequestKind::Named(permission) => permission,
            PermissionRequestKind::EngineDefault => return PermissionAdmission::EngineDefault,
            PermissionRequestKind::Unnamed => return PermissionAdmission::Deny,
            PermissionRequestKind::DeviceInfo => {
                let Some(origin) = origin else {
                    return PermissionAdmission::Deny;
                };
                let allowed = self
                    .remembered
                    .iter()
                    .any(|((thread, known, permission), allow)| {
                        *allow
                            && thread == thread_id
                            && known == &origin
                            && permission.covers_device_info()
                    });
                return if allowed {
                    PermissionAdmission::Allow
                } else {
                    PermissionAdmission::Deny
                };
            }
        };
        let Some(origin) = origin else {
            return PermissionAdmission::Deny;
        };
        match self
            .remembered
            .get(&(thread_id.clone(), origin.clone(), permission))
        {
            Some(true) => return PermissionAdmission::Allow,
            Some(false) => return PermissionAdmission::Deny,
            None => {}
        }
        let duplicate = self.pending.iter().any(|(_, request)| {
            &request.thread_id == thread_id
                && request.tab_id == tab_id
                && request.origin == origin
                && request.prompt == PagePrompt::Permission(permission)
        });
        if duplicate {
            // The engine asks again before the user answered; the earlier prompt stands.
            return PermissionAdmission::Deny;
        }
        match self.insert_pending(PendingPageRequest {
            thread_id: thread_id.clone(),
            tab_id: tab_id.to_string(),
            origin,
            prompt: PagePrompt::Permission(permission),
        }) {
            Some(id) => PermissionAdmission::Ask(id),
            None => PermissionAdmission::Deny,
        }
    }

    /// Decides whether a dialog is shown to the user or dismissed for a running model action.
    pub(super) fn admit_dialog(
        &mut self,
        thread_id: &ChatThreadId,
        tab_id: &str,
        origin: Option<String>,
        kind: PageDialogKind,
        message: &str,
        default_text: &str,
    ) -> DialogAdmission {
        let mut model_action = false;
        for (action_tab, dialogs) in self.model_actions.values_mut() {
            if action_tab == tab_id {
                dialogs.record(kind, message);
                model_action = true;
            }
        }
        if model_action {
            return DialogAdmission::Dismiss;
        }
        let Some(origin) = origin else {
            return DialogAdmission::Dismiss;
        };
        match self.insert_pending(PendingPageRequest {
            thread_id: thread_id.clone(),
            tab_id: tab_id.to_string(),
            origin,
            prompt: PagePrompt::Dialog {
                kind,
                message: bounded_dialog_text(message),
                default_text: bounded_dialog_text(default_text),
            },
        }) {
            Some(id) => DialogAdmission::Ask(id),
            None => DialogAdmission::Dismiss,
        }
    }

    /// Removes a thread's pending request and remembers a permission answer. Returns false
    /// when the request is gone or belongs to another thread.
    fn resolve(&mut self, thread_id: &ChatThreadId, request_id: u64, allow: bool) -> bool {
        let Some(request) = self.pending.get(&request_id) else {
            return false;
        };
        if &request.thread_id != thread_id {
            return false;
        }
        let Some(request) = self.pending.remove(&request_id) else {
            return false;
        };
        if let PagePrompt::Permission(permission) = request.prompt {
            let remembered = self
                .remembered
                .keys()
                .filter(|(thread, _, _)| thread == thread_id)
                .count();
            if remembered < MAX_REMEMBERED_PERMISSIONS_PER_THREAD {
                self.remembered
                    .insert((request.thread_id, request.origin, permission), allow);
            }
        }
        true
    }

    /// Removes pending requests matching a thread and tab and returns their ids and threads.
    fn take_pending(
        &mut self,
        matches: impl Fn(&ChatThreadId, &str) -> bool,
    ) -> Vec<(u64, ChatThreadId)> {
        let ids = self
            .pending
            .iter()
            .filter(|(_, request)| matches(&request.thread_id, &request.tab_id))
            .map(|(id, request)| (*id, request.thread_id.clone()))
            .collect::<Vec<_>>();
        for (id, _) in &ids {
            self.pending.remove(id);
        }
        ids
    }

    fn forget_thread(&mut self, thread_id: &ChatThreadId) {
        self.remembered
            .retain(|(thread, _, _), _| thread != thread_id);
    }

    fn first_for_thread(&self, thread_id: &ChatThreadId) -> Option<approvals::PreviewApprovalRead> {
        self.pending
            .iter()
            .find(|(_, request)| &request.thread_id == thread_id)
            .map(|(id, request)| approval_read(*id, request))
    }

    /// Opens a model action window on a tab. Dialogs the tab already shows the user are taken
    /// and returned for refusal, because a waiting dialog blocks the page the model acts on;
    /// they are reported to the model like dialogs raised during the action.
    fn begin_model_action(&mut self, tab_id: &str) -> (u64, Vec<(u64, ChatThreadId)>) {
        let mut dialogs = ModelActionDialogs::default();
        let blocking = self
            .pending
            .iter()
            .filter_map(|(id, request)| match &request.prompt {
                PagePrompt::Dialog { kind, message, .. } if request.tab_id == tab_id => {
                    dialogs.record(*kind, message);
                    Some((*id, request.thread_id.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (id, _) in &blocking {
            self.pending.remove(id);
        }
        let id = NEXT_MODEL_ACTION_ID.fetch_add(1, Ordering::Relaxed);
        self.model_actions.insert(id, (tab_id.to_string(), dialogs));
        (id, blocking)
    }

    fn finish_model_action(&mut self, id: u64) -> ModelActionDialogs {
        self.model_actions
            .remove(&id)
            .map(|(_, dialogs)| dialogs)
            .unwrap_or_default()
    }
}

fn approval_read(id: u64, request: &PendingPageRequest) -> approvals::PreviewApprovalRead {
    match &request.prompt {
        PagePrompt::Permission(permission) => approvals::PreviewApprovalRead::Permission {
            request_id: id,
            origin: request.origin.clone(),
            permission: *permission,
        },
        PagePrompt::Dialog {
            kind,
            message,
            default_text,
        } => approvals::PreviewApprovalRead::Dialog {
            request_id: id,
            origin: request.origin.clone(),
            dialog_kind: *kind,
            message: message.clone(),
            default_text: default_text.clone(),
        },
    }
}

/// Bounds page-controlled dialog text to what the Browser panel shows.
pub(super) fn bounded_dialog_text(value: &str) -> String {
    value.chars().take(MAX_DIALOG_TEXT_CHARS).collect()
}

/// The origin the user sees for a request from a page at `url`. Opaque origins have none.
pub(super) fn request_origin(url: Option<&str>) -> Option<String> {
    let url = reqwest::Url::parse(url?).ok()?;
    let origin = url.origin();
    origin
        .is_tuple()
        .then(|| origin.ascii_serialization())
        .filter(|origin| origin.len() <= MAX_BROWSER_URL_BYTES)
}

/// Marks a tab as running a model action until `finish` or drop, so its dialogs are dismissed.
pub(crate) struct ModelActionWindow {
    manager: ChatBrowserManager,
    id: Option<u64>,
}

impl ModelActionWindow {
    /// Ends the window and returns the dialogs dismissed during it.
    pub(crate) fn finish(mut self) -> ModelActionDialogs {
        self.close()
    }

    fn close(&mut self) -> ModelActionDialogs {
        let Some(id) = self.id.take() else {
            return ModelActionDialogs::default();
        };
        self.manager
            .page_requests
            .lock()
            .map(|mut state| state.finish_model_action(id))
            .unwrap_or_default()
    }
}

impl Drop for ModelActionWindow {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Tells the Browser panel that a thread's page requests changed.
fn notify_thread<R: tauri::Runtime>(app: &tauri::AppHandle<R>, thread_id: &ChatThreadId) {
    let _ = app.emit(
        approvals::CHAT_BROWSER_APPROVAL_EVENT,
        approvals::PreviewApprovalNotice { thread_id },
    );
}

/// Delivers refusals for requests that will never be answered, on the main thread.
fn settle_refused<R: tauri::Runtime>(app: &tauri::AppHandle<R>, refused: Vec<(u64, ChatThreadId)>) {
    if refused.is_empty() {
        return;
    }
    let threads = refused
        .iter()
        .map(|(_, thread)| thread.clone())
        .collect::<HashSet<_>>();
    let ids = refused.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    if let Err(error) = app.run_on_main_thread(move || {
        for id in ids {
            settle(id, PageAnswer::REFUSE);
        }
    }) {
        eprintln!("Could not refuse Chat preview page requests: {error}");
    }
    for thread in threads {
        notify_thread(app, &thread);
    }
}

impl ChatBrowserManager {
    /// Starts a model action on a tab. Dialogs the tab raises until it ends are dismissed.
    pub(crate) fn begin_model_action<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        tab_id: &str,
    ) -> ModelActionWindow {
        let (id, blocking) = match self.page_requests.lock() {
            Ok(mut state) => {
                let (id, blocking) = state.begin_model_action(tab_id);
                (Some(id), blocking)
            }
            Err(_) => (None, Vec::new()),
        };
        settle_refused(app, blocking);
        ModelActionWindow {
            manager: self.clone(),
            id,
        }
    }

    pub(super) fn pending_page_request(
        &self,
        thread_id: &ChatThreadId,
    ) -> ChatResult<Option<approvals::PreviewApprovalRead>> {
        Ok(self
            .page_requests
            .lock()
            .map_err(|_| browser_state_error())?
            .first_for_thread(thread_id))
    }

    /// Refuses the matching pending requests. Call it before closing their webviews, so the
    /// refusals reach the main thread first.
    fn refuse_page_requests<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        matches: impl Fn(&ChatThreadId, &str) -> bool,
    ) {
        let refused = self
            .page_requests
            .lock()
            .map(|mut state| state.take_pending(matches))
            .unwrap_or_default();
        settle_refused(app, refused);
    }

    /// Refuses the pending requests of a tab that is closing.
    pub(super) fn refuse_tab_page_requests<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        tab_id: &str,
    ) {
        self.refuse_page_requests(app, |_, tab| tab == tab_id);
    }

    /// Refuses a closing thread's pending requests and forgets its permission answers.
    pub(super) fn forget_thread_page_requests<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        thread_id: &ChatThreadId,
    ) {
        if let Ok(mut state) = self.page_requests.lock() {
            state.forget_thread(thread_id);
        }
        self.refuse_page_requests(app, |thread, _| thread == thread_id);
    }

    /// Refuses every pending request and forgets every answer, when all previews close.
    pub(super) fn forget_all_page_requests<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) {
        self.forget_page_permissions();
        self.refuse_page_requests(app, |_, _| true);
    }

    /// Forgets every remembered permission answer, as part of clearing preview data.
    pub(super) fn forget_page_permissions(&self) {
        if let Ok(mut state) = self.page_requests.lock() {
            state.remembered.clear();
        }
    }
}

/// Answers a thread's pending page request from the Browser panel.
pub(super) fn resolve_page_request(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    request_id: u64,
    allow: bool,
    text: Option<String>,
) -> ChatResult<()> {
    if text
        .as_ref()
        .is_some_and(|text| text.chars().count() > MAX_DIALOG_TEXT_CHARS)
    {
        return Err(ChatError::validation(
            "text",
            "Dialog text exceeds the supported limit",
        ));
    }
    let manager = app.state::<ChatBrowserManager>();
    let resolved = manager
        .page_requests
        .lock()
        .map_err(|_| browser_state_error())?
        .resolve(thread_id, request_id, allow);
    if !resolved {
        return Ok(());
    }
    let answer = PageAnswer { allow, text };
    app.run_on_main_thread(move || settle(request_id, answer))
        .map_err(|_| browser_unavailable_error())
}

/// Routes the webview's permission requests and dialogs through the Browser panel.
pub(super) fn install(
    app: &tauri::AppHandle,
    webview: &tauri::Webview,
    thread_id: &ChatThreadId,
    tab_id: &str,
) -> ChatResult<()> {
    native::install(
        webview,
        PageRequestRoute {
            app: app.clone(),
            thread_id: thread_id.clone(),
            tab_id: tab_id.to_string(),
        },
    )
}

/// What a native handler needs to admit a request and tell the Browser panel about it.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
#[derive(Clone)]
pub(super) struct PageRequestRoute {
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
    tab_id: String,
}

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
impl PageRequestRoute {
    fn state<T>(&self, update: impl FnOnce(&mut PageRequestState) -> T) -> Option<T> {
        self.app
            .state::<ChatBrowserManager>()
            .page_requests
            .lock()
            .ok()
            .map(|mut state| update(&mut state))
    }

    /// Decides a permission request, notifying the panel when the user is asked.
    pub(super) fn permission(
        &self,
        url: Option<&str>,
        kind: PermissionRequestKind,
    ) -> PermissionAdmission {
        let origin = request_origin(url);
        let admission = self
            .state(|state| state.admit_permission(&self.thread_id, &self.tab_id, origin, kind))
            .unwrap_or(PermissionAdmission::Deny);
        if matches!(admission, PermissionAdmission::Ask(_)) {
            notify_thread(&self.app, &self.thread_id);
        }
        admission
    }

    /// Decides a dialog, notifying the panel when the user is asked.
    pub(super) fn dialog(
        &self,
        url: Option<&str>,
        kind: PageDialogKind,
        message: &str,
        default_text: &str,
    ) -> DialogAdmission {
        let origin = request_origin(url);
        let admission = self
            .state(|state| {
                state.admit_dialog(
                    &self.thread_id,
                    &self.tab_id,
                    origin,
                    kind,
                    message,
                    default_text,
                )
            })
            .unwrap_or(DialogAdmission::Dismiss);
        if matches!(admission, DialogAdmission::Ask(_)) {
            notify_thread(&self.app, &self.thread_id);
        }
        admission
    }

    /// Withdraws a request whose native object could not be held, so the panel never
    /// shows a prompt that cannot be delivered.
    pub(super) fn withdraw(&self, request_id: u64) {
        let removed = self
            .state(|state| state.pending.remove(&request_id).is_some())
            .unwrap_or(false);
        if removed {
            notify_thread(&self.app, &self.thread_id);
        }
    }
}

thread_local! {
    /// Native requests waiting for the user, kept on the main thread that owns them. Each
    /// held request answers itself with a refusal if it is dropped unanswered.
    static HELD_REQUESTS: RefCell<HashMap<u64, native::HeldRequest>> =
        RefCell::new(HashMap::new());
}

/// Keeps a native request until the user answers it. Called on the main thread from the
/// engine callback that admitted it; a request that cannot be kept is withdrawn and refused.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
fn hold(route: &PageRequestRoute, request_id: u64, request: native::HeldRequest) {
    let mut request = Some(request);
    let _ = HELD_REQUESTS.try_with(|held| {
        if let Ok(mut held) = held.try_borrow_mut()
            && let Some(request) = request.take()
        {
            held.insert(request_id, request);
        }
    });
    if let Some(request) = request {
        route.withdraw(request_id);
        request.answer(PageAnswer::REFUSE);
    }
}

/// Delivers an answer to a held native request. Runs on the main thread.
fn settle(request_id: u64, answer: PageAnswer) {
    let request = HELD_REQUESTS
        .try_with(|held| {
            held.try_borrow_mut()
                .ok()
                .and_then(|mut held| held.remove(&request_id))
        })
        .ok()
        .flatten();
    if let Some(request) = request {
        contain_native_callback((), || request.answer(answer));
    }
}

/// Runs a native handler body behind an unwind barrier. Engine callbacks cross foreign ABIs
/// that cannot take a Rust unwind; a caught payload is leaked because dropping it may panic.
#[cfg_attr(not(any(target_os = "linux", windows, test)), allow(dead_code))]
fn contain_native_callback<T>(fallback: T, callback: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
        Ok(value) => value,
        Err(payload) => {
            std::mem::forget(payload);
            fallback
        }
    }
}

#[cfg(test)]
mod tests;
