use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(super) fn unique_path(suffix: &str) -> PathBuf {
    // Salt paths with process, time, and sequence values so filesystem
    // tests can share this helper without colliding.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("ganbaru-vault-test-{pid}-{nanos}-{seq}-{suffix}"));
    path
}

#[cfg(unix)]
#[test]
fn app_config_directory_is_created_and_kept_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let root = unique_path("private-config");
    let created = root.join("created");
    make_directory_private(&created).unwrap();
    let mode = fs::metadata(&created).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);

    let shared = root.join("shared");
    fs::create_dir_all(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
    make_directory_private(&shared).unwrap();
    let mode = fs::metadata(&shared).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn write_text_file_atomically_rejects_relative_paths() {
    let err = write_text_file_atomically(
        Path::new("relative/path.txt"),
        "data",
        FileAccess::OwnerOnly,
    )
    .unwrap_err();
    assert_eq!(err, "path must be absolute");
}

#[test]
fn write_text_file_atomically_leaves_only_the_target() {
    let parent = unique_path("write");
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join("write.txt");

    write_text_file_atomically(&path, "payload", FileAccess::OwnerOnly)
        .expect("write should succeed");

    assert_eq!(fs::read_to_string(&path).unwrap(), "payload");
    let names = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from("write.txt")]);
    fs::remove_dir_all(parent).unwrap();
}

/// Atomic vault writes never write through a temporary-name link planted in a shared vault.
#[cfg(unix)]
#[test]
fn atomic_writes_ignore_a_planted_temporary_symlink() {
    let vault = unique_path("planted-temporary");
    fs::create_dir_all(&vault).unwrap();
    initialize_vault(&vault).unwrap();
    let sentinel = unique_path("planted-temporary-sentinel");
    fs::write(&sentinel, "untouched").unwrap();
    std::os::unix::fs::symlink(&sentinel, vault.join("written.json.tmp")).unwrap();

    write_text_file_atomically(&vault.join("written.json"), "{}", FileAccess::OwnerOnly).unwrap();

    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
    let metadata = fs::symlink_metadata(vault.join("written.json")).unwrap();
    assert!(metadata.file_type().is_file());
    fs::remove_dir_all(vault).unwrap();
    fs::remove_file(sentinel).unwrap();
}

#[test]
fn an_oversized_vault_marker_is_refused_before_parsing() {
    let vault = unique_path("oversized-marker");
    fs::create_dir_all(&vault).unwrap();
    initialize_vault(&vault).unwrap();
    let mut contents = fs::read(manifest_path(&vault)).unwrap();
    let limit = usize::try_from(MAX_VAULT_MANIFEST_BYTES).unwrap();
    contents.resize(limit + 1, b' ');
    fs::write(manifest_path(&vault), contents).unwrap();
    let error = vault_info_from_path(&vault).unwrap_err();
    assert!(error.contains("read Ganbaru folder marker"), "{error}");
    let _ = fs::remove_dir_all(vault);
}

