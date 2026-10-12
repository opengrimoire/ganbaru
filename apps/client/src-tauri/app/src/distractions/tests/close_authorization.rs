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
    };

    assert!(super::validate_observed_close_process(&request, &reused).is_err());
    let mut tampered = request;
    tampered.process_name = "discord".to_string();
    assert!(super::validate_observed_close_process(&tampered, &valid_observed_process()).is_err());
}

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
    };

    assert!(super::validate_observed_close_process(&request, &observed).is_err());
}
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

fn valid_observed_process() -> super::ObservedDesktopProcess {
    super::ObservedDesktopProcess {
        process_name: "steam".to_string(),
        match_names: vec!["steam".to_string()],
        process_identity: "observed-start-1".to_string(),
    }
}

#[cfg(target_os = "linux")]
struct MockDesktopProcessController {
    observations: VecDeque<Option<super::ObservedDesktopProcess>>,
    fallback: Option<super::ObservedDesktopProcess>,
    signals: Vec<super::DesktopProcessSignal>,
    waits: usize,
}

#[cfg(target_os = "linux")]
impl super::DesktopProcessController for MockDesktopProcessController {
    fn observe(
        &mut self,
        _process_id: u32,
    ) -> Result<Option<super::ObservedDesktopProcess>, String> {
        Ok(self
            .observations
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone()))
    }

    fn signal(
        &mut self,
        _process_id: u32,
        signal: super::DesktopProcessSignal,
    ) -> Result<(), String> {
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
}

#[cfg(target_os = "linux")]
#[test]
fn already_exited_process_does_not_report_an_accepted_close() {
    let mut controller = MockDesktopProcessController {
        observations: VecDeque::new(),
        fallback: None,
        signals: Vec::new(),
        waits: 0,
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
    let authorization =
        super::configured_close_authorization(&config, Some(&runtime), None, &identity).unwrap();
    assert!(super::validate_names_authorized(vec!["steam".to_string()], &authorization).is_ok());

    let mut paused = state("focus");
    paused.paused = true;
    paused.pause_reason = Some("manual".to_string());
    assert!(
        super::configured_close_authorization(&config, Some(&paused), None, &identity).is_err()
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
        super::configured_close_authorization(&disabled, Some(&runtime), None, &identity).is_err()
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
        database_path: Some("/tmp/vault/ganbaru-ai.sqlite".to_string()),
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
        super::configured_close_authorization(&config, None, Some(&limit_state), &identity).is_ok()
    );
    let mut changed_config = config.clone();
    changed_config["distractions"]["limits"]["items"][0]["minutesPerDay"] = serde_json::json!(20);
    assert!(
        super::configured_close_authorization(&changed_config, None, Some(&limit_state), &identity)
            .is_err()
    );
    changed_config = config.clone();
    changed_config["distractions"]["limits"]["items"][0]["entries"][0]["desktopAppMatchNames"] =
        serde_json::json!(["othergame"]);
    assert!(
        super::configured_close_authorization(&changed_config, None, Some(&limit_state), &identity)
            .is_err()
    );
    let mut invalid = limit_state.clone();
    invalid.configuration_digest = None;
    assert!(
        super::configured_close_authorization(&config, None, Some(&invalid), &identity).is_err()
    );
    invalid = limit_state.clone();
    invalid.limits[0].used_seconds = 1;
    assert!(
        super::configured_close_authorization(&config, None, Some(&invalid), &identity).is_err()
    );
    invalid = limit_state.clone();
    invalid.limits[0].window_end_local_date = "2026-07-11".into();
    assert!(
        super::configured_close_authorization(&config, None, Some(&invalid), &identity).is_err()
    );
    limit_state.limits[0].exhausted = false;
    assert!(
        super::configured_close_authorization(&config, None, Some(&limit_state), &identity)
            .is_err()
    );
}
