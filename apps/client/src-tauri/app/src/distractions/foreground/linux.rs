use super::*;

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(target_os = "linux")]
fn x11_intern_atom<C: x11rb::connection::Connection>(conn: &C, name: &[u8]) -> Result<u32, String> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    conn.intern_atom(false, name)
        .map_err(|e| format!("intern X11 atom: {e}"))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|e| format!("read X11 atom: {e}"))
}

#[cfg(target_os = "linux")]
fn x11_property_u32<C: x11rb::connection::Connection>(
    conn: &C,
    window: u32,
    property: u32,
    type_: u32,
) -> Result<Option<u32>, String> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    let reply = conn
        .get_property(false, window, property, type_, 0, 1)
        .map_err(|e| format!("read X11 property: {e}"))?
        .reply()
        .map_err(|e| format!("read X11 property reply: {e}"))?;
    Ok(reply.value32().and_then(|mut values| values.next()))
}

#[cfg(target_os = "linux")]
fn x11_wm_class<C: x11rb::connection::Connection>(conn: &C, window: u32) -> Vec<String> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let Ok(cookie) =
        conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 4096)
    else {
        return Vec::new();
    };
    let Ok(reply) = cookie.reply() else {
        return Vec::new();
    };
    reply
        .value
        .split(|byte| *byte == 0)
        .filter_map(|part| std::str::from_utf8(part).ok())
        .filter_map(normalize_app_candidate_name)
        .collect()
}

#[cfg(target_os = "linux")]
fn x11_foreground_window_status() -> Result<DistractionsForegroundDesktopAppStatus, String> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::AtomEnum;

    let (conn, screen_num) = x11rb::connect(None).map_err(|e| format!("connect to X11: {e}"))?;
    let root = conn.setup().roots[screen_num].root;
    let active_window_atom = x11_intern_atom(&conn, b"_NET_ACTIVE_WINDOW")?;
    let Some(window) = x11_property_u32(&conn, root, active_window_atom, AtomEnum::WINDOW.into())?
    else {
        return Err("no active X11 window is available".to_string());
    };
    if window == 0 {
        return Err("no active X11 window is available".to_string());
    }
    x11_window_status(&conn, window)
}

#[cfg(target_os = "linux")]
fn x11_window_status<C: x11rb::connection::Connection>(
    conn: &C,
    window: u32,
) -> Result<DistractionsForegroundDesktopAppStatus, String> {
    use x11rb::protocol::xproto::AtomEnum;

    let pid_atom = x11_intern_atom(conn, b"_NET_WM_PID")?;
    let process_id = x11_property_u32(conn, window, pid_atom, AtomEnum::CARDINAL.into())?;
    let wm_class_names = x11_wm_class(conn, window);
    let process_names = process_id
        .map(|id| read_linux_process_names(&PathBuf::from("/proc").join(id.to_string())))
        .unwrap_or_default();
    let app_name = wm_class_names
        .last()
        .cloned()
        .or_else(|| process_names.first().cloned())
        .ok_or_else(|| "active X11 window app name is unavailable".to_string())?;
    let process_name = process_names.first().cloned();
    let mut match_names = Vec::new();
    match_names.extend(wm_class_names);
    match_names.extend(process_names);
    let mut status = foreground_status_from_parts(app_name, process_name, process_id, match_names);
    status.process_identity = process_id
        .and_then(|id| observe_linux_process(&PathBuf::from("/proc").join(id.to_string()), id))
        .map(|observed| observed.process_identity);
    Ok(status)
}

