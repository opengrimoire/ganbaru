//! Positive guard for process closes on Linux: the processes that hold up the running graphical
//! session are computed from that session instead of from names.

use super::foreground::{x11_intern_atom, x11_property_u32};
use super::processes::{current_real_uid, linux_process_parent_id, linux_process_real_uid};
use super::*;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const SESSION_ANCHOR_TTL: Duration = Duration::from_secs(30);
const PROC_ROOT: &str = "/proc";
const X11_SOCKET_DIR: &str = "/tmp/.X11-unix";
/// Prefix `/proc/net/unix` gives abstract socket names.
const ABSTRACT_SOCKET_PREFIX: &str = "@";
/// `__SO_ACCEPTCON` in the `/proc/net/unix` flags column: the socket listens.
const UNIX_SOCKET_LISTENING: u32 = 0x0001_0000;
/// systemd slice that holds a session's services, as opposed to `app.slice` applications.
const SESSION_SLICE_SEGMENT: &str = "/session.slice/";

static CURRENT_ANCHORS: Mutex<Option<Arc<SessionAnchors>>> = Mutex::new(None);

/// Processes the running session depends on: display and session bus listeners, the X11 window
/// manager, the ancestors of each, the ancestors of Ganbaru, and session slice services.
///
/// Session leaders are not anchors, because launchers call `setsid` for ordinary applications.
pub(super) struct SessionAnchors {
    protected_pids: HashSet<u32>,
    collected_at: Instant,
}

/// Session environment values that name the display and session bus sockets.
#[derive(Clone, Debug, Default)]
pub(super) struct SessionVariables {
    pub(super) wayland_display: Option<String>,
    pub(super) xdg_runtime_dir: Option<String>,
    pub(super) display: Option<String>,
    pub(super) dbus_session_bus_address: Option<String>,
}

impl SessionVariables {
    fn from_environment() -> Self {
        let read = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self {
            wayland_display: read("WAYLAND_DISPLAY"),
            xdg_runtime_dir: read("XDG_RUNTIME_DIR"),
            display: read("DISPLAY"),
            dbus_session_bus_address: read("DBUS_SESSION_BUS_ADDRESS"),
        }
    }
}

/// Socket paths as `/proc/net/unix` prints them, with `@` marking abstract sockets.
pub(super) fn session_socket_paths(variables: &SessionVariables) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(display) = &variables.wayland_display {
        let display = Path::new(display);
        if display.is_absolute() {
            paths.push(display.to_string_lossy().into_owned());
        } else if let Some(runtime_dir) = &variables.xdg_runtime_dir {
            paths.push(
                Path::new(runtime_dir)
                    .join(display)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    if let Some(number) = variables.display.as_deref().and_then(x11_display_number) {
        let path = format!("{X11_SOCKET_DIR}/X{number}");
        let abstract_path = format!("{ABSTRACT_SOCKET_PREFIX}{path}");
        paths.extend([path, abstract_path]);
    }
    if let Some(address) = &variables.dbus_session_bus_address {
        paths.extend(address.split(';').filter_map(dbus_socket_path));
    }
    paths
}

/// Display number of a local `DISPLAY` value. A value with a host name goes over TCP and has no
/// local socket.
fn x11_display_number(display: &str) -> Option<&str> {
    let (host, display) = display.rsplit_once(':')?;
    if !matches!(host, "" | "unix") {
        return None;
    }
    let number = display.split('.').next()?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then_some(number)
}

/// Socket path of one D-Bus server address, such as `unix:path=/run/user/1000/bus,guid=abc`.
fn dbus_socket_path(address: &str) -> Option<String> {
    address
        .strip_prefix("unix:")?
        .split(',')
        .find_map(|key_value| {
            if let Some(path) = key_value.strip_prefix("path=") {
                return Some(path.to_string());
            }
            key_value
                .strip_prefix("abstract=")
                .map(|name| format!("{ABSTRACT_SOCKET_PREFIX}{name}"))
        })
}

/// Inodes of the sockets in a `/proc/net/unix` table that listen at one of `socket_paths`.
fn listening_socket_inodes(table: &str, socket_paths: &[String]) -> HashSet<u64> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = u32::from_str_radix(fields.nth(3)?, 16).ok()?;
            let inode = fields.nth(2)?.parse().ok()?;
            let path = fields.collect::<Vec<_>>().join(" ");
            (flags & UNIX_SOCKET_LISTENING != 0 && socket_paths.contains(&path)).then_some(inode)
        })
        .collect()
}

