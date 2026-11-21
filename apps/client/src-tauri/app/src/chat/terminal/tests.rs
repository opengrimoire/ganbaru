use super::*;

fn session(running: bool) -> Arc<TerminalSession> {
    session_at("terminal:test", "channel:test", "workspace:test", running)
}

fn session_at(
    id: &str,
    channel_id: &str,
    working_folder_id: &str,
    running: bool,
) -> Arc<TerminalSession> {
    Arc::new(TerminalSession {
        id: id.to_string(),
        channel_id: ChatChannelId::new(channel_id).expect("channel ID should be valid"),
        reply_thread_id: None,
        working_folder_id: ProjectWorkingFolderId::new(working_folder_id)
            .expect("workspace ID should be valid"),
        state: Mutex::new(TerminalState {
            name: "Terminal 1".to_string(),
            shell: "test-shell".to_string(),
            columns: 80,
            rows: 24,
            running,
            exit_code: None,
            generation: 1,
            sequence: 0,
            replay_bytes: 0,
            replay: VecDeque::new(),
        }),
        master: Mutex::new(None),
        writer: Mutex::new(None),
        killer: Mutex::new(None),
        subscribers: Mutex::new(OutputSubscribers::default()),
        output_credit: Condvar::new(),
    })
}

#[test]
fn replay_is_sequenced_generation_scoped_and_byte_bounded() {
    let session = session(true);
    assert!(
        session
            .record_output(0, b"stale".to_vec())
            .expect("stale output should be handled")
            .is_none()
    );
    for byte in [1_u8, 2, 3] {
        session
            .record_output(1, vec![byte; MAX_REPLAY_BYTES / 2])
            .expect("output should record")
            .expect("current generation should emit");
    }
    let snapshot = session.snapshot().expect("snapshot should read");
    assert_eq!(snapshot.terminal.last_sequence, 3);
    assert_eq!(snapshot.scrollback.len(), 2);
    assert_eq!(snapshot.scrollback[0].sequence, 2);
    assert!(
        session
            .state
            .lock()
            .expect("terminal state should lock")
            .replay_bytes
            <= MAX_REPLAY_BYTES
    );
}

fn decoded(chunk: &ChatTerminalOutputChunk) -> Vec<u8> {
    general_purpose::STANDARD
        .decode(&chunk.data_base64)
        .expect("chunk data should decode")
}

#[test]
fn replay_starts_with_the_mode_reset_and_live_output_does_not() {
    let session = session(true);
    assert!(
        session
            .snapshot()
            .expect("empty snapshot should read")
            .scrollback
            .is_empty()
    );
    let live = session
        .record_output(1, b"\x1b[?1049hvim".to_vec())
        .expect("output should record")
        .expect("current generation should emit");
    assert_eq!(decoded(&live), b"\x1b[?1049hvim");
    session
        .record_output(1, b"second".to_vec())
        .expect("output should record");

    let snapshot = session.snapshot().expect("snapshot should read");
    assert_eq!(snapshot.scrollback.len(), 2);
    assert_eq!(snapshot.scrollback[0].sequence, 1);
    let mut expected = TERMINAL_REPLAY_RESET_PREFIX.as_bytes().to_vec();
    expected.extend_from_slice(b"\x1b[?1049hvim");
    assert_eq!(decoded(&snapshot.scrollback[0]), expected);
    assert_eq!(decoded(&snapshot.scrollback[1]), b"second");
    assert!(TERMINAL_REPLAY_RESET_PREFIX.starts_with("\x1b[?1049l"));
    assert!(TERMINAL_REPLAY_RESET_PREFIX.ends_with("\x1b[0m"));

    for byte in [1_u8, 2, 3] {
        session
            .record_output(1, vec![byte; MAX_REPLAY_BYTES / 2])
            .expect("output should record");
    }
    let trimmed = session.snapshot().expect("snapshot should read");
    let first = decoded(&trimmed.scrollback[0]);
    assert!(first.starts_with(TERMINAL_REPLAY_RESET_PREFIX.as_bytes()));
    assert_eq!(
        first.len(),
        TERMINAL_REPLAY_RESET_PREFIX.len() + MAX_REPLAY_BYTES / 2
    );
    assert!(
        session
            .state
            .lock()
            .expect("terminal state should lock")
            .replay_bytes
            <= MAX_REPLAY_BYTES
    );
}

