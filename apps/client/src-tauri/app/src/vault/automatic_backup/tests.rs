use super::*;
use serde_json::json;
use std::path::PathBuf;

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn enabled(interval: BackupInterval) -> Schedule {
    Schedule {
        enabled: true,
        interval,
        ..Schedule::default()
    }
}

fn unique_test_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ganbaru-automatic-backup-test-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn a_missing_entry_is_off_and_daily() {
    let schedule = parse_schedule(None).unwrap();
    assert!(!schedule.enabled);
    assert_eq!(schedule.interval, BackupInterval::Daily);
    assert_eq!(schedule.folder, None);
}

#[test]
fn stored_schedules_round_trip_and_reject_unknown_shapes() {
    let schedule = Schedule {
        enabled: true,
        interval: BackupInterval::SixHours,
        folder: Some(CustomFolder::Tree {
            uri: "content://tree/primary%3ABackups".to_string(),
            name: "Backups".to_string(),
        }),
        last_backup_at: Some(at("2026-11-22T10:00:00Z")),
        failure: Some(Failure {
            kind: FailureKind::FolderUnavailable,
            at: at("2026-11-22T11:00:00Z"),
        }),
    };
    let value = serde_json::to_value(&schedule).unwrap();
    assert_eq!(value["interval"], "sixHours");
    assert_eq!(value["folder"]["kind"], "tree");
    assert_eq!(value["failure"]["kind"], "folderUnavailable");
    assert_eq!(parse_schedule(Some(&value)).unwrap(), schedule);

    let mut extra = value.clone();
    extra["retained"] = json!(3);
    assert!(parse_schedule(Some(&extra)).is_err());
    let mut folder_extra = value.clone();
    folder_extra["folder"]["path"] = json!("/tmp");
    assert!(parse_schedule(Some(&folder_extra)).is_err());
    let mut interval = value;
    interval["interval"] = json!("hourly");
    assert!(parse_schedule(Some(&interval)).is_err());
}

#[test]
fn a_backup_is_due_at_once_without_a_last_one_and_never_while_off() {
    let now = at("2026-11-22T10:00:00Z");
    assert_eq!(next_attempt(&Schedule::default(), now), None);
    assert_eq!(
        next_attempt(&enabled(BackupInterval::Weekly), now),
        Some(now)
    );
}

#[test]
fn a_backup_is_due_one_interval_after_the_last() {
    let now = at("2026-11-22T10:00:00Z");
    for (interval, due) in [
        (BackupInterval::SixHours, "2026-11-22T15:00:00Z"),
        (BackupInterval::Daily, "2026-11-23T09:00:00Z"),
        (BackupInterval::Weekly, "2026-11-29T09:00:00Z"),
    ] {
        let schedule = Schedule {
            last_backup_at: Some(at("2026-11-22T09:00:00Z")),
            ..enabled(interval)
        };
        assert_eq!(next_attempt(&schedule, now), Some(at(due)), "{interval:?}");
    }
    let overdue = Schedule {
        last_backup_at: Some(at("2026-11-01T00:00:00Z")),
        ..enabled(BackupInterval::Daily)
    };
    assert!(next_attempt(&overdue, now).unwrap() <= now);
}

#[test]
fn a_last_backup_in_the_future_waits_one_interval_from_now() {
    let now = at("2026-11-22T10:00:00Z");
    let schedule = Schedule {
        last_backup_at: Some(at("2027-03-01T00:00:00Z")),
        ..enabled(BackupInterval::Daily)
    };
    assert_eq!(
        next_attempt(&schedule, now),
        Some(at("2026-11-23T10:00:00Z"))
    );
}

#[test]
fn a_failure_delays_the_next_attempt_by_an_hour() {
    let now = at("2026-11-22T10:30:00Z");
    let failed = Schedule {
        failure: Some(Failure {
            kind: FailureKind::Failed,
            at: at("2026-11-22T10:00:00Z"),
        }),
        ..enabled(BackupInterval::SixHours)
    };
    assert_eq!(next_attempt(&failed, now), Some(at("2026-11-22T11:00:00Z")));
    let failed_in_the_future = Schedule {
        failure: Some(Failure {
            kind: FailureKind::Failed,
            at: at("2027-01-01T00:00:00Z"),
        }),
        ..failed.clone()
    };
    assert_eq!(
        next_attempt(&failed_in_the_future, now),
        Some(at("2026-11-22T11:30:00Z"))
    );
    let due_later = Schedule {
        last_backup_at: Some(at("2026-11-22T09:00:00Z")),
        ..failed
    };
    assert_eq!(
        next_attempt(&due_later, now),
        Some(at("2026-11-22T15:00:00Z"))
    );
}

#[test]
fn backup_names_tag_the_vault_and_sort_by_time() {
    let names = BackupNames::for_vault("vault-1a2b-3c4d");
    let other = BackupNames::for_vault("vault-1a2b-3c4e");
    assert_ne!(names.prefix, other.prefix);
    assert!(names.prefix.starts_with("ganbaru-auto-backup-"));
    assert_eq!(
        names.prefix.len(),
        "ganbaru-auto-backup-".len() + 2 * VAULT_TAG_BYTES + 1
    );
    let file_name = stamped_file_name(&names.stem);
    assert!(file_name.starts_with(&names.prefix));
    assert!(file_name.ends_with(".ganbaru-backup"));
}

