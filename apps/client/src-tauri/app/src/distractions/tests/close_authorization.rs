use super::*;

#[cfg(target_os = "linux")]
#[test]
fn close_request_rejects_pid_reuse_and_tampered_identity() {
    let request = super::DistractionsCloseDesktopAppRequest {
        process_id: 42,
        process_name: "steam".to_string(),
        process_identity: "observed-start-1".to_string(),
        rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
            rule_id: "Steam".to_string(),
        },
    };
    let reused = super::ObservedDesktopProcess {
        process_name: "steam".to_string(),
        match_names: vec!["steam".to_string()],
        process_identity: "observed-start-2".to_string(),
        executable_name: Some("steam".to_string()),
    };

    assert!(super::validate_observed_close_process(&request, &reused).is_err());
    let mut tampered = request;
    tampered.process_name = "discord".to_string();
    assert!(super::validate_observed_close_process(&tampered, &valid_observed_process()).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn close_request_rejects_protected_process_aliases() {
    let request = super::DistractionsCloseDesktopAppRequest {
        process_id: 42,
        process_name: "steam".to_string(),
        process_identity: "observed-start-1".to_string(),
        rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
            rule_id: "Steam".to_string(),
        },
    };
    let observed = super::ObservedDesktopProcess {
        process_name: "steam".to_string(),
        match_names: vec!["steam".to_string(), "sh".to_string()],
        process_identity: "observed-start-1".to_string(),
        executable_name: Some("steam".to_string()),
    };

    assert!(super::validate_observed_close_process(&request, &observed).is_err());
}

#[cfg(target_os = "linux")]
fn valid_close_request() -> super::DistractionsCloseDesktopAppRequest {
    super::DistractionsCloseDesktopAppRequest {
        process_id: 42,
        process_name: "steam".to_string(),
        process_identity: "observed-start-1".to_string(),
        rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
            rule_id: "Steam".to_string(),
        },
    }
}

#[cfg(target_os = "linux")]
fn valid_observed_process() -> super::ObservedDesktopProcess {
    super::ObservedDesktopProcess {
        process_name: "steam".to_string(),
        match_names: vec!["steam".to_string()],
        process_identity: "observed-start-1".to_string(),
        executable_name: Some("steam".to_string()),
    }
}

#[cfg(target_os = "linux")]
struct MockDesktopProcessController {
    observations: VecDeque<Option<super::ObservedDesktopProcess>>,
    fallback: Option<super::ObservedDesktopProcess>,
    signals: Vec<super::DesktopProcessSignal>,
    waits: usize,
    pins: usize,
    calls: Vec<&'static str>,
}

#[cfg(target_os = "linux")]
impl super::DesktopProcessController for MockDesktopProcessController {
    /// The order in which the process was pinned.
    type Pinned = usize;

    fn pin(&mut self, _process_id: u32) -> Result<Option<usize>, String> {
        self.calls.push("pin");
        self.pins += 1;
        Ok(Some(self.pins))
    }

    fn observe(
        &mut self,
        _process_id: u32,
    ) -> Result<Option<super::ObservedDesktopProcess>, String> {
        self.calls.push("observe");
        Ok(self
            .observations
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone()))
    }

    fn signal(
        &mut self,
        pinned: &usize,
        signal: super::DesktopProcessSignal,
    ) -> Result<(), String> {
        assert_eq!(
            *pinned, 1,
            "signals go to the process pinned before the first check"
        );
        self.calls.push("signal");
        self.signals.push(signal);
        Ok(())
    }

    fn wait(&mut self, _duration: std::time::Duration) {
        self.waits += 1;
    }
}

#[cfg(target_os = "linux")]
#[test]
fn close_process_rechecks_identity_before_kill_fallback() {
    let observed = valid_observed_process();
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::from([Some(observed.clone())]),
        fallback: Some(observed),
        signals: Vec::new(),
        waits: 0,
        pins: 0,
        calls: Vec::new(),
    };
    let mut authorization_checks = 0;

    super::close_desktop_process_with(&valid_close_request(), &mut controller, |_| {
        authorization_checks += 1;
        Ok(())
    })
    .unwrap();

    assert_eq!(
        controller.signals,
        vec![
            super::DesktopProcessSignal::Term,
            super::DesktopProcessSignal::Kill
        ]
    );
    assert_eq!(controller.waits, 8);
    assert_eq!(authorization_checks, 2);
    assert_eq!(controller.pins, 1);
    assert_eq!(&controller.calls[..3], ["pin", "observe", "signal"]);
}