#[test]
fn running_terminal_close_requires_confirmation_and_removes_exact_session() {
    let registry = ChatTerminalRegistry::default();
    registry
        .sessions
        .lock()
        .expect("registry should lock")
        .insert("terminal:test".to_string(), session(true));
    let first = registry
        .close("terminal:test", false)
        .expect("close should respond");
    assert!(first.confirmation_required);
    assert!(!first.closed);
    let confirmed = registry
        .close("terminal:test", true)
        .expect("confirmed close should work");
    assert!(confirmed.closed);
    assert!(registry.snapshot("terminal:test").is_err());
}

#[test]
fn terminals_belong_to_one_channel_and_working_folder() {
    let registry = ChatTerminalRegistry::default();
    let sessions = [
        session_at("terminal:a-root", "channel:a", "workspace:root", true),
        session_at("terminal:a-docs", "channel:a", "workspace:docs", true),
        session_at("terminal:b-root", "channel:b", "workspace:root", true),
    ];
    {
        let mut registered = registry.sessions.lock().expect("registry should lock");
        for session in &sessions {
            registered.insert(session.id.clone(), Arc::clone(session));
        }
    }
    let channel = |id: &str| ChatChannelId::new(id).expect("channel ID should be valid");
    let folder = |id: &str| ProjectWorkingFolderId::new(id).expect("workspace ID should be valid");

    let listed = registry
        .list(&channel("channel:a"), &folder("workspace:root"))
        .expect("list should work")
        .into_iter()
        .map(|terminal| terminal.id)
        .collect::<Vec<_>>();
    assert_eq!(listed, ["terminal:a-root"]);

    assert!(
        registry
            .require_scope(
                "terminal:a-root",
                &channel("channel:a"),
                &folder("workspace:root")
            )
            .is_ok()
    );
    for (channel_id, folder_id) in [
        ("channel:b", "workspace:root"),
        ("channel:a", "workspace:docs"),
    ] {
        let refused = registry
            .require_scope("terminal:a-root", &channel(channel_id), &folder(folder_id))
            .expect_err("a terminal must not be reachable from another channel or folder");
        assert_eq!(refused.code, ChatErrorCode::Permission);
    }
}

#[test]
fn reply_thread_tabs_list_only_their_own_terminals_of_the_channel() {
    let thread = |id: &str| ChatReplyThreadId::new(id).expect("reply thread ID should be valid");
    let in_thread = |id: &str, reply_thread_id: &str| {
        let mut session = Arc::into_inner(session_at(id, "channel:a", "workspace:root", true))
            .expect("a fresh session has one owner");
        session.reply_thread_id = Some(thread(reply_thread_id));
        Arc::new(session)
    };
    let registry = ChatTerminalRegistry::default();
    let sessions = [
        session_at("terminal:channel", "channel:a", "workspace:root", true),
        in_thread("terminal:thread-a", "reply:a"),
        in_thread("terminal:thread-b", "reply:b"),
    ];
    {
        let mut registered = registry.sessions.lock().expect("registry should lock");
        for session in &sessions {
            registered.insert(session.id.clone(), Arc::clone(session));
        }
    }
    let channel = ChatChannelId::new("channel:a").expect("channel ID should be valid");
    let folder =
        ProjectWorkingFolderId::new("workspace:root").expect("workspace ID should be valid");
    let ids = |terminals: Vec<ChatTerminalRead>| {
        terminals
            .into_iter()
            .map(|terminal| terminal.id)
            .collect::<Vec<_>>()
    };

    let channel_panel = registry
        .list_for_panel(&channel, &folder, None)
        .expect("channel panel list should work");
    assert_eq!(ids(channel_panel), ["terminal:channel"]);
    let thread_panel = registry
        .list_for_panel(&channel, &folder, Some(&thread("reply:a")))
        .expect("thread panel list should work");
    assert_eq!(ids(thread_panel), ["terminal:thread-a"]);
    let every = registry
        .list(&channel, &folder)
        .expect("full list should work");
    assert_eq!(ids(every).len(), 3);
    assert!(
        registry
            .require_scope("terminal:thread-b", &channel, &folder)
            .is_ok(),
        "the channel still authorizes terminals opened from its reply threads"
    );
}

