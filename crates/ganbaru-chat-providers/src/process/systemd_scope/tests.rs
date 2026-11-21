use super::*;

fn strings(arguments: &[OsString]) -> Vec<&str> {
    arguments
        .iter()
        .map(|argument| argument.to_str().expect("arguments are UTF-8"))
        .collect()
}

fn missing_marker(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ganbaru-{label}-{}", std::process::id()))
}

#[test]
fn systemd_scope_ceilings_keep_half_of_a_small_machine() {
    let ceilings = memory_ceilings(8 * GIB);
    assert_eq!(ceilings.high_bytes, 4 * GIB);
    assert_eq!(ceilings.max_bytes, 6 * GIB);
}

#[test]
fn systemd_scope_ceilings_leave_fixed_headroom_on_a_large_machine() {
    let ceilings = memory_ceilings(64 * GIB);
    assert_eq!(ceilings.high_bytes, 58 * GIB);
    assert_eq!(ceilings.max_bytes, 60 * GIB);
}

#[test]
fn systemd_scope_arguments_strip_wrapper_variables_before_the_provider() {
    let arguments = scope_arguments(
        Some(memory_ceilings(8 * GIB)),
        Path::new("/usr/bin/env"),
        &["XDG_RUNTIME_DIR", "INVOCATION_ID"],
        Path::new("/opt/codex/bin/codex"),
        &["app-server".to_string(), "--listen".to_string()],
    );
    assert_eq!(
        strings(&arguments),
        vec![
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "-p",
            "CPUWeight=10",
            "-p",
            "IOWeight=10",
            "-p",
            "TasksMax=25%",
            "-p",
            "MemoryHigh=4294967296",
            "-p",
            "MemoryMax=6442450944",
            "--",
            "/usr/bin/env",
            "-u",
            "XDG_RUNTIME_DIR",
            "-u",
            "INVOCATION_ID",
            "--",
            "/opt/codex/bin/codex",
            "app-server",
            "--listen",
        ]
    );
    let without_memory = scope_arguments(
        None,
        Path::new("/usr/bin/env"),
        &[],
        Path::new("/bin/true"),
        &[],
    );
    assert!(
        strings(&without_memory)
            .iter()
            .all(|argument| !argument.starts_with("Memory"))
    );
    assert_eq!(strings(&without_memory).last(), Some(&"/bin/true"));
}

#[test]
fn systemd_scope_keeps_wrapper_variables_the_provider_configures() {
    let configured = BTreeMap::from([(
        "DBUS_SESSION_BUS_ADDRESS".to_string(),
        "unix:path=/run/bus".to_string(),
    )]);
    assert_eq!(
        wrapper_only_variables(&configured),
        vec!["XDG_RUNTIME_DIR", "INVOCATION_ID"]
    );
    assert_eq!(
        wrapper_only_variables(&BTreeMap::new()),
        vec![
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "INVOCATION_ID"
        ]
    );
    let environment = wrapper_environment(&configured);
    assert_eq!(
        environment
            .get("DBUS_SESSION_BUS_ADDRESS")
            .map(String::as_str),
        Some("unix:path=/run/bus"),
        "a configured value is never replaced by the app environment"
    );
}

#[test]
fn systemd_scope_refuses_executables_env_would_read_as_assignments() {
    assert!(launchable_through_env(Path::new("/opt/codex/bin/codex")));
    assert!(!launchable_through_env(Path::new("/opt/a=b/codex")));
}

#[test]
fn systemd_scope_reads_mem_total_in_bytes() {
    let meminfo = "MemFree:         1000 kB\nMemTotal:       16318640 kB\n";
    assert_eq!(parse_mem_total(meminfo), Some(16_318_640 * 1024));
    assert_eq!(parse_mem_total("MemTotal: 12 MB\n"), None);
    assert_eq!(parse_mem_total("MemFree: 1 kB\n"), None);
}

