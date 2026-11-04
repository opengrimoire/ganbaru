use super::*;

const ORIGIN: &str = "http://127.0.0.1:5173";
const OTHER_ORIGIN: &str = "http://localhost:3000";
const TAB: &str = "tab-1";
const OTHER_TAB: &str = "tab-2";

fn thread(id: &str) -> ChatThreadId {
    ChatThreadId::new(id).expect("thread ID should be valid")
}

fn ask_permission(
    state: &mut PageRequestState,
    thread_id: &ChatThreadId,
    tab_id: &str,
    origin: &str,
    permission: PagePermission,
) -> PermissionAdmission {
    state.admit_permission(
        thread_id,
        tab_id,
        Some(origin.to_string()),
        PermissionRequestKind::Named(permission),
    )
}

fn ask_id(admission: PermissionAdmission) -> u64 {
    match admission {
        PermissionAdmission::Ask(id) => id,
        other => panic!("expected a prompt, got {other:?}"),
    }
}

fn open_dialog(
    state: &mut PageRequestState,
    thread_id: &ChatThreadId,
    tab_id: &str,
    kind: PageDialogKind,
    message: &str,
) -> DialogAdmission {
    state.admit_dialog(
        thread_id,
        tab_id,
        Some(ORIGIN.to_string()),
        kind,
        message,
        "",
    )
}

#[test]
fn media_capture_requests_name_the_devices_they_use() {
    assert_eq!(
        media_capture_permission(true, true),
        PagePermission::CameraAndMicrophone
    );
    assert_eq!(
        media_capture_permission(true, false),
        PagePermission::Camera
    );
    assert_eq!(
        media_capture_permission(false, true),
        PagePermission::Microphone
    );
    assert_eq!(
        media_capture_permission(false, false),
        PagePermission::ScreenCapture
    );
}

#[test]
fn request_origins_name_tuple_origins_and_refuse_opaque_ones() {
    assert_eq!(
        request_origin(Some("http://127.0.0.1:5173/app?x=1#y")).as_deref(),
        Some(ORIGIN)
    );
    assert_eq!(
        request_origin(Some("https://example.com/")).as_deref(),
        Some("https://example.com")
    );
    assert_eq!(request_origin(Some("data:text/html,<p>x</p>")), None);
    assert_eq!(request_origin(Some("about:blank")), None);
    assert_eq!(request_origin(Some("not a url")), None);
    assert_eq!(request_origin(None), None);
}