#[test]
fn shutting_down_a_folder_stops_its_terminals_in_every_channel() {
    let registry = ChatTerminalRegistry::default();
    let sessions = [
        session_at("terminal:a", "channel:a", "workspace:archived", true),
        session_at("terminal:b", "channel:b", "workspace:archived", true),
        session_at("terminal:other", "channel:a", "workspace:other", true),
    ];
    {
        let mut registered = registry.sessions.lock().expect("registry should lock");
        for session in &sessions {
            registered.insert(session.id.clone(), Arc::clone(session));
        }
    }

    registry
        .shutdown_workspace(
            &ProjectWorkingFolderId::new("workspace:archived")
                .expect("workspace ID should be valid"),
        )
        .expect("shutdown should work");

    for (index, id) in ["terminal:a", "terminal:b"].iter().enumerate() {
        assert!(registry.snapshot(id).is_err());
        assert!(!sessions[index].read().expect("session should read").running);
    }
    assert!(
        registry
            .snapshot("terminal:other")
            .expect("terminal should remain")
            .terminal
            .running
    );
}

struct RecordingSink {
    delivered: Arc<Mutex<Vec<u64>>>,
    open: bool,
}

impl TerminalOutputSink for RecordingSink {
    fn deliver(&self, chunk: &ChatTerminalOutputChunk) -> bool {
        if self.open {
            self.delivered
                .lock()
                .expect("sink should lock")
                .push(chunk.sequence);
        }
        self.open
    }
}

fn recording_sink(open: bool) -> (Box<dyn TerminalOutputSink>, Arc<Mutex<Vec<u64>>>) {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingSink {
        delivered: Arc::clone(&delivered),
        open,
    };
    (Box::new(sink), delivered)
}

fn output_chunk(sequence: u64) -> ChatTerminalOutputChunk {
    ChatTerminalOutputChunk {
        terminal_id: "terminal:test".to_string(),
        generation: 1,
        sequence,
        data_base64: String::new(),
        replay: false,
    }
}

#[test]
fn output_reaches_only_attached_views_and_pauses_at_the_watermark() {
    let session = session(true);
    session
        .record_output(1, b"before".to_vec())
        .expect("output should record");
    let (sink, delivered) = recording_sink(true);
    let snapshot = session
        .attach_output("view:a".to_string(), sink)
        .expect("view should attach");
    assert_eq!(snapshot.scrollback.len(), 1);

    let half = TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES / 2;
    for sequence in 2..=4 {
        session
            .publish_output(&output_chunk(sequence), half)
            .expect("output should publish");
    }
    assert_eq!(
        *delivered.lock().expect("sink should lock"),
        vec![2, 3],
        "a full view skips live chunks instead of queueing them"
    );
    assert!(
        session
            .subscribers
            .lock()
            .expect("subscribers should lock")
            .should_wait()
    );

    session
        .acknowledge_output("view:a", half)
        .expect("acknowledgement should apply");
    session
        .publish_output(&output_chunk(5), half)
        .expect("output should publish");
    assert_eq!(*delivered.lock().expect("sink should lock"), vec![2, 3, 5]);
    assert_eq!(
        session
            .acknowledge_output("view:unknown", half)
            .expect_err("unknown subscriptions are rejected")
            .code,
        ChatErrorCode::NotFound
    );

    session.detach_output("view:a").expect("view should detach");
    session
        .publish_output(&output_chunk(6), 1)
        .expect("output should publish");
    assert_eq!(delivered.lock().expect("sink should lock").len(), 3);
}