#[cfg(unix)]
#[test]
fn opening_a_vault_rejects_symlinked_fixed_entries() {
    for entry in [
        "ganbaru.sqlite",
        "ganbaru.sqlite-wal",
        "vault.lock",
        "assets",
    ] {
        let vault = unique_path("symlinked-entry");
        fs::create_dir_all(&vault).unwrap();
        initialize_vault(&vault).unwrap();
        let target = unique_path("symlinked-entry-target");
        fs::create_dir_all(&target).unwrap();
        let link = vault.join(entry);
        if link.is_dir() {
            fs::remove_dir_all(&link).unwrap();
        } else if link.exists() {
            fs::remove_file(&link).unwrap();
        }
        std::os::unix::fs::symlink(target.join("elsewhere"), &link).unwrap();

        let error = vault_info_from_path(&vault).unwrap_err();
        assert!(error.contains(entry), "{entry}: {error}");
        assert!(initialize_vault(&vault).is_err(), "{entry}");
        fs::remove_dir_all(vault).unwrap();
        fs::remove_dir_all(target).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn opening_a_vault_through_a_symlinked_root_still_works() {
    let vault = unique_path("linked-root");
    fs::create_dir_all(&vault).unwrap();
    let created = initialize_vault(&vault).unwrap();
    let alias = unique_path("linked-root-alias");
    std::os::unix::fs::symlink(&vault, &alias).unwrap();

    let opened = vault_info_from_path(&alias).unwrap();
    assert_eq!(opened.vault_id, created.vault_id);
    assert!(created.created);
    assert!(!opened.created);
    fs::remove_file(alias).unwrap();
    fs::remove_dir_all(vault).unwrap();
}

#[test]
fn initialize_vault_creates_manifest_database_and_dirs_without_config() {
    let path = unique_path("portable-vault");
    fs::create_dir_all(&path).expect("create vault folder");

    let info = initialize_vault(&path).expect("initialize vault");

    assert_eq!(
        info.database_path,
        path.join(APP_SQLITE_FILE).to_string_lossy()
    );
    assert!(path.join(VAULT_MANIFEST_FILE).exists());
    assert!(!path.join("config.json").exists());
    assert!(path.join(APP_SQLITE_FILE).exists());
    assert!(path.join("projects").is_dir());
    assert!(path.join("assets").is_dir());
    assert!(!path.join("notes").exists());
    assert!(!path.join(".yjs").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path.join(APP_SQLITE_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let _ = fs::remove_dir_all(&path);
}

#[test]
#[cfg(desktop)]
fn background_vault_resolution_waits_for_selection_but_rejects_invalid_manifests() {
    let path = unique_path("background-vault");
    assert_eq!(available_vault_path(None).unwrap(), None);
    assert_eq!(available_vault_path(Some(path.clone())).unwrap(), None);
    fs::create_dir_all(&path).unwrap();
    assert!(available_vault_path(Some(path.clone())).is_err());
    initialize_vault(&path).unwrap();
    assert_eq!(
        available_vault_path(Some(path.clone())).unwrap(),
        Some(path.clone())
    );
    fs::write(path.join(VAULT_MANIFEST_FILE), "invalid json").unwrap();
    assert!(available_vault_path(Some(path.clone())).is_err());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn vault_markers_refuse_ids_that_could_name_other_key_files_or_accounts() {
    let marker = |vault_id: &str| {
        serde_json::json!({
            "app": VAULT_APP_MARKER,
            "schemaVersion": VAULT_SCHEMA_VERSION,
            "vaultId": vault_id,
            "displayName": "Vault",
            "createdAt": "2026-10-20T00:00:00Z",
            "hasData": false,
        })
        .to_string()
    };
    let generated = new_vault_id();
    assert_eq!(
        parse_vault_manifest(marker(&generated).as_bytes())
            .unwrap()
            .vault_id,
        generated
    );
    for vault_id in [
        "",
        "../other",
        "a/b",
        "a\\b",
        "vault:other",
        "vault\nid",
        &"v".repeat(161),
    ] {
        assert!(
            parse_vault_manifest(marker(vault_id).as_bytes()).is_err(),
            "{vault_id:?}"
        );
    }
}

#[test]
fn initialize_vault_reports_creation_only_for_a_new_vault() {
    let path = unique_path("created-flag");
    fs::create_dir_all(&path).unwrap();

    let created = initialize_vault(&path).unwrap();
    let reopened = initialize_vault(&path).unwrap();

    assert!(created.created);
    assert!(!reopened.created);
    assert_eq!(reopened.vault_id, created.vault_id);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn initialize_vault_rejects_non_empty_non_vault_folders() {
    let path = unique_path("non-empty");
    fs::create_dir_all(&path).expect("create vault folder");
    fs::write(path.join("random.txt"), "existing file").expect("seed file");

    let err = initialize_vault(&path).unwrap_err();

    assert_eq!(
        err,
        "selected folder is not empty and is not a Ganbaru folder"
    );
    let _ = fs::remove_dir_all(&path);
}

#[test]
fn folders_with_a_foreign_or_unsupported_marker_are_refused_before_any_write() {
    let markers = [
        (
            serde_json::json!({
                "app": "another-app",
                "schemaVersion": VAULT_SCHEMA_VERSION,
                "vaultId": "vault",
                "displayName": "Vault",
                "createdAt": "2026-10-20T00:00:00Z",
                "hasData": false,
            }),
            "selected folder is not a Ganbaru folder".to_string(),
        ),
        (
            serde_json::json!({
                "app": VAULT_APP_MARKER,
                "schemaVersion": VAULT_SCHEMA_VERSION + 1,
                "vaultId": "vault",
                "displayName": "Vault",
                "createdAt": "2026-10-20T00:00:00Z",
                "hasData": false,
            }),
            format!(
                "unsupported Ganbaru folder schema version {}",
                VAULT_SCHEMA_VERSION + 1
            ),
        ),
    ];
    for (marker, expected) in markers {
        let path = unique_path("foreign-marker");
        fs::create_dir_all(&path).expect("create folder");
        fs::write(path.join(VAULT_MANIFEST_FILE), marker.to_string()).expect("seed marker");

        assert_eq!(initialize_vault(&path).unwrap_err(), expected);
        assert_eq!(validate_vault_candidate(&path).unwrap_err(), expected);

        let entries = fs::read_dir(&path)
            .expect("read folder")
            .map(|entry| entry.expect("folder entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [std::ffi::OsString::from(VAULT_MANIFEST_FILE)]);
        let _ = fs::remove_dir_all(&path);
    }
}

#[test]
fn validating_an_existing_vault_completes_its_skeleton() {
    let path = unique_path("existing-vault");
    fs::create_dir_all(&path).expect("create vault folder");
    let created = initialize_vault(&path).expect("initialize vault");
    fs::remove_dir_all(path.join("assets")).expect("remove assets");
    fs::write(path.join("config.json"), "{\"chat\":{}}").expect("write stray config");

    let opened = validate_vault_candidate(&path).expect("validate vault");

    assert_eq!(opened.vault_id, created.vault_id);
    assert!(path.join("assets").is_dir());
    assert_eq!(
        fs::read_to_string(path.join("config.json")).expect("read stray config"),
        "{\"chat\":{}}",
        "an existing config.json is left as it was"
    );
    let _ = fs::remove_dir_all(&path);
}

#[test]
fn default_vault_name_tracks_build_mode() {
    if cfg!(debug_assertions) {
        assert_eq!(default_vault_name(), DEVELOPMENT_VAULT_NAME);
    } else {
        assert_eq!(default_vault_name(), PRODUCTION_VAULT_NAME);
    }
}

#[test]
fn recent_vault_ids_refuse_to_guess_while_a_folder_is_unreachable() {
    let vault = unique_path("recent-vault");
    fs::create_dir_all(&vault).expect("create vault folder");
    let info = initialize_vault(&vault).expect("initialize vault");
    let plain = unique_path("recent-plain");
    fs::create_dir_all(&plain).expect("create plain folder");
    let reachable = vec![
        vault.to_string_lossy().into_owned(),
        plain.to_string_lossy().into_owned(),
    ];

    assert_eq!(
        recent_vault_ids(&reachable),
        Some(BTreeSet::from([info.vault_id]))
    );
    let mut with_missing = reachable.clone();
    with_missing.push(unique_path("recent-missing").to_string_lossy().into_owned());
    assert_eq!(recent_vault_ids(&with_missing), None);
    let _ = fs::remove_dir_all(&vault);
    let _ = fs::remove_dir_all(&plain);
}

#[test]
fn vault_scopes_keep_only_known_vaults() {
    let mut state = VaultAppState::default();
    for vault_id in ["vault-live", "vault-deleted"] {
        state
            .music_root_bindings
            .insert(vault_id.to_string(), BTreeMap::new());
        state
            .music_soundscape_paths
            .insert(vault_id.to_string(), BTreeMap::new());
        state
            .project_working_folders
            .vaults
            .insert(vault_id.to_string(), BTreeMap::new());
        state
            .chat
            .vaults
            .insert(vault_id.to_string(), BTreeMap::new());
    }

    retain_vault_scopes(&mut state, &BTreeSet::from(["vault-live".to_string()]));

    let live = ["vault-live"];
    assert_eq!(
        state.music_root_bindings.keys().collect::<Vec<_>>(),
        live.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        state.music_soundscape_paths.keys().collect::<Vec<_>>(),
        live.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        state
            .project_working_folders
            .vaults
            .keys()
            .collect::<Vec<_>>(),
        live.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        state.chat.vaults.keys().collect::<Vec<_>>(),
        live.iter().collect::<Vec<_>>()
    );
}

#[test]
fn app_state_round_trips_active_and_recent_vault_paths() {
    let path = unique_path("app-state.json");
    let mut music_root_bindings = BTreeMap::new();
    music_root_bindings.insert(
        "vault-1".to_string(),
        BTreeMap::from([(
            "soundtracks".to_string(),
            "/mnt/music/soundtracks".to_string(),
        )]),
    );
    let state = VaultAppState {
        device_id: Some("device-test".to_string()),
        active_vault_path: Some("/tmp/ganbaru-vault".to_string()),
        recent_vault_paths: vec!["/tmp/ganbaru-vault".to_string()],
        music_root_bindings,
        project_working_folders: Default::default(),
        chat: Default::default(),
        music_path_grants: Default::default(),
        music_soundscape_paths: Default::default(),
        starter_vault_id: None,
    };

    write_app_state_to_path(&path, &state).expect("write app state");
    let saved = read_app_state_from_path(&path).expect("read app state");

    assert_eq!(saved.active_vault_path, state.active_vault_path);
    assert_eq!(saved.device_id, state.device_id);
    assert_eq!(saved.recent_vault_paths, state.recent_vault_paths);
    assert_eq!(saved.music_root_bindings, state.music_root_bindings);
    let _ = fs::remove_file(&path);
}

#[test]
fn starter_record_names_one_vault_and_never_returns_after_another_is_selected() {
    let mut state = VaultAppState::default();
    assert!(!is_untouched_starter(&state, "vault-starter"));

    state.starter_vault_id = Some("vault-starter".to_string());
    assert!(is_untouched_starter(&state, "vault-starter"));
    assert!(!is_untouched_starter(&state, "vault-imported"));

    forget_starter_unless(&mut state, "vault-starter");
    assert!(is_untouched_starter(&state, "vault-starter"));

    forget_starter_unless(&mut state, "vault-imported");
    assert_eq!(state.starter_vault_id, None);
    forget_starter_unless(&mut state, "vault-starter");
    assert!(!is_untouched_starter(&state, "vault-starter"));
}

#[test]
fn app_state_cache_reuses_parse_until_the_file_changes() {
    let path = unique_path("cached-app-state.json");
    let mut cache = None;

    let missing = cached_app_state(&mut cache, &path).expect("read missing app state");
    assert_eq!(*missing, VaultAppState::default());

    let mut state = VaultAppState {
        device_id: Some("device-test".to_string()),
        ..VaultAppState::default()
    };
    write_app_state_to_path(&path, &state).expect("write app state");
    let first = cached_app_state(&mut cache, &path).expect("read written app state");
    let second = cached_app_state(&mut cache, &path).expect("read cached app state");
    assert_eq!(first.device_id.as_deref(), Some("device-test"));
    assert!(Arc::ptr_eq(&first, &second));

    // A different length changes the stamp even on coarse-timestamp filesystems.
    state.active_vault_path = Some("/tmp/ganbaru-external-vault".to_string());
    write_app_state_to_path(&path, &state).expect("write external change");
    let changed = cached_app_state(&mut cache, &path).expect("read changed app state");
    assert_eq!(changed.active_vault_path, state.active_vault_path);
    assert!(!Arc::ptr_eq(&first, &changed));

    fs::remove_file(&path).expect("remove app state");
    let removed = cached_app_state(&mut cache, &path).expect("read removed app state");
    assert_eq!(*removed, VaultAppState::default());
}

#[test]
fn app_state_rejects_missing_current_device_state_sections() {
    let path = unique_path("incomplete-app-state.json");
    fs::write(
        &path,
        r#"{"activeVaultPath":"/tmp/vault","recentVaultPaths":[]}"#,
    )
    .expect("write incomplete state");

    let state = read_app_state_from_path(&path);

    assert!(state.is_err());
    let _ = fs::remove_file(&path);
}

#[test]
fn app_state_requires_current_nullable_fields() {
    let current = serde_json::to_value(VaultAppState::default()).expect("serialize app state");
    assert!(serde_json::from_value::<VaultAppState>(current.clone()).is_ok());

    for field in ["deviceId", "activeVaultPath"] {
        let mut incomplete = current.clone();
        incomplete.as_object_mut().unwrap().remove(field);

        assert!(
            serde_json::from_value::<VaultAppState>(incomplete).is_err(),
            "missing {field} must be rejected"
        );
    }
}

#[test]
fn provider_probe_requires_current_authority_support_fields() {
    let path = unique_path("incomplete-provider-probe-app-state.json");
    fs::write(
        &path,
        r#"{
  "deviceId": "device-test",
  "activeVaultPath": null,
  "recentVaultPaths": [],
  "musicRootBindings": {},
  "projectWorkingFolders": { "schemaVersion": 1, "vaults": {} },
  "chat": {
    "schemaVersion": 1,
    "vaults": {
      "vault-test": {
        "device-test": {
          "providerInstances": {
            "codex": {
              "executablePath": null,
              "providerHomePath": null,
              "lastProbe": {
                "instanceId": "codex",
                "state": "healthy",
                "version": "1.0.0",
                "negotiatedProtocolVersion": null,
                "accountLabel": null,
                "capabilities": { "entries": [] },
                "authoritySupport": {
                  "internalHostTools": true,
                  "denyShell": true,
                  "readOnlyRoot": true,
                  "writableRoot": true,
                  "confinedCommands": true,
                  "networkBoundary": true,
                  "classifiedPublish": true
                },
                "checkedAt": "2026-08-22T00:00:00Z",
                "detail": null
              },
              "lastSuccessfulProbeAt": null,
              "modelCatalog": null
            }
          },
          "fullAccessTrust": {},
          "preferences": {
            "lastSelectedThreadId": null
          },
          "diagnostics": { "captureEnabled": false, "retentionDays": 7 },
          "executionEnvironmentPaths": {}
        }
      }
    }
  }
}"#,
    )
    .expect("write incomplete provider probe state");

    let state = read_app_state_from_path(&path);

    assert!(state.is_err());
    let _ = fs::remove_file(&path);
}

/// A test vault activated in a fresh config directory, removed on drop.
struct OpenedVault {
    config: PathBuf,
    root: PathBuf,
}

impl OpenedVault {
    fn new(name: &str) -> Self {
        let config = unique_path(&format!("{name}-config"));
        let database =
            activate_test_vault(&config, &unique_path(name), "data-vault", "device-a").unwrap();
        let root = database.parent().unwrap().to_path_buf();
        Self { config, root }
    }

    async fn open(&self) -> Result<(), String> {
        let registry = crate::db::DatabaseState::default();
        let opened = crate::db::connect_vault_for_sync_in(&registry, &self.config).await;
        registry.close_all().await.unwrap();
        opened.map(|_| ())
    }

    fn has_data(&self) -> bool {
        read_vault_manifest(&self.root).unwrap().has_data
    }
}

impl Drop for OpenedVault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.config);
    }
}

#[tokio::test]
async fn the_first_open_records_that_the_vault_holds_data() {
    let vault = OpenedVault::new("has-data-first-open");
    assert!(!vault.has_data());
    vault.open().await.unwrap();
    assert!(vault.has_data());
    vault.open().await.unwrap();
    assert!(vault.has_data());
}

#[tokio::test]
async fn a_vault_that_held_data_never_reopens_empty() {
    for lost in ["missing", "empty"] {
        let vault = OpenedVault::new("has-data-lost");
        vault.open().await.unwrap();
        let database = database_path(&vault.root);
        fs::remove_file(&database).unwrap();
        if lost == "empty" {
            fs::write(&database, b"").unwrap();
        }
        let marker = fs::read(manifest_path(&vault.root)).unwrap();

        assert_eq!(
            vault.open().await,
            Err(VAULT_DATABASE_MISSING.to_string()),
            "{lost}"
        );
        assert_eq!(
            fs::metadata(&database).map(|meta| meta.len()).ok(),
            (lost == "empty").then_some(0),
            "{lost}"
        );
        assert_eq!(
            fs::read(manifest_path(&vault.root)).unwrap(),
            marker,
            "{lost}"
        );
    }
}

#[tokio::test]
async fn a_vault_reset_reopens_empty_and_holds_data_again() {
    let vault = OpenedVault::new("has-data-reset");
    vault.open().await.unwrap();
    database_rollback::move_database_to_rollback(&vault.root, now_utc())
        .unwrap()
        .unwrap();
    assert!(!vault.has_data());
    vault.open().await.unwrap();
    assert!(vault.has_data());
}

#[tokio::test]
async fn a_marker_without_the_data_fact_is_refused() {
    let vault = OpenedVault::new("has-data-old-marker");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path(&vault.root)).unwrap()).unwrap();
    marker.as_object_mut().unwrap().remove("hasData");
    fs::write(manifest_path(&vault.root), marker.to_string()).unwrap();
    assert!(vault.open().await.is_err());
    assert!(read_vault_manifest(&vault.root).is_err());
}
