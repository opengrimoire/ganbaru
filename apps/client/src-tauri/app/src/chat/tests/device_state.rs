use crate::chat::device_state::{
    ChatDeviceScope, ChatDeviceState, full_access_is_trusted, set_full_access_trust,
};
use crate::projects::working_folders::{
    ProjectWorkingFolderBindingState, WORKING_FOLDER_DEVICE_STATE_SCHEMA_VERSION,
};
use ganbaru_chat_contracts::models::{
    ProjectWorkingFolderId, ProviderInstanceId, RepositoryKind, UtcTimestamp,
};
use ganbaru_working_folders::WorkingFolderDeviceState;

#[test]
fn bindings_are_scoped_by_vault_and_device() {
    let mut state = WorkingFolderDeviceState::default();
    let working_folder_id = ProjectWorkingFolderId::new("workspace-1").unwrap();
    let binding = ProjectWorkingFolderBindingState {
        canonical_path: "/mnt/work/ganbaru".to_string(),
        filesystem_identity: "filesystem-sha256:folder".to_string(),
        repository_kind: RepositoryKind::Git,
        repository_identity: Some("git:example/ganbaru".to_string()),
        repository_storage_identity: Some("filesystem-sha256:git".to_string()),
        last_verified_at: UtcTimestamp::new("2026-07-20T12:00:00Z").unwrap(),
    };

    let first_device = state.scope_mut("vault-1", "device-1");
    first_device
        .bindings
        .insert(working_folder_id.clone(), binding.clone());

    assert_eq!(
        state
            .scope("vault-1", "device-1")
            .and_then(|scope| scope.bindings.get(&working_folder_id)),
        Some(&binding)
    );
    assert!(state.scope("vault-1", "device-2").is_none());
    assert!(state.scope("vault-2", "device-1").is_none());
}

#[test]
fn working_folder_device_state_round_trips_typed_map_keys() {
    let mut state = WorkingFolderDeviceState::default();
    state.scope_mut("vault-1", "device-1").bindings.insert(
        ProjectWorkingFolderId::new("workspace-1").unwrap(),
        ProjectWorkingFolderBindingState {
            canonical_path: "/mnt/work/ganbaru".to_string(),
            filesystem_identity: "filesystem-sha256:folder".to_string(),
            repository_kind: RepositoryKind::Git,
            repository_identity: Some("git:example/ganbaru".to_string()),
            repository_storage_identity: Some("filesystem-sha256:git".to_string()),
            last_verified_at: UtcTimestamp::new("2026-07-20T12:00:00Z").unwrap(),
        },
    );

    let serialized = serde_json::to_value(&state).unwrap();
    let restored: WorkingFolderDeviceState = serde_json::from_value(serialized).unwrap();

    assert_eq!(restored, state);
    assert_eq!(
        restored.schema_version,
        WORKING_FOLDER_DEVICE_STATE_SCHEMA_VERSION
    );
}

#[test]
fn working_folder_binding_requires_current_identity_fields() {
    let binding = serde_json::from_value::<ProjectWorkingFolderBindingState>(serde_json::json!({
        "canonicalPath": "/mnt/work/ganbaru",
        "repositoryKind": "git",
        "repositoryIdentity": "git-sha256:repository",
        "repositoryStorageIdentity": null,
        "lastVerifiedAt": "2026-07-20T12:00:00Z"
    }));

    assert!(binding.is_err());
}

#[test]
fn app_state_requires_the_current_device_state_sections() {
    let restored = serde_json::from_value::<crate::vault::VaultAppState>(serde_json::json!({
        "activeVaultPath": null,
        "recentVaultPaths": []
    }));

    assert!(restored.is_err());
}

#[test]
fn device_scope_requires_current_preferences_and_diagnostics() {
    let scope = serde_json::from_value::<ChatDeviceScope>(serde_json::json!({}));
    assert!(scope.is_err());
}

#[test]
fn full_access_trust_never_crosses_provider_workspace_vault_or_device_boundaries() {
    let mut state = ChatDeviceState::default();
    let trusted_provider = ProviderInstanceId::new("codex-personal").unwrap();
    let incompatible_provider = ProviderInstanceId::new("codex-other-home").unwrap();
    let trusted_workspace = ProjectWorkingFolderId::new("workspace-1").unwrap();
    let new_workspace = ProjectWorkingFolderId::new("workspace-2").unwrap();
    let timestamp = UtcTimestamp::new("2026-07-21T12:00:00Z").unwrap();
    set_full_access_trust(
        state.scope_mut("vault-1", "device-1"),
        trusted_provider.clone(),
        trusted_workspace.clone(),
        Some(timestamp),
    );

    let trusted_scope = state.scope("vault-1", "device-1").unwrap();
    assert!(full_access_is_trusted(
        trusted_scope,
        &trusted_provider,
        &trusted_workspace,
    ));
    assert!(!full_access_is_trusted(
        trusted_scope,
        &trusted_provider,
        &new_workspace,
    ));
    assert!(!full_access_is_trusted(
        trusted_scope,
        &incompatible_provider,
        &trusted_workspace,
    ));
    assert!(!full_access_is_trusted(
        state.scope_mut("vault-1", "device-2"),
        &trusted_provider,
        &trusted_workspace,
    ));
    assert!(!full_access_is_trusted(
        state.scope_mut("vault-2", "device-1"),
        &trusted_provider,
        &trusted_workspace,
    ));

    set_full_access_trust(
        state.scope_mut("vault-1", "device-1"),
        trusted_provider.clone(),
        trusted_workspace.clone(),
        None,
    );
    assert!(!full_access_is_trusted(
        state.scope("vault-1", "device-1").unwrap(),
        &trusted_provider,
        &trusted_workspace,
    ));
}
