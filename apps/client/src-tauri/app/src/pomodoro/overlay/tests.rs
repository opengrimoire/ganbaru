use super::shortcuts::{
    SHORTCUT_RESTORE_VALUE_MAX_BYTES, SavedShortcuts, is_allowed_gsettings_restore_target,
    load_pending_restore, merge_pending_originals, unusable_restore_path, validate_saved_shortcuts,
};
use super::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[test]
fn native_completion_dismissal_requires_the_retained_generation_run_and_segment() {
    let mut scope = PomodoroCompletionScope {
        vault_generation: 3,
        run_id: "run".into(),
        segment_id: "segment".into(),
    };
    assert!(scope.matches_retained(3, Some("run"), Some("segment")));
    assert!(!scope.matches_retained(4, Some("run"), Some("segment")));
    assert!(!scope.matches_retained(3, Some("other-run"), Some("segment")));
    assert!(!scope.matches_retained(3, Some("run"), Some("other-segment")));
    assert!(!scope.matches_retained(3, None, Some("segment")));
    assert!(!scope.matches_retained(3, Some("run"), None));
    scope.vault_generation = 0;
    assert!(!scope.matches_retained(0, Some("run"), Some("segment")));
    scope.vault_generation = 3;
    scope.segment_id.clear();
    assert!(!scope.matches_retained(3, Some("run"), Some("")));
    scope.segment_id = "x".repeat(129);
    assert!(!scope.matches_retained(3, Some("run"), Some(&scope.segment_id)));
}

#[test]
fn overlay_lifecycle_shut_down_is_terminal() {
    let mut lifecycle = OverlayLifecycle::Closed;
    assert!(lifecycle.close(false).is_none());
    assert!(matches!(lifecycle, OverlayLifecycle::Closed));
    assert!(lifecycle.session().is_none());

    assert!(lifecycle.close(true).is_none());
    assert!(matches!(lifecycle, OverlayLifecycle::ShutDown));
    assert!(lifecycle.close(false).is_none());
    assert!(matches!(lifecycle, OverlayLifecycle::ShutDown));
    assert!(lifecycle.session_mut().is_none());
}

#[test]
fn overlay_state_starts_closed_and_reports_shutdown() {
    let overlays = PomodoroOverlayState::default();
    assert!(!overlays.is_active());
    assert!(!overlays.is_shut_down());
    overlays.lock_state().close(true);
    assert!(!overlays.is_active());
    assert!(overlays.is_shut_down());
}

fn known_keys() -> HashMap<String, HashSet<String>> {
    HashMap::from([(
        "org.gnome.desktop.wm.keybindings".to_string(),
        HashSet::from(["close".to_string(), "switch-applications".to_string()]),
    )])
}

fn valid_saved_shortcuts() -> SavedShortcuts {
    SavedShortcuts {
        overlay_key: "'Super_L'".to_string(),
        dock_hotkeys: "true".to_string(),
        disabled: vec![(
            "org.gnome.desktop.wm.keybindings".to_string(),
            "close".to_string(),
            "['<Alt>F4']".to_string(),
        )],
    }
}

#[test]
fn pomodoro_overlay_visual_states_have_expected_backgrounds() {
    assert_eq!(
        OverlayVisualState::from_id("idle")
            .unwrap()
            .background_color(),
        OverlayColor {
            red: 0xA3,
            green: 0x37,
            blue: 0x28,
        }
    );
    assert_eq!(
        OverlayVisualState::from_id("break_countdown")
            .unwrap()
            .background_color(),
        OverlayColor {
            red: 0x03,
            green: 0x5B,
            blue: 0x33,
        }
    );
    assert_eq!(
        OverlayVisualState::from_id("break_finished")
            .unwrap()
            .background_color(),
        OverlayColor {
            red: 0xEE,
            green: 0xBA,
            blue: 0x04,
        }
    );
    assert_eq!(
        OverlayVisualState::from_id("event_finished")
            .unwrap()
            .background_color(),
        OverlayColor {
            red: 0x0E,
            green: 0x74,
            blue: 0x90,
        }
    );
    assert_eq!(
        OverlayVisualState::from_id("day_complete")
            .unwrap()
            .background_color(),
        OverlayColor {
            red: 0xEE,
            green: 0xBA,
            blue: 0x04,
        }
    );
    assert_eq!(
        OverlayVisualState::from_id("workweek_complete")
            .unwrap()
            .background_color(),
        OverlayColor {
            red: 0x1D,
            green: 0x4E,
            blue: 0xD8,
        }
    );
}

#[test]
fn pomodoro_completion_kinds_map_to_visual_states() {
    assert_eq!(
        completion_visual_state_from_kind("event").unwrap(),
        OverlayVisualState::EventFinished
    );
    assert_eq!(
        completion_visual_state_from_kind("day").unwrap(),
        OverlayVisualState::DayComplete
    );
    assert_eq!(
        completion_visual_state_from_kind("workweek").unwrap(),
        OverlayVisualState::WorkweekComplete
    );
    assert!(completion_visual_state_from_kind("unknown").is_err());
}

#[test]
fn break_end_esc_presses_accepts_supported_values_and_disabled() {
    assert_eq!(normalize_break_end_esc_presses(None), None);
    assert_eq!(normalize_break_end_esc_presses(Some(1)), Some(1));
    assert_eq!(normalize_break_end_esc_presses(Some(3)), Some(3));
    assert_eq!(normalize_break_end_esc_presses(Some(10)), Some(10));
    assert_eq!(normalize_break_end_esc_presses(Some(20)), Some(20));
    assert_eq!(normalize_break_end_esc_presses(Some(50)), Some(50));
    assert_eq!(normalize_break_end_esc_presses(Some(2)), Some(10));
}