/// Spawns a child that waits to be signalled.
#[cfg(target_os = "linux")]
fn sleeping_child() -> std::process::Child {
    std::process::Command::new("sleep")
        .arg("30")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn sleep")
}

#[cfg(target_os = "linux")]
#[test]
fn a_pinned_process_receives_the_signal() {
    use std::os::unix::process::ExitStatusExt;

    let mut child = sleeping_child();
    let pinned = super::process_control::pin_linux_process(child.id())
        .unwrap()
        .expect("live child is pinned");

    super::process_control::signal_linux_process(&pinned, super::DesktopProcessSignal::Term)
        .unwrap();

    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
}

#[cfg(target_os = "linux")]
#[test]
fn a_pinned_process_that_exited_is_never_signalled_by_its_old_pid() {
    let mut child = sleeping_child();
    let process_id = child.id();
    let pinned = super::process_control::pin_linux_process(process_id)
        .unwrap()
        .expect("live child is pinned");
    child.kill().unwrap();
    child.wait().unwrap();

    let result =
        super::process_control::signal_linux_process(&pinned, super::DesktopProcessSignal::Kill);

    match pinned {
        super::process_control::PinnedLinuxProcess::Pidfd(_) => assert!(
            result.is_err(),
            "a pidfd keeps naming the exited process, so the signal cannot reach a PID reuse"
        ),
        super::process_control::PinnedLinuxProcess::Pid(_) => {}
    }
}