fn socket_inode(descriptor_target: &Path) -> Option<u64> {
    descriptor_target
        .to_str()?
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

fn holds_socket(process: &Path, inodes: &HashSet<u64>) -> bool {
    let Ok(descriptors) = std::fs::read_dir(process.join("fd")) else {
        return false;
    };
    descriptors.flatten().any(|descriptor| {
        std::fs::read_link(descriptor.path())
            .ok()
            .and_then(|target| socket_inode(&target))
            .is_some_and(|inode| inodes.contains(&inode))
    })
}

fn runs_in_session_slice(process: &Path) -> bool {
    std::fs::read_to_string(process.join("cgroup"))
        .is_ok_and(|cgroup| cgroup.contains(SESSION_SLICE_SEGMENT))
}

/// Process ID the X11 window manager on `display` publishes under EWMH: the root window's
/// `_NET_SUPPORTING_WM_CHECK` names a check window that names itself, and that window carries
/// `_NET_WM_PID`. An X11 window manager is a client, not a socket listener, so the socket walk
/// alone misses it. Returns `None` when the display cannot be reached or the manager publishes no
/// usable check window, which leaves the other anchors and the name list in place.
fn x11_window_manager_process_id(display: &str) -> Option<u32> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::AtomEnum;

    let (conn, screen_num) = x11rb::connect(Some(display)).ok()?;
    let root = conn.setup().roots.get(screen_num)?.root;
    let check_atom = x11_intern_atom(&conn, b"_NET_SUPPORTING_WM_CHECK").ok()?;
    let window_type = AtomEnum::WINDOW.into();
    let check_window = x11_property_u32(&conn, root, check_atom, window_type).ok()??;
    let named_by_check_window =
        x11_property_u32(&conn, check_window, check_atom, window_type).ok()??;
    if check_window == 0 || named_by_check_window != check_window {
        return None;
    }
    let pid_atom = x11_intern_atom(&conn, b"_NET_WM_PID").ok()?;
    x11_property_u32(&conn, check_window, pid_atom, AtomEnum::CARDINAL.into())
        .ok()?
        .filter(|process_id| *process_id > 1)
}

/// Adds `process_id` and its ancestors below PID 1. Reaching a process already in `visited`
/// means its ancestors are in too, which also ends a parent cycle.
fn add_with_ancestors(proc_root: &Path, process_id: u32, visited: &mut HashSet<u32>) {
    let mut current = process_id;
    while current > 1 && visited.insert(current) {
        let Some(parent) = linux_process_parent_id(&proc_root.join(current.to_string())) else {
            break;
        };
        current = parent;
    }
}

impl SessionAnchors {
    /// Returns `None` when no display socket or session bus can be resolved, which keeps the name
    /// list alone on minimal sessions, or when `is_cancelled` turns true.
    fn collect(is_cancelled: impl Fn() -> bool) -> Option<Self> {
        let variables = SessionVariables::from_environment();
        Self::collect_from(
            Path::new(PROC_ROOT),
            &session_socket_paths(&variables),
            current_real_uid()?,
            std::process::id(),
            variables
                .display
                .as_deref()
                .and_then(x11_window_manager_process_id),
            is_cancelled,
        )
    }