#[test]
fn permission_answers_are_remembered_per_thread_origin_and_permission() {
    let mut state = PageRequestState::default();
    let first = thread("thread:first");
    let second = thread("thread:second");
    let id = ask_id(ask_permission(
        &mut state,
        &first,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
    assert!(state.resolve(&first, id, true));

    assert_eq!(
        ask_permission(
            &mut state,
            &first,
            OTHER_TAB,
            ORIGIN,
            PagePermission::Camera
        ),
        PermissionAdmission::Allow
    );
    ask_id(ask_permission(
        &mut state,
        &first,
        TAB,
        OTHER_ORIGIN,
        PagePermission::Camera,
    ));
    ask_id(ask_permission(
        &mut state,
        &first,
        TAB,
        ORIGIN,
        PagePermission::Microphone,
    ));
    ask_id(ask_permission(
        &mut state,
        &second,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));

    let id = ask_id(ask_permission(
        &mut state,
        &first,
        TAB,
        ORIGIN,
        PagePermission::Notifications,
    ));
    assert!(state.resolve(&first, id, false));
    assert_eq!(
        ask_permission(
            &mut state,
            &first,
            TAB,
            ORIGIN,
            PagePermission::Notifications
        ),
        PermissionAdmission::Deny
    );

    state.forget_thread(&first);
    ask_id(ask_permission(
        &mut state,
        &first,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
}

#[test]
fn requests_that_cannot_be_named_are_denied_without_a_prompt() {
    let mut state = PageRequestState::default();
    let thread_id = thread("thread:test");
    assert_eq!(
        state.admit_permission(
            &thread_id,
            TAB,
            None,
            PermissionRequestKind::Named(PagePermission::Location),
        ),
        PermissionAdmission::Deny
    );
    assert_eq!(
        state.admit_permission(
            &thread_id,
            TAB,
            Some(ORIGIN.to_string()),
            PermissionRequestKind::Unnamed
        ),
        PermissionAdmission::Deny
    );
    assert_eq!(
        state.admit_permission(
            &thread_id,
            TAB,
            Some(ORIGIN.to_string()),
            PermissionRequestKind::EngineDefault
        ),
        PermissionAdmission::EngineDefault
    );
    assert_eq!(
        state.admit_dialog(&thread_id, TAB, None, PageDialogKind::Alert, "x", ""),
        DialogAdmission::Dismiss
    );
    assert!(state.first_for_thread(&thread_id).is_none());
}

#[test]
fn device_lists_follow_the_origins_camera_and_microphone_answers() {
    let mut state = PageRequestState::default();
    let thread_id = thread("thread:test");
    let device_info = |state: &mut PageRequestState, origin: &str| {
        state.admit_permission(
            &thread_id,
            TAB,
            Some(origin.to_string()),
            PermissionRequestKind::DeviceInfo,
        )
    };
    assert_eq!(device_info(&mut state, ORIGIN), PermissionAdmission::Deny);

    let id = ask_id(ask_permission(
        &mut state,
        &thread_id,
        TAB,
        ORIGIN,
        PagePermission::Location,
    ));
    state.resolve(&thread_id, id, true);
    assert_eq!(device_info(&mut state, ORIGIN), PermissionAdmission::Deny);

    let id = ask_id(ask_permission(
        &mut state,
        &thread_id,
        TAB,
        ORIGIN,
        PagePermission::Microphone,
    ));
    state.resolve(&thread_id, id, true);
    assert_eq!(device_info(&mut state, ORIGIN), PermissionAdmission::Allow);
    assert_eq!(
        device_info(&mut state, OTHER_ORIGIN),
        PermissionAdmission::Deny
    );
    assert!(state.first_for_thread(&thread_id).is_none());
}

#[test]
fn repeated_and_excess_requests_are_denied_instead_of_queued() {
    let mut state = PageRequestState::default();
    let thread_id = thread("thread:test");
    ask_id(ask_permission(
        &mut state,
        &thread_id,
        TAB,
        ORIGIN,
        PagePermission::Clipboard,
    ));
    assert_eq!(
        ask_permission(
            &mut state,
            &thread_id,
            TAB,
            ORIGIN,
            PagePermission::Clipboard
        ),
        PermissionAdmission::Deny
    );
    for index in 1..MAX_PENDING_PAGE_REQUESTS_PER_THREAD {
        assert!(matches!(
            open_dialog(
                &mut state,
                &thread_id,
                TAB,
                PageDialogKind::Alert,
                &index.to_string()
            ),
            DialogAdmission::Ask(_)
        ));
    }
    assert_eq!(
        open_dialog(&mut state, &thread_id, TAB, PageDialogKind::Alert, "excess"),
        DialogAdmission::Dismiss
    );
    assert_eq!(
        ask_permission(&mut state, &thread_id, TAB, ORIGIN, PagePermission::Camera),
        PermissionAdmission::Deny
    );
    ask_id(ask_permission(
        &mut state,
        &thread("thread:other"),
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
}

#[test]
fn remembered_answers_are_capped_per_thread() {
    let mut state = PageRequestState::default();
    let thread_id = thread("thread:test");
    for port in 0..MAX_REMEMBERED_PERMISSIONS_PER_THREAD + 1 {
        let origin = format!("http://127.0.0.1:{}", 3000 + port);
        let id = ask_id(ask_permission(
            &mut state,
            &thread_id,
            TAB,
            &origin,
            PagePermission::Camera,
        ));
        assert!(state.resolve(&thread_id, id, true));
    }
    assert_eq!(
        state.remembered.len(),
        MAX_REMEMBERED_PERMISSIONS_PER_THREAD
    );
}

#[test]
fn only_the_owning_thread_can_answer_a_request() {
    let mut state = PageRequestState::default();
    let owner = thread("thread:owner");
    let id = ask_id(ask_permission(
        &mut state,
        &owner,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
    assert!(!state.resolve(&thread("thread:other"), id, true));
    assert!(state.first_for_thread(&owner).is_some());
    assert!(state.resolve(&owner, id, false));
    assert!(!state.resolve(&owner, id, true));
    assert!(state.first_for_thread(&owner).is_none());
}

#[test]
fn pending_requests_are_read_oldest_first_with_bounded_page_text() {
    let mut state = PageRequestState::default();
    let thread_id = thread("thread:test");
    let long = "m".repeat(MAX_DIALOG_TEXT_CHARS + 10);
    let DialogAdmission::Ask(dialog_id) = state.admit_dialog(
        &thread_id,
        TAB,
        Some(ORIGIN.to_string()),
        PageDialogKind::Prompt,
        &long,
        &long,
    ) else {
        panic!("dialog should be shown to the user");
    };
    ask_id(ask_permission(
        &mut state,
        &thread_id,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));

    let read = serde_json::to_value(state.first_for_thread(&thread_id))
        .expect("approval should serialize");
    assert_eq!(read["kind"], "dialog");
    assert_eq!(read["requestId"], dialog_id);
    assert_eq!(read["origin"], ORIGIN);
    assert_eq!(read["dialogKind"], "prompt");
    let bounded = "m".repeat(MAX_DIALOG_TEXT_CHARS);
    assert_eq!(read["message"], bounded.as_str());
    assert_eq!(read["defaultText"], bounded.as_str());

    state.resolve(&thread_id, dialog_id, false);
    let read = serde_json::to_value(state.first_for_thread(&thread_id))
        .expect("approval should serialize");
    assert_eq!(read["kind"], "permission");
    assert_eq!(read["permission"], "camera");
    assert_eq!(read["origin"], ORIGIN);
}

#[test]
fn model_actions_dismiss_and_report_their_tabs_dialogs() {
    let mut state = PageRequestState::default();
    let thread_id = thread("thread:test");
    let waiting = open_dialog(
        &mut state,
        &thread_id,
        TAB,
        PageDialogKind::Confirm,
        "earlier",
    );
    let permission = ask_id(ask_permission(
        &mut state,
        &thread_id,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
    let other_tab = open_dialog(
        &mut state,
        &thread_id,
        OTHER_TAB,
        PageDialogKind::Alert,
        "other",
    );

    let (action, blocking) = state.begin_model_action(TAB);
    let DialogAdmission::Ask(waiting) = waiting else {
        panic!("dialog should have been shown before the action");
    };
    assert_eq!(blocking, vec![(waiting, thread_id.clone())]);

    let long = "d".repeat(MODEL_ACTION_DIALOG_MESSAGE_CHARS + 1);
    for _ in 0..MAX_MODEL_ACTION_DIALOGS + 2 {
        assert_eq!(
            open_dialog(&mut state, &thread_id, TAB, PageDialogKind::Alert, &long),
            DialogAdmission::Dismiss
        );
    }
    assert!(matches!(
        open_dialog(
            &mut state,
            &thread_id,
            OTHER_TAB,
            PageDialogKind::Alert,
            "user"
        ),
        DialogAdmission::Ask(_)
    ));

    let dialogs = state.finish_model_action(action);
    assert_eq!(dialogs.count as usize, MAX_MODEL_ACTION_DIALOGS + 3);
    assert_eq!(dialogs.dialogs.len(), MAX_MODEL_ACTION_DIALOGS);
    assert_eq!(dialogs.dialogs[0].kind, PageDialogKind::Confirm);
    assert_eq!(dialogs.dialogs[0].message, "earlier");
    assert_eq!(
        dialogs.dialogs[1].message.chars().count(),
        MODEL_ACTION_DIALOG_MESSAGE_CHARS
    );
    let result = dialogs.tool_result();
    assert_eq!(result["accepted"], true);
    assert_eq!(result["dismissedDialogCount"], MAX_MODEL_ACTION_DIALOGS + 3);

    assert!(matches!(
        open_dialog(&mut state, &thread_id, TAB, PageDialogKind::Alert, "after"),
        DialogAdmission::Ask(_)
    ));
    assert!(state.pending.contains_key(&permission));
    assert!(matches!(other_tab, DialogAdmission::Ask(_)));
    assert_eq!(
        PageRequestState::default()
            .finish_model_action(action)
            .tool_result(),
        serde_json::json!({ "accepted": true })
    );
}

#[test]
fn closing_tabs_and_threads_takes_only_their_pending_requests() {
    let mut state = PageRequestState::default();
    let first = thread("thread:first");
    let second = thread("thread:second");
    let first_tab = ask_id(ask_permission(
        &mut state,
        &first,
        TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
    let first_other_tab = ask_id(ask_permission(
        &mut state,
        &first,
        OTHER_TAB,
        ORIGIN,
        PagePermission::Camera,
    ));
    let second_tab = ask_id(ask_permission(
        &mut state,
        &second,
        "tab-3",
        ORIGIN,
        PagePermission::Camera,
    ));

    assert_eq!(
        state.take_pending(|_, tab| tab == TAB),
        vec![(first_tab, first.clone())]
    );
    assert_eq!(
        state.take_pending(|thread, _| thread == &first),
        vec![(first_other_tab, first.clone())]
    );
    assert_eq!(state.first_for_thread(&first), None);
    assert!(state.pending.contains_key(&second_tab));
}