#[cfg(target_os = "linux")]
#[test]
fn already_exited_process_does_not_report_an_accepted_close() {
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::new(),
        fallback: None,
        signals: Vec::new(),
        waits: 0,
        pins: 0,
        calls: Vec::new(),
    };
    let signalled =
        super::close_desktop_process_with(&valid_close_request(), &mut controller, |_| Ok(()))
            .unwrap();
    assert!(!signalled);
    assert!(controller.signals.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn close_process_stops_when_process_exits_between_checks() {
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::from([Some(valid_observed_process()), None]),
        fallback: None,
        signals: Vec::new(),
        waits: 0,
        pins: 0,
        calls: Vec::new(),
    };

    super::close_desktop_process_with(&valid_close_request(), &mut controller, |_| Ok(())).unwrap();

    assert_eq!(controller.signals, vec![super::DesktopProcessSignal::Term]);
    assert_eq!(controller.waits, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn close_process_rejects_pid_reuse_after_term() {
    let mut reused = valid_observed_process();
    reused.process_identity = "observed-start-2".to_string();
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::from([Some(valid_observed_process()), Some(reused)]),
        fallback: None,
        signals: Vec::new(),
        waits: 0,
        pins: 0,
        calls: Vec::new(),
    };

    assert!(
        super::close_desktop_process_with(&valid_close_request(), &mut controller, |_| Ok(()))
            .is_err()
    );
    assert_eq!(controller.signals, vec![super::DesktopProcessSignal::Term]);
}

#[cfg(target_os = "linux")]
#[test]
fn close_process_rejects_stale_rule_before_term() {
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::from([Some(valid_observed_process())]),
        fallback: None,
        signals: Vec::new(),
        waits: 0,
        pins: 0,
        calls: Vec::new(),
    };

    assert!(
        super::close_desktop_process_with(&valid_close_request(), &mut controller, |_| Err(
            "desktop blocker rule is no longer configured".to_string()
        ),)
        .is_err()
    );
    assert!(controller.signals.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn close_process_rejects_rule_revoked_before_kill_fallback() {
    let observed = valid_observed_process();
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::from([Some(observed.clone())]),
        fallback: Some(observed),
        signals: Vec::new(),
        waits: 0,
        pins: 0,
        calls: Vec::new(),
    };
    let mut authorization_checks = 0;

    assert!(
        super::close_desktop_process_with(&valid_close_request(), &mut controller, |_| {
            authorization_checks += 1;
            if authorization_checks == 1 {
                Ok(())
            } else {
                Err("desktop blocker rule is no longer configured".to_string())
            }
        })
        .is_err()
    );

    assert_eq!(controller.signals, vec![super::DesktopProcessSignal::Term]);
    assert_eq!(authorization_checks, 2);
}

#[test]
fn persisted_desktop_rule_authorization_tracks_current_phase_and_rule_state() {
    let config = serde_json::json!({
        "distractions": {
            "desktop": {
                "enabled": true,
                "blockDuringFocus": true,
                "pauseDuringFocusPause": true,
                "blockedApps": [{
                    "name": "Steam",
                    "enabled": true,
                    "matchNames": ["steam"]
                }]
            }
        }
    });
    let runtime = state("focus");
    let identity = super::DistractionsDesktopRuleIdentity::DesktopApp {
        rule_id: "Steam".to_string(),
    };
    let authorization = super::configured_close_authorization(
        &config,
        Some(&runtime),
        None,
        &identity,
        &super::DesktopCatalogIndex::default(),
    )
    .unwrap();
    assert!(super::validate_names_authorized(vec!["steam".to_string()], &authorization).is_ok());

    let mut paused = state("focus");
    paused.paused = true;
    paused.pause_reason = Some("manual".to_string());
    assert!(
        super::configured_close_authorization(
            &config,
            Some(&paused),
            None,
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
    let disabled = serde_json::json!({
        "distractions": {
            "desktop": {
                "enabled": true,
                "blockedApps": [{
                    "name": "Steam",
                    "enabled": false,
                    "matchNames": ["steam"]
                }]
            }
        }
    });
    assert!(
        super::configured_close_authorization(
            &disabled,
            Some(&runtime),
            None,
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
}

#[test]
fn persisted_usage_rule_requires_exact_entry_and_exhausted_limit() {
    let config = serde_json::json!({
        "distractions": {
            "limits": {
                "enabled": true,
                "items": [{
                    "id": "games",
                    "enabled": true,
                    "minutesPerDay": 10,
                    "entries": [{
                        "id": "steam-entry",
                        "desktopAppName": "Steam",
                        "desktopAppMatchNames": ["steam"]
                    }]
                }]
            }
        }
    });
    let identity = super::DistractionsDesktopRuleIdentity::UsageLimit {
        rule_id: "games".to_string(),
        entry_id: "steam-entry".to_string(),
    };
    let mut limit_state = DistractionsLimitState {
        local_date: "2026-07-10".to_string(),
        week_start_local_date: "2026-07-06".to_string(),
        updated_at: "2026-07-10T12:00:00.000Z".to_string(),
        database_path: Some("/tmp/vault/ganbaru.sqlite".to_string()),
        configuration_digest: Some(limits::configuration_digest(&config).unwrap()),
        limits: vec![DistractionsLimitStateItem {
            id: "games".to_string(),
            period: "day".to_string(),
            window_start_local_date: "2026-07-10".to_string(),
            window_end_local_date: "2026-07-10".to_string(),
            used_seconds: 600,
            limit_seconds: 600,
            remaining_seconds: 0,
            exhausted: true,
        }],
    };

    assert!(
        super::configured_close_authorization(
            &config,
            None,
            Some(&limit_state),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_ok()
    );
    let mut changed_config = config.clone();
    changed_config["distractions"]["limits"]["items"][0]["minutesPerDay"] = serde_json::json!(20);
    assert!(
        super::configured_close_authorization(
            &changed_config,
            None,
            Some(&limit_state),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
    changed_config = config.clone();
    changed_config["distractions"]["limits"]["items"][0]["entries"][0]["desktopAppMatchNames"] =
        serde_json::json!(["othergame"]);
    assert!(
        super::configured_close_authorization(
            &changed_config,
            None,
            Some(&limit_state),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
    let mut invalid = limit_state.clone();
    invalid.configuration_digest = None;
    assert!(
        super::configured_close_authorization(
            &config,
            None,
            Some(&invalid),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
    invalid = limit_state.clone();
    invalid.limits[0].used_seconds = 1;
    assert!(
        super::configured_close_authorization(
            &config,
            None,
            Some(&invalid),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
    invalid = limit_state.clone();
    invalid.limits[0].window_end_local_date = "2026-07-11".into();
    assert!(
        super::configured_close_authorization(
            &config,
            None,
            Some(&invalid),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
    limit_state.limits[0].exhausted = false;
    assert!(
        super::configured_close_authorization(
            &config,
            None,
            Some(&limit_state),
            &identity,
            &super::DesktopCatalogIndex::default()
        )
        .is_err()
    );
}

fn focus_rule_config(name: &str, match_names: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "distractions": {
            "desktop": {
                "enabled": true,
                "blockDuringFocus": true,
                "blockedApps": [{ "name": name, "enabled": true, "matchNames": match_names }]
            }
        }
    })
}

#[test]
fn desktop_rule_closes_only_names_its_own_name_or_the_app_catalog_vouches_for() {
    let config = focus_rule_config("Firefox", serde_json::json!(["firefox-bin", "obsidian"]));
    let identity = super::DistractionsDesktopRuleIdentity::DesktopApp {
        rule_id: "Firefox".to_string(),
    };
    let catalog =
        super::DesktopCatalogIndex::from_candidates(vec![super::DistractionsDesktopAppCandidate {
            name: "Firefox".to_string(),
            source: "Installed app".to_string(),
            detail: None,
            process_names: vec!["firefox".to_string(), "firefox-bin".to_string()],
        }]);
    let authorization = super::configured_close_authorization(
        &config,
        Some(&state("focus")),
        None,
        &identity,
        &catalog,
    )
    .unwrap();
    assert!(super::validate_names_authorized(vec!["firefox".to_string()], &authorization).is_ok());
    assert!(
        super::validate_names_authorized(vec!["firefox-bin".to_string()], &authorization).is_ok()
    );
    assert!(
        super::validate_names_authorized(vec!["obsidian".to_string()], &authorization).is_err()
    );

    let without_catalog = super::configured_close_authorization(
        &config,
        Some(&state("focus")),
        None,
        &identity,
        &super::DesktopCatalogIndex::default(),
    )
    .unwrap();
    assert_eq!(without_catalog.match_names(), ["Firefox"]);
}

#[test]
fn desktop_rule_naming_only_a_session_service_is_not_closeable() {
    for name in ["pipewire", "ssh-agent", "tmux"] {
        let config = focus_rule_config(name, serde_json::json!([name]));
        let identity = super::DistractionsDesktopRuleIdentity::DesktopApp {
            rule_id: name.to_string(),
        };
        assert!(
            super::configured_close_authorization(
                &config,
                Some(&state("focus")),
                None,
                &identity,
                &super::DesktopCatalogIndex::default(),
            )
            .is_err(),
            "{name}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn close_request_rejects_protected_executables_and_truncated_comm_names() {
    let mut renamed = valid_observed_process();
    renamed.executable_name = Some("tmux".to_string());
    assert!(super::validate_observed_close_process(&valid_close_request(), &renamed).is_err());

    let mut truncated = valid_observed_process();
    truncated.match_names.push("gnome-terminal-".to_string());
    assert!(super::validate_observed_close_process(&valid_close_request(), &truncated).is_err());

    assert!(
        super::validate_observed_close_process(&valid_close_request(), &valid_observed_process())
            .is_ok()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn executable_names_ignore_the_deleted_marker() {
    assert_eq!(
        super::processes::linux_executable_name("/usr/bin/tmux (deleted)").as_deref(),
        Some("tmux")
    );
    assert_eq!(super::processes::linux_executable_name(""), None);
}

#[cfg(target_os = "linux")]
#[test]
fn status_real_uid_reads_the_first_uid_field() {
    let status = "Name:\tsteam\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
    assert_eq!(super::processes::linux_status_real_uid(status), Some(1000));
    assert_eq!(
        super::processes::linux_status_real_uid("Name:\tsteam\n"),
        None
    );
    assert_eq!(
        super::processes::linux_status_real_uid("Uid:\tnot-a-number\n"),
        None
    );
}

#[cfg(target_os = "linux")]
#[test]
fn observation_skips_processes_of_other_users() {
    let own_id = std::process::id();
    let own = super::observe_linux_process(std::path::Path::new("/proc/self"), own_id);
    assert!(own.is_some());
    let init_status = std::fs::read_to_string("/proc/1/status").unwrap_or_default();
    let own_status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let init_uid = super::processes::linux_status_real_uid(&init_status);
    let own_uid = super::processes::linux_status_real_uid(&own_status);
    if init_uid.is_some() && init_uid != own_uid {
        assert!(super::observe_linux_process(std::path::Path::new("/proc/1"), 1).is_none());
    }
}