#[test]
fn stalled_views_stop_holding_the_shell_and_closed_views_are_dropped() {
    let mut subscribers = OutputSubscribers::default();
    let (stalled, _) = recording_sink(true);
    subscribers.attach("view:stalled".to_string(), stalled);
    subscribers.deliver(&output_chunk(1), TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES);
    assert!(subscribers.should_wait());
    subscribers.mark_full_as_lagging();
    assert!(
        !subscribers.should_wait(),
        "a lagging view never holds the reader again"
    );

    let (responsive, responsive_chunks) = recording_sink(true);
    subscribers.attach("view:responsive".to_string(), responsive);
    assert!(!subscribers.should_wait());
    subscribers.deliver(&output_chunk(2), 1);
    assert_eq!(
        *responsive_chunks.lock().expect("sink should lock"),
        vec![2]
    );
    assert!(subscribers.acknowledge("view:stalled", TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES));
    assert!(subscribers.entries.iter().all(|entry| !entry.lagging));

    let (closed, _) = recording_sink(false);
    subscribers.attach("view:closed".to_string(), closed);
    subscribers.deliver(&output_chunk(3), 1);
    assert!(
        subscribers
            .entries
            .iter()
            .all(|entry| entry.id != "view:closed")
    );

    for index in 0..MAX_TERMINAL_OUTPUT_SUBSCRIBERS {
        let (sink, _) = recording_sink(true);
        subscribers.attach(format!("view:{index}"), sink);
    }
    assert_eq!(subscribers.entries.len(), MAX_TERMINAL_OUTPUT_SUBSCRIBERS);
    assert!(
        subscribers
            .entries
            .iter()
            .all(|entry| entry.id != "view:stalled")
    );
    assert!(validate_subscription_id("view:1").is_ok());
    assert!(validate_subscription_id("").is_err());
    assert!(validate_subscription_id("view one").is_err());
    assert!(validate_subscription_id(&"a".repeat(MAX_TERMINAL_SUBSCRIPTION_ID_BYTES + 1)).is_err());
}

#[test]
fn vault_suspension_blocks_creation_and_counts_only_running_terminals() {
    let registry = ChatTerminalRegistry::default();
    {
        let mut sessions = registry.sessions.lock().expect("registry should lock");
        sessions.insert("terminal:running".to_string(), session(true));
        sessions.insert("terminal:exited".to_string(), session(false));
    }
    assert_eq!(registry.live_count().expect("count should read"), 1);
    assert!(registry.admit_creation().is_ok());

    registry
        .suspend_creation()
        .expect("suspension should apply");
    let paused = registry
        .admit_creation()
        .expect_err("a suspended registry must reject new terminals");
    assert_eq!(paused.code, ChatErrorCode::Busy);
    assert_eq!(registry.live_count().expect("count should read"), 1);

    registry.resume_creation();
    assert!(registry.admit_creation().is_ok());
    registry.shutdown_all().expect("shutdown should apply");
    registry.resume_creation();
    assert!(registry.admit_creation().is_err());
}

#[test]
fn terminal_validation_rejects_unsafe_dimensions_and_input() {
    assert!(validate_dimensions(19, 24).is_err());
    assert!(validate_dimensions(80, 1).is_err());
    assert!(validate_dimensions(80, 24).is_ok());
    let registry = ChatTerminalRegistry::default();
    registry
        .sessions
        .lock()
        .expect("registry should lock")
        .insert("terminal:test".to_string(), session(true));
    assert!(registry.input("terminal:test", "bad\0input").is_err());
    assert!(
        registry
            .input("terminal:test", "pasted\x1b[201~rm -rf ~\r")
            .is_err()
    );
    assert!(
        registry
            .input("terminal:test", "\x1b[200~a\x1b[201~b\x1b[201~")
            .is_err()
    );
    assert!(
        registry
            .input("terminal:test", "\x1b[200~a\x1b[200~b\x1b[201~")
            .is_err()
    );
    for keys in ["\x1b[20~", "\x1b[21~", "\x1b[2~", "\x1b[2;5~", "\x1b[20;2~"] {
        assert!(
            validate_paste_framing(keys).is_ok(),
            "{keys:?} is a key sequence"
        );
    }
    assert!(validate_paste_framing("\x1b[200~echo one\recho two\x1b[201~").is_ok());
}