#[test]
fn break_extension_limit_accepts_supported_values_and_disabled() {
    assert_eq!(normalize_break_extension_limit(None), None);
    assert_eq!(normalize_break_extension_limit(Some(1)), Some(1));
    assert_eq!(normalize_break_extension_limit(Some(3)), Some(3));
    assert_eq!(normalize_break_extension_limit(Some(5)), Some(5));
    assert_eq!(normalize_break_extension_limit(Some(10)), Some(10));
    assert_eq!(normalize_break_extension_limit(Some(15)), Some(15));
    assert_eq!(normalize_break_extension_limit(Some(2)), Some(3));
}

fn unique_restore_file(name: &str) -> PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("ganbaru-{name}-{}-{now}.json", std::process::id()))
}

#[test]
fn restore_validation_accepts_known_schema_and_key() {
    assert!(validate_saved_shortcuts(&valid_saved_shortcuts(), &known_keys()).is_ok());
}

#[test]
fn restore_validation_rejects_unknown_schema() {
    let mut saved = valid_saved_shortcuts();
    saved.disabled[0].0 = "org.gnome.unknown.keybindings".to_string();

    assert!(validate_saved_shortcuts(&saved, &known_keys()).is_err());
}

#[test]
fn restore_validation_rejects_unknown_key() {
    let mut saved = valid_saved_shortcuts();
    saved.disabled[0].1 = "launch-terminal".to_string();

    assert!(validate_saved_shortcuts(&saved, &known_keys()).is_err());
}

#[test]
fn restore_validation_rejects_oversized_values() {
    let mut saved = valid_saved_shortcuts();
    saved.disabled[0].2 = "x".repeat(SHORTCUT_RESTORE_VALUE_MAX_BYTES + 1);

    assert!(validate_saved_shortcuts(&saved, &known_keys()).is_err());
}

#[test]
fn restore_validation_rejects_unsafe_command_targets() {
    assert!(is_allowed_gsettings_restore_target(
        "org.gnome.desktop.wm.keybindings",
        "close"
    ));
    assert!(!is_allowed_gsettings_restore_target(
        "org.gnome.desktop.wm.keybindings",
        "Close"
    ));
    assert!(!is_allowed_gsettings_restore_target(
        "org.gnome.unknown.keybindings",
        "close"
    ));
}

fn restore_fixture_dir(name: &str) -> PathBuf {
    let dir = unique_restore_file(name).with_extension("");
    std::fs::create_dir_all(&dir).expect("restore fixture directory should be creatable");
    dir.join("gnome-shortcuts-restore.json")
}

#[test]
fn malformed_restore_file_is_kept_aside_instead_of_deleted() {
    let path = restore_fixture_dir("malformed-shortcuts");
    std::fs::write(&path, "{not valid json").expect("restore fixture should be writable");

    assert!(load_pending_restore(&path, &known_keys()).is_err());
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(unusable_restore_path(&path))
            .expect("unusable restore file should be kept"),
        "{not valid json"
    );
}

#[test]
fn invalid_restore_file_is_kept_aside_before_apply() {
    let path = restore_fixture_dir("invalid-shortcuts");
    let mut saved = valid_saved_shortcuts();
    saved.disabled[0].1 = "unknown-key".to_string();
    std::fs::write(
        &path,
        serde_json::to_string(&saved).expect("restore fixture should serialize"),
    )
    .expect("restore fixture should be writable");

    assert!(load_pending_restore(&path, &known_keys()).is_err());
    assert!(!path.exists());
    assert!(unusable_restore_path(&path).exists());
}

#[test]
fn pending_restore_file_returns_recorded_originals() {
    let path = restore_fixture_dir("pending-shortcuts");
    assert!(matches!(
        load_pending_restore(&path, &known_keys()),
        Ok(None)
    ));
    std::fs::write(
        &path,
        serde_json::to_string(&valid_saved_shortcuts()).expect("restore fixture should serialize"),
    )
    .expect("restore fixture should be writable");

    let pending = load_pending_restore(&path, &known_keys())
        .expect("pending restore should load")
        .expect("pending restore should exist");
    assert_eq!(pending.overlay_key, "'Super_L'");
    assert_eq!(pending.disabled[0].2, "['<Alt>F4']");
    assert!(path.exists());
}

#[test]
fn disabling_again_keeps_recorded_originals_over_disabled_values() {
    let pending = valid_saved_shortcuts();
    let captured = SavedShortcuts {
        overlay_key: "''".to_string(),
        dock_hotkeys: "false".to_string(),
        disabled: vec![
            (
                "org.gnome.desktop.wm.keybindings".to_string(),
                "close".to_string(),
                "['<Super>q']".to_string(),
            ),
            (
                "org.gnome.desktop.wm.keybindings".to_string(),
                "switch-applications".to_string(),
                "['<Alt>Tab']".to_string(),
            ),
        ],
    };

    let merged = merge_pending_originals(pending, captured);

    assert_eq!(merged.overlay_key, "'Super_L'");
    assert_eq!(merged.dock_hotkeys, "true");
    assert_eq!(
        merged.disabled,
        vec![
            (
                "org.gnome.desktop.wm.keybindings".to_string(),
                "close".to_string(),
                "['<Alt>F4']".to_string(),
            ),
            (
                "org.gnome.desktop.wm.keybindings".to_string(),
                "switch-applications".to_string(),
                "['<Alt>Tab']".to_string(),
            ),
        ]
    );
}