#[test]
fn pruning_keeps_the_newest_backups_of_the_vault_only() {
    let prefix = "ganbaru-auto-backup-0a0b0c0d-";
    let mut names: Vec<String> = (1..=7)
        .map(|day| format!("{prefix}2026-11-{day:02}T10-00-00Z.ganbaru-backup"))
        .collect();
    names.reverse();
    names.push("ganbaru-auto-backup-ffffffff-2026-10-01T10-00-00Z.ganbaru-backup".to_string());
    names.push("ganbaru-backup-2026-10-01T10-00-00Z.ganbaru-backup".to_string());
    names.push(format!("{prefix}2026-10-01T10-00-00Z.ganbaru-backup.part"));
    assert_eq!(
        expired_backups(names, prefix),
        vec![
            format!("{prefix}2026-11-01T10-00-00Z.ganbaru-backup"),
            format!("{prefix}2026-11-02T10-00-00Z.ganbaru-backup"),
        ]
    );
    let few = vec![format!("{prefix}2026-11-01T10-00-00Z.ganbaru-backup")];
    assert!(expired_backups(few, prefix).is_empty());
}

#[test]
fn the_default_folder_is_named_after_the_vault_folder() {
    assert_eq!(
        default_folder_name(Path::new("/home/someone/Documents/Ganbaru")).unwrap(),
        "Ganbaru Backup"
    );
    assert_eq!(
        default_folder_name(Path::new("/data/Ganbaru Dev")).unwrap(),
        "Ganbaru Dev Backup"
    );
    assert!(default_folder_name(Path::new("/")).is_err());
}

#[cfg(desktop)]
#[test]
fn the_default_desktop_folder_sits_beside_the_vault() {
    assert_eq!(
        default_directory(Path::new("/media/disk/Ganbaru")).unwrap(),
        PathBuf::from("/media/disk/Ganbaru Backup")
    );
}

#[test]
fn the_status_reports_the_schedule_and_folder() {
    let vault_root = Path::new("/media/disk/Ganbaru");
    let now = at("2026-11-22T10:00:00Z");
    let schedule = Schedule {
        last_backup_at: Some(at("2026-11-22T09:00:00Z")),
        folder: Some(CustomFolder::Folder {
            path: "/media/backups".to_string(),
        }),
        ..enabled(BackupInterval::Daily)
    };
    let status = status_for(vault_root, &schedule, now, true).unwrap();
    assert_eq!(
        status.folder,
        FolderStatus {
            label: "/media/backups".to_string(),
            custom: true
        }
    );
    assert_eq!(
        status.last_backup_at.as_deref(),
        Some("2026-11-22T09:00:00Z")
    );
    assert_eq!(
        status.next_backup_at.as_deref(),
        Some("2026-11-23T09:00:00Z")
    );
    assert!(status.running);
    let off = status_for(vault_root, &Schedule::default(), now, false).unwrap();
    assert!(!off.folder.custom);
    assert_eq!(off.next_backup_at, None);
    let value = serde_json::to_value(&off).unwrap();
    assert_eq!(value["interval"], "daily");
    assert_eq!(value["lastBackupAt"], serde_json::Value::Null);
}

#[cfg(desktop)]
#[test]
fn desktop_folders_inside_the_vault_are_unavailable() {
    let root = unique_test_path("inside");
    let vault = root.join("Ganbaru");
    let inner = vault.join("assets");
    std::fs::create_dir_all(&inner).unwrap();
    assert!(inside_vault(&inner, &vault));
    assert!(inside_vault(&vault, &vault));
    assert!(!inside_vault(&root, &vault));
    let inner_folder = CustomFolder::Folder {
        path: inner.display().to_string(),
    };
    assert!(matches!(
        prepare_directory(&vault, Some(&inner_folder)),
        Err(AttemptError::FolderUnavailable(_))
    ));
    let missing = CustomFolder::Folder {
        path: root.join("missing").display().to_string(),
    };
    assert!(matches!(
        prepare_directory(&vault, Some(&missing)),
        Err(AttemptError::FolderUnavailable(_))
    ));
    let created = prepare_directory(&vault, None).unwrap();
    assert_eq!(created, root.join("Ganbaru Backup"));
    assert!(created.is_dir());
    assert_eq!(prepare_directory(&vault, None).unwrap(), created);
    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(desktop)]
#[test]
fn desktop_pruning_deletes_only_expired_regular_files() {
    let directory = unique_test_path("prune");
    std::fs::create_dir_all(&directory).unwrap();
    let prefix = "ganbaru-auto-backup-0a0b0c0d-";
    for day in 1..=6 {
        std::fs::write(
            directory.join(format!("{prefix}2026-11-{day:02}T10-00-00Z.ganbaru-backup")),
            b"backup",
        )
        .unwrap();
    }
    let kept_folder = directory.join(format!("{prefix}2026-10-01T10-00-00Z.ganbaru-backup"));
    std::fs::create_dir(&kept_folder).unwrap();
    std::fs::write(directory.join("notes.txt"), b"keep").unwrap();
    prune_directory(&directory, prefix).unwrap();
    let mut remaining: Vec<String> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    remaining.sort();
    assert_eq!(remaining.len(), RETAINED_BACKUPS + 2);
    assert!(!remaining.contains(&format!("{prefix}2026-11-01T10-00-00Z.ganbaru-backup")));
    assert!(kept_folder.is_dir());
    assert!(directory.join("notes.txt").is_file());
    std::fs::remove_dir_all(&directory).unwrap();
}