#[test]
fn shell_command_starts_in_the_workspace_and_advertises_a_color_terminal() {
    let workspace = PathBuf::from("workspace");
    let command = shell_command(&workspace);
    assert_eq!(command.get_cwd().map(PathBuf::from), Some(workspace));
    assert_eq!(command.get_env("COLORTERM"), Some("truecolor".as_ref()));
    if std::env::var_os(FISH_FEATURES_VARIABLE).is_none() {
        assert_eq!(
            command.get_env(FISH_FEATURES_VARIABLE),
            Some(FISH_NO_QUERY_TERM_FEATURE.as_ref())
        );
    }
    for name in TERMINAL_ENVIRONMENT_BLOCKLIST {
        assert_eq!(
            command.get_env(name),
            None,
            "{name} should not be inherited"
        );
    }
    #[cfg(not(windows))]
    assert_eq!(command.get_env("TERM"), Some("xterm-256color".as_ref()));
    #[cfg(windows)]
    assert!(Path::new(&command.get_argv()[0]).is_absolute());
}

#[test]
fn windows_shells_resolve_by_absolute_path_and_never_by_a_bare_name() {
    let root = std::env::temp_dir();
    let custom = root.join("shells").join("custom.exe");
    let system_shell = root.join("System32").join("cmd.exe");
    assert_eq!(
        absolute_windows_shell(Some(custom.as_os_str()), Some(root.as_os_str())),
        Some(custom)
    );
    assert_eq!(
        absolute_windows_shell(Some(OsStr::new("cmd.exe")), Some(root.as_os_str())),
        Some(system_shell.clone())
    );
    assert_eq!(
        absolute_windows_shell(None, Some(root.as_os_str())),
        Some(system_shell)
    );
    assert_eq!(
        absolute_windows_shell(Some(OsStr::new("cmd.exe")), Some(OsStr::new("Windows"))),
        None
    );
    assert_eq!(absolute_windows_shell(None, None), None);
}