#[cfg(target_os = "linux")]
fn x11_close_active_window(
    expected: DistractionsForegroundDesktopAppExpectation,
    authorize: &mut dyn FnMut(&DistractionsForegroundDesktopAppStatus) -> Result<(), String>,
) -> Result<(), String> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask,
    };

    let (conn, screen_num) = x11rb::connect(None).map_err(|e| format!("connect to X11: {e}"))?;
    let root = conn.setup().roots[screen_num].root;
    let active_window_atom = x11_intern_atom(&conn, b"_NET_ACTIVE_WINDOW")?;
    let Some(window) = x11_property_u32(&conn, root, active_window_atom, AtomEnum::WINDOW.into())?
    else {
        return Err("no active X11 window is available".to_string());
    };
    let status = x11_window_status(&conn, window)?;
    validate_foreground_status_is_closeable(&status)?;
    if !foreground_expectation_matches(&status, &expected) {
        return Err("foreground app changed before it could be closed".to_string());
    }
    authorize(&status)?;
    let close_atom = x11_intern_atom(&conn, b"_NET_CLOSE_WINDOW")?;
    let event = ClientMessageEvent::new(
        32,
        window,
        close_atom,
        ClientMessageData::from([0u32, 2, 0, 0, 0]),
    );
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .map_err(|e| format!("send X11 close request: {e}"))?;
    conn.flush()
        .map_err(|e| format!("flush X11 close request: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct WaylandToplevelInfo {
    handle: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
    app_id: Option<String>,
    active: bool,
    closed: bool,
}

#[cfg(target_os = "linux")]
struct WaylandToplevelState {
    manager: Option<wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1>,
    toplevels: HashMap<wayland_client::backend::ObjectId, WaylandToplevelInfo>,
    finished: bool,
}

#[cfg(target_os = "linux")]
fn wayland_state_is_activated(state: &[u8]) -> bool {
    const ACTIVATED_STATE: u32 = 2;
    state
        .as_chunks::<4>()
        .0
        .iter()
        .any(|chunk| u32::from_ne_bytes(*chunk) == ACTIVATED_STATE)
}

#[cfg(target_os = "linux")]
fn wayland_status_from_toplevel(
    info: &WaylandToplevelInfo,
) -> Option<DistractionsForegroundDesktopAppStatus> {
    let app_name = info.app_id.clone()?;
    let mut match_names = Vec::new();
    if let Some(app_id) = &info.app_id {
        match_names.push(app_id.clone());
    }
    Some(foreground_status_from_parts(
        app_name,
        info.app_id.clone(),
        None,
        match_names,
    ))
}

#[cfg(target_os = "linux")]
impl WaylandToplevelState {
    fn active_toplevel(&self) -> Option<&WaylandToplevelInfo> {
        self.toplevels
            .values()
            .find(|info| info.active && !info.closed && info.app_id.is_some())
    }
}

#[cfg(target_os = "linux")]
impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()>
    for WaylandToplevelState
{
    fn event(
        state: &mut Self,
        registry: &wayland_client::protocol::wl_registry::WlRegistry,
        event: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "zwlr_foreign_toplevel_manager_v1" {
                let manager = registry.bind::<
                    wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
                    _,
                    _,
                >(name, version.min(3), qh, ());
                state.manager = Some(manager);
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl wayland_client::Dispatch<
    wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
    (),
> for WaylandToplevelState {
    fn event(
        state: &mut Self,
        _: &wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
        event: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        use wayland_client::Proxy as _;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::Event;

        match event {
            Event::Toplevel { toplevel } => {
                state.toplevels.insert(
                    toplevel.id(),
                    WaylandToplevelInfo {
                        handle: toplevel,
                        app_id: None,
                        active: false,
                        closed: false,
                    },
                );
            }
            Event::Finished => state.finished = true,
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
impl wayland_client::Dispatch<
    wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
    (),
> for WaylandToplevelState {
    fn event(
        state: &mut Self,
        toplevel: &wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
        event: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        use wayland_client::Proxy as _;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::Event;

        let key = toplevel.id();
        let Some(info) = state.toplevels.get_mut(&key) else {
            return;
        };
        match event {
            Event::AppId { app_id } => info.app_id = normalize_app_candidate_name(&app_id),
            Event::State { state: raw_state } => {
                info.active = wayland_state_is_activated(&raw_state);
            }
            Event::Closed => info.closed = true,
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
wayland_client::delegate_noop!(WaylandToplevelState: ignore wayland_client::protocol::wl_output::WlOutput);

#[cfg(target_os = "linux")]
fn with_wayland_toplevel_state<T>(
    action: impl FnOnce(&wayland_client::Connection, &mut WaylandToplevelState) -> Result<T, String>,
) -> Result<T, String> {
    let conn = wayland_client::Connection::connect_to_env()
        .map_err(|e| format!("connect to Wayland: {e}"))?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = WaylandToplevelState {
        manager: None,
        toplevels: HashMap::new(),
        finished: false,
    };
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| format!("read Wayland globals: {e}"))?;
    if state.manager.is_none() {
        return Err(
            "Wayland compositor does not advertise zwlr_foreign_toplevel_manager_v1".to_string(),
        );
    }
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| format!("read Wayland toplevels: {e}"))?;
    action(&conn, &mut state)
}

#[cfg(target_os = "linux")]
fn wayland_foreground_window_status() -> Result<DistractionsForegroundDesktopAppStatus, String> {
    with_wayland_toplevel_state(|_, state| {
        let Some(info) = state.active_toplevel() else {
            return Err("no active Wayland toplevel is available".to_string());
        };
        wayland_status_from_toplevel(info)
            .ok_or_else(|| "active Wayland toplevel app name is unavailable".to_string())
    })
}

#[cfg(target_os = "linux")]
fn wayland_close_active_window(
    expected: DistractionsForegroundDesktopAppExpectation,
    authorize: &mut dyn FnMut(&DistractionsForegroundDesktopAppStatus) -> Result<(), String>,
) -> Result<(), String> {
    with_wayland_toplevel_state(|conn, state| {
        let Some(info) = state.active_toplevel().cloned() else {
            return Err("no active Wayland toplevel is available".to_string());
        };
        let status = wayland_status_from_toplevel(&info)
            .ok_or_else(|| "active Wayland toplevel app name is unavailable".to_string())?;
        validate_foreground_status_is_closeable(&status)?;
        if !foreground_expectation_matches(&status, &expected) {
            return Err("foreground app changed before it could be closed".to_string());
        }
        authorize(&status)?;
        info.handle.close();
        conn.flush()
            .map_err(|e| format!("flush Wayland close request: {e}"))?;
        Ok(())
    })
}

#[cfg(target_os = "linux")]
pub(in crate::distractions) fn foreground_desktop_app_status()
-> DistractionsForegroundDesktopAppStatus {
    if is_wayland_session() {
        return wayland_foreground_window_status()
            .unwrap_or_else(unavailable_foreground_desktop_app_status);
    }
    if std::env::var_os("DISPLAY").is_some() {
        return x11_foreground_window_status()
            .unwrap_or_else(unavailable_foreground_desktop_app_status);
    }
    unavailable_foreground_desktop_app_status(
        "foreground desktop app detection needs X11 or a Wayland compositor with zwlr_foreign_toplevel_manager_v1",
    )
}

#[cfg(target_os = "linux")]
pub(in crate::distractions) fn close_current_foreground_desktop_app(
    expected: DistractionsForegroundDesktopAppExpectation,
    authorize: &mut dyn FnMut(&DistractionsForegroundDesktopAppStatus) -> Result<(), String>,
) -> Result<(), String> {
    if is_wayland_session() {
        return wayland_close_active_window(expected, authorize);
    }
    if std::env::var_os("DISPLAY").is_some() {
        return x11_close_active_window(expected, authorize);
    }
    Err("foreground desktop app closing needs X11 or a Wayland compositor with zwlr_foreign_toplevel_manager_v1".to_string())
}