#[cfg(unix)]
#[test]
fn systemd_scope_probe_falls_back_without_a_working_chain() {
    let marker = missing_marker("no-flatpak");
    let env = Path::new("/usr/bin/env");
    let missing = Path::new("/nonexistent/systemd-run");
    assert_eq!(
        crate::test_block_on(probe_scope_launcher(&[missing], &[env], &marker)),
        None
    );
    let failing = Path::new("/bin/false");
    if failing.is_file() {
        assert_eq!(
            crate::test_block_on(probe_scope_launcher(&[failing], &[env], &marker)),
            None,
            "a runner that fails the probe is not used"
        );
    }
    let runner = Path::new("/bin/true");
    if runner.is_file() {
        assert_eq!(
            crate::test_block_on(probe_scope_launcher(
                &[runner],
                &[Path::new("/nonexistent/env")],
                &marker
            )),
            None,
            "a missing env wrapper disables the scope"
        );
    }
}

#[test]
fn systemd_scope_probe_is_skipped_inside_flatpak() {
    let marker = missing_marker("flatpak-info");
    std::fs::write(&marker, "[Application]\n").expect("marker is written");
    #[cfg(unix)]
    let runner = Path::new("/bin/true");
    #[cfg(not(unix))]
    let runner = Path::new("C:\\Windows\\System32\\whoami.exe");
    let probed = crate::test_block_on(probe_scope_launcher(&[runner], &[runner], &marker));
    std::fs::remove_file(&marker).expect("marker is removed");
    assert_eq!(probed, None);
}

#[cfg(unix)]
#[test]
fn systemd_scope_probe_accepts_a_chain_that_succeeds() {
    let marker = missing_marker("no-flatpak-ok");
    let runner = Path::new("/bin/true");
    let env = Path::new("/usr/bin/env");
    if runner.is_file() && env.is_file() {
        assert_eq!(
            crate::test_block_on(probe_scope_launcher(&[runner], &[env], &marker)),
            Some(ScopeLauncher {
                runner: runner.to_path_buf(),
                env: env.to_path_buf(),
            })
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn systemd_scope_reads_cgroup_paths_and_oom_kill_counters() {
    use super::linux::{parse_cgroup_path, parse_oom_kills};

    assert_eq!(
        parse_cgroup_path("0::/user.slice/app.slice/run-r1.scope\n"),
        Some("/user.slice/app.slice/run-r1.scope")
    );
    assert_eq!(parse_cgroup_path("1:name=systemd:/x\n"), None);
    let events = "low 0\nhigh 4\nmax 2\noom 1\noom_kill 3\noom_group_kill 0\n";
    assert_eq!(parse_oom_kills(events), Some(3));
    assert_eq!(parse_oom_kills("oom 1\n"), None);
}

#[cfg(target_os = "linux")]
#[test]
fn systemd_scope_runs_the_provider_in_place_without_the_wrapper_variables() {
    let runners = SYSTEMD_RUN_CANDIDATES.map(Path::new);
    let envs = ENV_CANDIDATES.map(Path::new);
    let marker = Path::new(FLATPAK_MARKER);
    crate::test_block_on(async {
        let Some(launcher) = probe_scope_launcher(&runners, &envs, marker).await else {
            return;
        };
        let output =
            std::env::temp_dir().join(format!("ganbaru-scope-environment-{}", std::process::id()));
        let configured = BTreeMap::from([("GANBARU_SCOPE_TEST".to_string(), "1".to_string())]);
        let script = format!("/usr/bin/env > {}; exec /bin/sleep 30", output.display());
        let mut command = tokio::process::Command::new(&launcher.runner);
        command
            .args(scope_arguments(
                None,
                &launcher.env,
                &wrapper_only_variables(&configured),
                Path::new("/bin/sh"),
                &["-c".to_string(), script],
            ))
            .env_clear()
            .envs(wrapper_environment(&configured))
            .kill_on_drop(true);
        let mut child = command.spawn().expect("scoped launch starts");
        let pid = child.id().expect("scoped launch has a process ID");
        assert!(
            confirm_scope_entry(&mut child, &launcher.runner),
            "the scope replaces the runner with the provider"
        );
        let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
        let mut environment = String::new();
        for _ in 0..100 {
            environment = std::fs::read_to_string(&output).unwrap_or_default();
            if environment.contains("GANBARU_SCOPE_TEST") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _ = child.kill().await;
        let _ = std::fs::remove_file(&output);
        assert!(cgroup.contains(".scope"), "{cgroup}");
        assert!(
            environment.contains("GANBARU_SCOPE_TEST=1"),
            "{environment}"
        );
        for key in [
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "INVOCATION_ID",
        ] {
            assert!(
                !environment.contains(&format!("{key}=")),
                "{key} reached the provider"
            );
        }
    });
}