fn environment(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

#[test]
fn terminals_inherit_the_launch_environment_instead_of_later_process_changes() {
    let launch = environment(&[
        ("SSH_AUTH_SOCK", "/run/user/1000/agent"),
        ("PATH", "/usr/bin"),
    ]);
    let current = environment(&[
        ("SSH_AUTH_SOCK", "/run/user/1000/agent"),
        ("PATH", "/opt/bundle/bin:/usr/bin"),
        ("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt"),
    ]);
    assert_eq!(
        environment_edits(&launch, &current),
        vec![
            EnvironmentEdit::Set("PATH".into(), "/usr/bin".into()),
            EnvironmentEdit::Remove("SSL_CERT_FILE".into()),
        ]
    );
    assert!(environment_edits(&launch, &launch).is_empty());
    assert_eq!(
        environment_edits(&launch, &environment(&[("PATH", "/usr/bin")])),
        vec![EnvironmentEdit::Set(
            "SSH_AUTH_SOCK".into(),
            "/run/user/1000/agent".into()
        )]
    );
}

#[test]
fn terminals_drop_multiplexer_and_port_variables_and_keep_the_rest() {
    let launch = environment(&[
        ("TMUX", "/tmp/tmux-1000/default,1234,0"),
        ("TMUX_PANE", "%3"),
        ("STY", "1234.pts-0.host"),
        ("WINDOW", "0"),
        ("PORT", "5173"),
        ("ZELLIJ", "0"),
        ("ZELLIJ_SESSION_NAME", "work"),
        ("EDITOR", "nvim"),
        ("PATH", "/usr/bin"),
        ("TMUXINATOR_CONFIG", "/home/user/.tmuxinator"),
        ("REPORT", "weekly"),
    ]);
    assert_eq!(
        without_blocklisted_variables(launch),
        environment(&[
            ("EDITOR", "nvim"),
            ("PATH", "/usr/bin"),
            ("TMUXINATOR_CONFIG", "/home/user/.tmuxinator"),
            ("REPORT", "weekly"),
        ])
    );
}

#[test]
fn fish_skips_the_terminal_query_unless_the_user_chose_its_behavior() {
    let fish_features = |launch: &[(&str, &str)]| {
        terminal_environment(environment(launch))
            .into_iter()
            .find(|(key, _)| key == FISH_FEATURES_VARIABLE)
            .map(|(_, value)| value)
    };
    assert_eq!(
        fish_features(&[("EDITOR", "nvim")]),
        Some("no-query-term".into())
    );
    assert_eq!(
        fish_features(&[("fish_features", "")]),
        Some("no-query-term".into())
    );
    assert_eq!(
        fish_features(&[("fish_features", "qmark-noglob")]),
        Some("qmark-noglob,no-query-term".into())
    );
    for chosen in [
        "query-term",
        "no-query-term",
        "qmark-noglob,query-term",
        "qmark-noglob no-query-term",
    ] {
        assert_eq!(
            fish_features(&[("fish_features", chosen)]),
            Some(chosen.into()),
            "{chosen} is the user's choice"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn terminals_drop_only_what_the_appimage_runtime_injects() {
    let user = environment(&[
        ("PATH", "/tmp/.mount_gan/usr/bin:/usr/bin"),
        ("GTK_PATH", "/tmp/.mount_gan/usr/lib/gtk-3.0"),
        ("EDITOR", "nvim"),
    ]);
    assert_eq!(
        terminal_environment(user.clone()),
        with_fish_query_term_disabled(user)
    );

    let packaged = environment(&[
        ("APPIMAGE", "/home/user/Ganbaru.AppImage"),
        ("APPDIR", "/tmp/.mount_gan"),
        ("ARGV0", "./Ganbaru.AppImage"),
        ("OWD", "/home/user"),
        ("PATH", "/tmp/.mount_gan/usr/bin:/usr/local/bin:/usr/bin"),
        ("LD_LIBRARY_PATH", "/tmp/.mount_gan/usr/lib:"),
        ("XDG_DATA_DIRS", "/tmp/.mount_gan/usr/share:/usr/share:"),
        ("GTK_PATH", "/tmp/.mount_gan/usr/lib/gtk-3.0"),
        (
            "GDK_PIXBUF_MODULE_FILE",
            "/tmp/.mount_gan/usr/lib/loaders.cache",
        ),
        ("PROJECTS", "/tmp/.mount_ganbaru/work"),
        ("SSH_AUTH_SOCK", "/run/user/1000/agent"),
        ("TMUX", "/tmp/tmux-1000/default,1234,0"),
        ("EDITOR", "nvim"),
    ]);
    assert_eq!(
        terminal_environment(packaged),
        with_fish_query_term_disabled(environment(&[
            ("PATH", "/usr/local/bin:/usr/bin"),
            ("XDG_DATA_DIRS", "/usr/share:"),
            ("PROJECTS", "/tmp/.mount_ganbaru/work"),
            ("SSH_AUTH_SOCK", "/run/user/1000/agent"),
            ("EDITOR", "nvim"),
        ]))
    );

    let unsafe_appdir = environment(&[
        ("APPIMAGE", "/home/user/Ganbaru.AppImage"),
        ("APPDIR", "/"),
        ("PATH", "/usr/bin"),
    ]);
    assert_eq!(
        terminal_environment(unsafe_appdir.clone()),
        with_fish_query_term_disabled(unsafe_appdir)
    );
}

#[test]
fn terminal_name_uses_the_local_identity_and_home_relative_workspace_path() {
    let home = PathBuf::from("home").join("alice");
    let workspace = home.join("Documents").join("ganbaru");
    let separator = std::path::MAIN_SEPARATOR;
    assert_eq!(
        format_terminal_name("alice", "workstation", &workspace, Some(&home)),
        format!("alice@workstation: ~{separator}Documents{separator}ganbaru")
    );
}

#[test]
fn terminal_identity_rejects_empty_and_control_character_values() {
    assert_eq!(normalized_identity("vic\ntor"), None);
    assert_eq!(normalized_identity("   "), None);
    assert_eq!(normalized_identity(" alice \n"), Some("alice".to_string()));
}

#[cfg(unix)]
#[test]
fn hostname_decoding_accepts_terminated_and_full_buffers() {
    assert_eq!(
        hostname_from_buffer(b"workstation\0ignored"),
        Some("workstation".to_string())
    );
    assert_eq!(
        hostname_from_buffer(b"full-buffer-name"),
        Some("full-buffer-name".to_string())
    );
    assert_eq!(hostname_from_buffer(b"\0"), None);
    assert_eq!(
        hostname_from_buffer(&[b'w', b'o', b'r', b'k', 0xff, 0]),
        Some("work�".to_string())
    );
    assert_eq!(hostname_from_buffer(b"invalid\nname\0"), None);
    assert_eq!(hostname_from_call(-1, b"ignored\0"), None);
}