    /// Computes the anchors below `proc_root`, or returns `None` when neither a listening session
    /// socket nor a same-user window manager is found, or when `is_cancelled` turns true.
    pub(super) fn collect_from(
        proc_root: &Path,
        socket_paths: &[String],
        real_uid: u32,
        own_process_id: u32,
        window_manager_process_id: Option<u32>,
        is_cancelled: impl Fn() -> bool,
    ) -> Option<Self> {
        let listening = std::fs::read_to_string(proc_root.join("net/unix"))
            .map(|table| listening_socket_inodes(&table, socket_paths))
            .unwrap_or_default();
        let window_manager = window_manager_process_id.filter(|process_id| {
            linux_process_real_uid(&proc_root.join(process_id.to_string())) == Some(real_uid)
        });
        if listening.is_empty() && window_manager.is_none() {
            return None;
        }
        let mut listeners = Vec::new();
        let mut session_services = Vec::new();
        for entry in std::fs::read_dir(proc_root).ok()?.flatten() {
            if is_cancelled() {
                return None;
            }
            let Some(process_id) = entry.file_name().to_str().and_then(|id| id.parse().ok()) else {
                continue;
            };
            let process = entry.path();
            if linux_process_real_uid(&process) != Some(real_uid) {
                continue;
            }
            if !listening.is_empty() && holds_socket(&process, &listening) {
                listeners.push(process_id);
            }
            if runs_in_session_slice(&process) {
                session_services.push(process_id);
            }
        }
        let mut protected_pids = HashSet::new();
        for process_id in listeners
            .into_iter()
            .chain([own_process_id])
            .chain(window_manager)
        {
            add_with_ancestors(proc_root, process_id, &mut protected_pids);
        }
        protected_pids.extend(session_services);
        Some(Self {
            protected_pids,
            collected_at: Instant::now(),
        })
    }

    pub(super) fn protects(&self, process_id: u32) -> bool {
        self.protected_pids.contains(&process_id)
    }
}

/// Returns the anchors collected within the TTL, collecting them when none are. Callers invoke
/// this only once a rule found a candidate, so an idle observation pays nothing.
pub(super) fn current_session_anchors(
    is_cancelled: impl Fn() -> bool,
) -> Option<Arc<SessionAnchors>> {
    let mut current = CURRENT_ANCHORS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(anchors) = current
        .as_ref()
        .filter(|anchors| anchors.collected_at.elapsed() < SESSION_ANCHOR_TTL)
    {
        return Some(anchors.clone());
    }
    *current = SessionAnchors::collect(is_cancelled).map(Arc::new);
    current.clone()
}

/// Removes matches on anchored processes. With no anchors the name list stands alone.
pub(super) fn drop_anchored_matches(
    matches: &mut Vec<DistractionsRunningDesktopAppMatch>,
    anchors: Option<&SessionAnchors>,
) {
    if let Some(anchors) = anchors {
        matches.retain(|found| !anchors.protects(found.process_id));
    }
}

