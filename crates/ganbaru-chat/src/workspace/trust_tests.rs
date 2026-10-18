use super::*;

fn digest(byte: char) -> FolderConfigDigest {
    FolderConfigDigest::from_hex(&byte.to_string().repeat(64)).unwrap()
}

fn configuration(git: char, provider: char, git_is_empty: bool) -> FolderConfiguration {
    FolderConfiguration {
        git: digest(git),
        provider: digest(provider),
        git_is_empty,
        provider_is_empty: false,
    }
}

fn timestamp() -> UtcTimestamp {
    UtcTimestamp::new("2026-10-18T12:00:00.000Z").unwrap()
}

fn folder_id() -> ProjectWorkingFolderId {
    ProjectWorkingFolderId::new("working-folder-1").unwrap()
}

fn scope_with(record: WorkingFolderTrustRecord) -> WorkingFolderDeviceScope {
    let mut scope = WorkingFolderDeviceScope::default();
    scope.trust.insert(folder_id(), record);
    scope
}

#[test]
fn folders_without_a_record_require_a_decision_when_git_defines_programs() {
    let scope = WorkingFolderDeviceScope::default();
    let check = check_trust_decision(&scope, &folder_id(), &configuration('a', 'b', false));
    assert_eq!(check, TrustDecisionCheck::Required);
    assert_eq!(
        WorkingFolderTrustState::recorded(&scope, &folder_id()),
        WorkingFolderTrustState::Undecided
    );
}

#[test]
fn folders_without_git_programs_have_nothing_to_decide() {
    let scope = WorkingFolderDeviceScope::default();
    let check = check_trust_decision(&scope, &folder_id(), &configuration('a', 'b', true));
    assert_eq!(check, TrustDecisionCheck::NothingToDecide);
}

#[test]
fn a_matching_decline_is_a_decision_that_runs_untrusted() {
    let current = configuration('a', 'b', false);
    let scope = scope_with(trust_record(&current, false, timestamp()));
    assert_eq!(
        check_trust_decision(&scope, &folder_id(), &current),
        TrustDecisionCheck::Decided
    );
    assert_eq!(
        folder_git_trust(&scope, &folder_id()),
        GitFolderTrust::Untrusted
    );
    assert_eq!(
        WorkingFolderTrustState::recorded(&scope, &folder_id()),
        WorkingFolderTrustState::Untrusted
    );
}

#[test]
fn a_changed_git_configuration_asks_again() {
    let scope = scope_with(trust_record(
        &configuration('a', 'b', false),
        true,
        timestamp(),
    ));
    assert_eq!(
        check_trust_decision(&scope, &folder_id(), &configuration('c', 'b', false)),
        TrustDecisionCheck::Required
    );
}

#[test]
fn a_changed_provider_configuration_does_not_block_git_actions() {
    let scope = scope_with(trust_record(
        &configuration('a', 'b', false),
        true,
        timestamp(),
    ));
    assert_eq!(
        check_trust_decision(&scope, &folder_id(), &configuration('a', 'c', false)),
        TrustDecisionCheck::Decided
    );
}

#[test]
fn a_trusted_record_carries_its_git_digest_into_runs() {
    let current = configuration('a', 'b', false);
    let scope = scope_with(trust_record(&current, true, timestamp()));
    assert_eq!(
        folder_git_trust(&scope, &folder_id()),
        GitFolderTrust::Trusted(digest('a'))
    );
    assert_eq!(
        WorkingFolderTrustState::recorded(&scope, &folder_id()),
        WorkingFolderTrustState::Trusted
    );
}

#[test]
fn an_unreadable_trusted_digest_runs_untrusted() {
    let mut record = trust_record(&configuration('a', 'b', false), true, timestamp());
    record.git_configuration_digest = "not-hex".to_string();
    assert_eq!(
        folder_git_trust(&scope_with(record), &folder_id()),
        GitFolderTrust::Untrusted
    );
}

#[test]
fn automatic_records_leave_unseen_provider_configuration_undecided() {
    let with_provider = configuration('a', 'b', true);
    let record = automatic_trust_record(&with_provider, timestamp());
    assert!(record.trusted);
    assert_eq!(record.git_configuration_digest, digest('a').to_hex());
    assert!(record.provider_configuration_digest.is_empty());

    let empty = FolderConfiguration {
        provider_is_empty: true,
        ..with_provider
    };
    let record = automatic_trust_record(&empty, timestamp());
    assert_eq!(record.provider_configuration_digest, digest('b').to_hex());
}

