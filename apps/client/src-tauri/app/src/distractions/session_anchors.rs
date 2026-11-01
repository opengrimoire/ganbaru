//! Positive guard for process closes on Linux: the processes that hold up the running graphical
//! session are computed from that session instead of from names.

use super::*;

/// Processes the running session depends on.
pub(super) struct SessionAnchors {
    protected_pids: HashSet<u32>,
}

/// Session environment values that name the display and session bus sockets.
#[derive(Clone, Debug, Default)]
pub(super) struct SessionVariables {
    pub(super) wayland_display: Option<String>,
    pub(super) xdg_runtime_dir: Option<String>,
    pub(super) display: Option<String>,
    pub(super) dbus_session_bus_address: Option<String>,
}

/// Socket paths as `/proc/net/unix` prints them, with `@` marking abstract sockets.
pub(super) fn session_socket_paths(_variables: &SessionVariables) -> Vec<String> {
    Vec::new()
}

impl SessionAnchors {
    /// Computes the anchors below `proc_root`, or returns `None` when no listening session socket
    /// is found or `is_cancelled` turns true.
    pub(super) fn collect_from(
        _proc_root: &Path,
        _socket_paths: &[String],
        _real_uid: u32,
        _own_process_id: u32,
        _is_cancelled: impl Fn() -> bool,
    ) -> Option<Self> {
        None
    }

    pub(super) fn protects(&self, process_id: u32) -> bool {
        self.protected_pids.contains(&process_id)
    }
}

/// Removes matches on anchored processes. With no anchors the name list stands alone.
pub(super) fn drop_anchored_matches(
    _matches: &mut Vec<DistractionsRunningDesktopAppMatch>,
    _anchors: Option<&SessionAnchors>,
) {
}

/// Refuses a close of an anchored process. With no anchors the name list stands alone.
pub(super) fn refuse_anchored_close(
    _process_id: u32,
    _anchors: Option<&SessionAnchors>,
) -> Result<(), String> {
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
        let protects =
            |process_id| anchors.as_ref().is_some_and(|set| set.protects(process_id));

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
        let no_socket =
            SessionAnchors::collect_from(&fixture.root, &[], USER, 400, || false);
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
    }

    #[test]
    fn cancelled_collection_yields_no_session_anchors() {
        let fixture = session_fixture("cancelled");
        let anchors = SessionAnchors::collect_from(
            &fixture.root,
            &[WAYLAND_SOCKET.to_string()],
            USER,
            400,
            || true,
        );
        assert!(anchors.is_none());
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