/// Refuses a close of an anchored process. With no anchors the name list stands alone.
pub(super) fn refuse_anchored_close(
    process_id: u32,
    anchors: Option<&SessionAnchors>,
) -> Result<(), String> {
    if anchors.is_some_and(|anchors| anchors.protects(process_id)) {
        return Err("refusing to close protected process".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const USER: u32 = 1000;
    const OTHER_USER: u32 = 1001;
    const LISTENING: u32 = 0x0001_0000;
    const WAYLAND_SOCKET: &str = "/run/user/1000/wayland-1";
    const USER_SLICE: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app.scope";
    const SESSION_SLICE: &str =
        "0::/user.slice/user-1000.slice/user@1000.service/session.slice/dbus.service";

    /// A `/proc` tree in a temporary folder, removed when dropped.
    struct ProcFixture {
        root: PathBuf,
    }

    impl ProcFixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "ganbaru-session-anchors-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("net")).unwrap();
            Self { root }
        }

        fn unix_sockets(&self, sockets: &[(u32, u64, &str)]) {
            let mut table = "Num RefCount Protocol Flags Type St Inode Path\n".to_string();
            for (flags, inode, path) in sockets {
                table.push_str(&format!(
                    "0000000000000000: 00000002 00000000 {flags:08X} 0001 01 {inode} {path}\n"
                ));
            }
            std::fs::write(self.root.join("net/unix"), table).unwrap();
        }

        fn process(&self, process_id: u32, parent_id: u32, uid: u32, cgroup: &str, inodes: &[u64]) {
            let path = self.root.join(process_id.to_string());
            std::fs::create_dir_all(path.join("fd")).unwrap();
            std::fs::write(
                path.join("stat"),
                format!("{process_id} (a (name) S {parent_id} {process_id} {process_id} 0\n"),
            )
            .unwrap();
            std::fs::write(
                path.join("status"),
                format!("Name:\tname\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .unwrap();
            std::fs::write(path.join("cgroup"), format!("{cgroup}\n")).unwrap();
            for (descriptor, inode) in inodes.iter().enumerate() {
                symlink(
                    format!("socket:[{inode}]"),
                    path.join("fd").join(descriptor.to_string()),
                )
                .unwrap();
            }
        }

        fn collect(&self, own_process_id: u32) -> Option<SessionAnchors> {
            SessionAnchors::collect_from(
                &self.root,
                &[WAYLAND_SOCKET.to_string()],
                USER,
                own_process_id,
                None,
                || false,
            )
        }
    }

    impl Drop for ProcFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn running_match(process_id: u32) -> DistractionsRunningDesktopAppMatch {
        DistractionsRunningDesktopAppMatch {
            app_name: "customwm".to_string(),
            process_name: "customwm".to_string(),
            process_id,
            process_identity: "identity".to_string(),
            rule_identity: DistractionsDesktopRuleIdentity::DesktopApp {
                rule_id: "customwm".to_string(),
            },
        }
    }

    /// A compositor whose name no list holds, under the user's service manager, with a terminal
    /// that launched Ganbaru and an unrelated application that called `setsid`.
    fn session_fixture(name: &str) -> ProcFixture {
        let fixture = ProcFixture::new(name);
        fixture.unix_sockets(&[
            (LISTENING, 9001, WAYLAND_SOCKET),
            (0, 9002, WAYLAND_SOCKET),
            (LISTENING, 9003, "/run/user/1000/other"),
        ]);
        fixture.process(100, 1, USER, USER_SLICE, &[]);
        fixture.process(200, 100, USER, USER_SLICE, &[9001, 9100]);
        fixture.process(300, 100, USER, USER_SLICE, &[9101]);
        fixture.process(350, 100, USER, USER_SLICE, &[]);
        fixture.process(400, 350, USER, USER_SLICE, &[]);
        fixture.process(450, 100, USER, SESSION_SLICE, &[]);
        fixture.process(460, 100, OTHER_USER, SESSION_SLICE, &[]);
        fixture.process(470, 100, USER, USER_SLICE, &[9002]);
        fixture.process(480, 100, USER, USER_SLICE, &[9003]);
        fixture
    }

    #[test]
    fn session_anchors_protect_listener_and_ancestors() {
        let fixture = session_fixture("listener");
        let anchors = fixture.collect(400);
        let protects = |process_id| anchors.as_ref().is_some_and(|set| set.protects(process_id));

        assert!(protects(200), "the display socket listener");
        assert!(protects(100), "the listener's ancestor");
        assert!(protects(350), "the terminal that launched Ganbaru");
        assert!(protects(450), "a session slice service");
        assert!(!protects(300), "an unrelated setsid application");
        assert!(!protects(460), "another user's process");
        assert!(!protects(470), "a holder of a socket that does not listen");
        assert!(!protects(480), "a listener at an unrelated path");
    }

    #[test]
    fn session_anchors_guard_listing_and_close() {
        let fixture = session_fixture("guard");
        let anchors = fixture.collect(400);
        let mut matches = vec![running_match(200), running_match(300)];

        drop_anchored_matches(&mut matches, anchors.as_ref());

        assert_eq!(
            matches
                .iter()
                .map(|found| found.process_id)
                .collect::<Vec<_>>(),
            vec![300]
        );
        assert!(refuse_anchored_close(200, anchors.as_ref()).is_err());
        assert!(refuse_anchored_close(300, anchors.as_ref()).is_ok());
    }

    #[test]
    fn missing_display_sockets_fall_back_to_name_list() {
        let fixture = session_fixture("fallback");
        let no_socket = SessionAnchors::collect_from(&fixture.root, &[], USER, 400, None, || false);
        assert!(no_socket.is_none());

        fixture.unix_sockets(&[(LISTENING, 9003, "/run/user/1000/other")]);
        let anchors = fixture.collect(400);
        assert!(anchors.is_none());

        let mut matches = vec![running_match(200)];
        drop_anchored_matches(&mut matches, anchors.as_ref());
        assert_eq!(matches.len(), 1);
        assert!(refuse_anchored_close(200, anchors.as_ref()).is_ok());
    }

    #[test]
    fn unreadable_socket_table_falls_back_to_name_list() {
        let fixture = session_fixture("no-table");
        std::fs::remove_file(fixture.root.join("net/unix")).unwrap();
        assert!(fixture.collect(400).is_none());

        let window_manager_only = SessionAnchors::collect_from(
            &fixture.root,
            &[WAYLAND_SOCKET.to_string()],
            USER,
            400,
            Some(300),
            || false,
        );
        assert!(window_manager_only.is_some_and(|set| set.protects(300) && !set.protects(200)));
    }

    #[test]
    fn cancelled_collection_yields_no_session_anchors() {
        let fixture = session_fixture("cancelled");
        let anchors = SessionAnchors::collect_from(
            &fixture.root,
            &[WAYLAND_SOCKET.to_string()],
            USER,
            400,
            None,
            || true,
        );
        assert!(anchors.is_none());
    }

    /// An X11 session started by `xinit`: the server runs as root, and a window manager whose
    /// name no list holds is a sibling of the server, not a socket listener, and Ganbaru was
    /// started outside the window manager's process tree.
    #[test]
    fn x11_window_manager_is_an_anchor() {
        const X11_SOCKET: &str = "/tmp/.X11-unix/X0";
        const ROOT: u32 = 0;
        let fixture = ProcFixture::new("x11-window-manager");
        fixture.unix_sockets(&[(LISTENING, 9201, X11_SOCKET)]);
        fixture.process(100, 1, USER, USER_SLICE, &[]);
        fixture.process(120, 100, USER, USER_SLICE, &[]);
        fixture.process(150, 120, ROOT, USER_SLICE, &[9201]);
        fixture.process(210, 120, USER, USER_SLICE, &[]);
        fixture.process(400, 1, USER, USER_SLICE, &[]);
        fixture.process(500, 1, USER, USER_SLICE, &[]);
        fixture.process(510, 1, OTHER_USER, USER_SLICE, &[]);
        let collect = |window_manager| {
            SessionAnchors::collect_from(
                &fixture.root,
                &[X11_SOCKET.to_string()],
                USER,
                400,
                window_manager,
                || false,
            )
        };

        let anchors = collect(Some(210));
        let protects = |process_id| anchors.as_ref().is_some_and(|set| set.protects(process_id));
        assert!(protects(210), "the window manager");
        assert!(protects(120), "xinit, the window manager's parent");
        assert!(!protects(500), "an unrelated setsid application");

        let foreign = collect(Some(510));
        assert!(
            !foreign.is_some_and(|set| set.protects(510)),
            "a window manager process owned by another user"
        );
    }

    #[test]
    fn session_anchors_ancestor_walk_ends_on_a_parent_cycle() {
        let fixture = ProcFixture::new("cycle");
        fixture.unix_sockets(&[(LISTENING, 9001, WAYLAND_SOCKET)]);
        fixture.process(600, 601, USER, USER_SLICE, &[9001]);
        fixture.process(601, 600, USER, USER_SLICE, &[]);

        let anchors = fixture.collect(601);

        assert!(anchors.is_some_and(|set| set.protects(601)));
    }

    #[test]
    fn session_socket_paths_cover_each_session_variable() {
        let paths = session_socket_paths(&SessionVariables {
            wayland_display: Some("wayland-1".to_string()),
            xdg_runtime_dir: Some("/run/user/1000".to_string()),
            display: Some(":1.0".to_string()),
            dbus_session_bus_address: Some(
                "unix:path=/run/user/1000/bus,guid=abc;unix:abstract=/tmp/dbus-x".to_string(),
            ),
        });
        assert_eq!(
            paths,
            vec![
                WAYLAND_SOCKET.to_string(),
                "/tmp/.X11-unix/X1".to_string(),
                "@/tmp/.X11-unix/X1".to_string(),
                "/run/user/1000/bus".to_string(),
                "@/tmp/dbus-x".to_string(),
            ]
        );
    }

    #[test]
    fn session_socket_paths_skip_values_with_no_local_socket() {
        let paths = session_socket_paths(&SessionVariables {
            wayland_display: Some("wayland-1".to_string()),
            xdg_runtime_dir: None,
            display: Some("remote.example:10.0".to_string()),
            dbus_session_bus_address: Some("tcp:host=remote.example,port=1".to_string()),
        });
        assert!(paths.is_empty());

        let absolute = session_socket_paths(&SessionVariables {
            wayland_display: Some("/custom/wayland".to_string()),
            ..SessionVariables::default()
        });
        assert_eq!(absolute, vec!["/custom/wayland".to_string()]);
    }
}