#[test]
fn trust_decision_errors_name_the_folder() {
    let error = trust_decision_required_error(&folder_id());
    assert_eq!(error.code, ChatErrorCode::TrustDecisionRequired);
    assert_eq!(
        error.details.as_deref(),
        Some(&serde_json::json!({ "workingFolderId": "working-folder-1" }))
    );
}

#[test]
fn trust_records_round_trip_through_device_state() {
    let record = trust_record(&configuration('a', 'b', false), true, timestamp());
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["gitConfigurationDigest"], digest('a').to_hex());
    assert_eq!(json["providerConfigurationDigest"], digest('b').to_hex());
    let parsed: WorkingFolderTrustRecord = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, record);
}

fn provider_entries(files: &[(&str, &str)]) -> ProviderConfigurationEntries {
    ProviderConfigurationEntries::from_files(files)
}

fn provider_record(
    entries: &ProviderConfigurationEntries,
    trusted: bool,
) -> WorkingFolderTrustRecord {
    WorkingFolderTrustRecord {
        git_configuration_digest: digest('a').to_hex(),
        provider_configuration_digest: entries.digest().to_hex(),
        trusted,
        decided_at: timestamp(),
    }
}

#[test]
fn folders_without_project_provider_configuration_load_everything_without_asking() {
    let empty = ProviderConfigurationEntries::default();
    let check = check_project_configuration(
        &WorkingFolderDeviceScope::default(),
        &folder_id(),
        &empty,
        &empty,
    );
    assert_eq!(
        check,
        ProjectConfigurationCheck::Decided(ProjectConfigurationAccess::Trusted)
    );
}

#[test]
fn undecided_project_provider_configuration_requires_a_decision() {
    let hooks = provider_entries(&[(".claude/settings.json", "{\"hooks\":{}}")]);
    let scope = WorkingFolderDeviceScope::default();
    assert_eq!(
        check_project_configuration(&scope, &folder_id(), &hooks, &hooks),
        ProjectConfigurationCheck::Required
    );
    let mut automatic = provider_record(&hooks, true);
    automatic.provider_configuration_digest = String::new();
    assert_eq!(
        check_project_configuration(&scope_with(automatic), &folder_id(), &hooks, &hooks),
        ProjectConfigurationCheck::Required
    );
}

#[test]
fn trusted_and_declined_decisions_apply_until_the_configuration_changes() {
    let hooks = provider_entries(&[(".claude/settings.json", "{\"hooks\":{}}")]);
    let trusted = scope_with(provider_record(&hooks, true));
    let declined = scope_with(provider_record(&hooks, false));
    assert_eq!(
        check_project_configuration(&trusted, &folder_id(), &hooks, &hooks),
        ProjectConfigurationCheck::Decided(ProjectConfigurationAccess::Trusted)
    );
    assert_eq!(
        check_project_configuration(&declined, &folder_id(), &hooks, &hooks),
        ProjectConfigurationCheck::Decided(ProjectConfigurationAccess::Restricted)
    );
    let changed = provider_entries(&[
        (".claude/settings.json", "{\"hooks\":{}}"),
        (".claude/skills/marker/SKILL.md", "hooks"),
    ]);
    assert_eq!(
        check_project_configuration(&trusted, &folder_id(), &changed, &changed),
        ProjectConfigurationCheck::Required
    );
}

#[test]
fn worktrees_load_project_configuration_only_when_the_folder_decision_covers_it() {
    let root = provider_entries(&[
        (".claude/settings.json", "{\"hooks\":{}}"),
        (".claude/settings.local.json", "{}"),
    ]);
    let scope = scope_with(provider_record(&root, true));
    let subset = provider_entries(&[(".claude/settings.json", "{\"hooks\":{}}")]);
    assert_eq!(
        check_project_configuration(&scope, &folder_id(), &root, &subset),
        ProjectConfigurationCheck::Decided(ProjectConfigurationAccess::Trusted)
    );
    let edited = provider_entries(&[(".claude/settings.json", "{\"hooks\":{\"x\":1}}")]);
    assert_eq!(
        check_project_configuration(&scope, &folder_id(), &root, &edited),
        ProjectConfigurationCheck::Decided(ProjectConfigurationAccess::Restricted)
    );
    let empty = ProviderConfigurationEntries::default();
    assert_eq!(
        check_project_configuration(
            &WorkingFolderDeviceScope::default(),
            &folder_id(),
            &empty,
            &edited
        ),
        ProjectConfigurationCheck::Decided(ProjectConfigurationAccess::Restricted)
    );
}
