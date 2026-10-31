//! Access-profile lifecycle and immutable revision persistence.

use super::retained_references::retained_profile_reference_issues_in_transaction;
use super::{
    ArchiveChatAccessProfileCommand, ChatAccessIssueCode, ChatAccessProfileId,
    ChatAccessProfileImpactPreview, ChatAccessProfileRead, ChatAccessProfileRevision,
    ChatAccessProfileRevisionId, ChatAccessProfileRevisionInput, ChatChannelCapabilities,
    ChatChannelId, ChatError, ChatErrorCode, ChatHistoryBoundary, ChatParticipantId, ChatResult,
    CreateChatAccessProfileCommand, DuplicateChatAccessProfileCommand,
    PreviewChatAccessProfileRevisionCommand, PublishChatAccessProfileRevisionCommand, UtcTimestamp,
};
use super::{history_boundary_is_expansion, issue, revoke_teammate_authorizations};
use crate::chat::channels::{
    chat_pool, i64_value, identifier_error, now_timestamp, optional_timestamp, persistence_error,
    timestamp, u64_value,
};
use crate::chat::organization::common::{
    from_grant_lower_order_key, new_id, parse_access_profile_builtin_key, parse_folder_capability,
    parse_history_boundary, wire_folder_capability, wire_history_boundary,
};
use ganbaru_chat::coordination::access_profiles::profile_revision_is_expansion;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::BTreeSet;

#[tauri::command]
pub async fn chat_list_access_profiles(
    app: tauri::AppHandle,
    db_url: String,
    archived: bool,
) -> ChatResult<Vec<ChatAccessProfileRead>> {
    let pool = chat_pool(app, db_url).await?;
    let rows = sqlx::query(
        "SELECT id FROM chat_access_profiles
         WHERE (archived_at IS NOT NULL) = ?
         ORDER BY builtin_key IS NULL, display_name COLLATE NOCASE, id",
    )
    .bind(archived)
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let mut profiles = Vec::with_capacity(rows.len());
    for row in rows {
        let id =
            ChatAccessProfileId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                .map_err(identifier_error)?;
        profiles.push(read_access_profile(&pool, &id).await?);
    }
    Ok(profiles)
}

#[tauri::command]
pub async fn chat_create_access_profile(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateChatAccessProfileCommand,
) -> ChatResult<ChatAccessProfileRead> {
    let display_name = validate_access_profile_name(&request.display_name)?;
    validate_profile_revision_input(&request.revision)?;
    let pool = chat_pool(app, db_url).await?;
    let now = now_timestamp()?;
    let revision_id = new_id("access-profile-revision")?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    require_unused_access_profile_name(&mut transaction, &display_name).await?;
    sqlx::query(
        "INSERT INTO chat_access_profiles
            (id, display_name, current_revision_id, current_maximum_folder_capability,
             revision, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(request.access_profile_id.as_str())
    .bind(display_name)
    .bind(revision_id.as_str())
    .bind(wire_folder_capability(
        request.revision.maximum_folder_capability,
    ))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    insert_access_profile_revision(
        &mut transaction,
        &request.access_profile_id,
        &revision_id,
        &request.revision,
        &now,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    read_access_profile(&pool, &request.access_profile_id).await
}

#[tauri::command]
pub async fn chat_duplicate_access_profile(
    app: tauri::AppHandle,
    db_url: String,
    request: DuplicateChatAccessProfileCommand,
) -> ChatResult<ChatAccessProfileRead> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let source = read_access_profile(&pool, &request.source_access_profile_id).await?;
    chat_create_access_profile(
        app,
        db_url,
        CreateChatAccessProfileCommand {
            access_profile_id: request.access_profile_id,
            display_name: request.display_name,
            revision: ChatAccessProfileRevisionInput {
                default_channel_capabilities: source.latest_revision.default_channel_capabilities,
                default_history_boundary: source.latest_revision.default_history_boundary,
                maximum_folder_capability: source.latest_revision.maximum_folder_capability,
            },
        },
    )
    .await
}

#[tauri::command]
pub async fn chat_preview_access_profile_revision(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewChatAccessProfileRevisionCommand,
) -> ChatResult<ChatAccessProfileImpactPreview> {
    validate_profile_revision_input(&request.revision)?;
    let pool = chat_pool(app, db_url).await?;
    let current = read_access_profile(&pool, &request.access_profile_id).await?;
    if current.revision != request.expected_revision {
        return Err(stale_access_profile_error());
    }
    let is_expansion = profile_revision_is_expansion(
        current.latest_revision.default_channel_capabilities,
        &current.latest_revision.default_history_boundary,
        current.latest_revision.maximum_folder_capability,
        request.revision.default_channel_capabilities,
        &request.revision.default_history_boundary,
        request.revision.maximum_folder_capability,
    );
    let is_reduction = profile_revision_is_reduction(&current.latest_revision, &request.revision);
    let membership_rows = sqlx::query(
        "SELECT DISTINCT membership.teammate_id, channel.id AS channel_id
         FROM chat_teammate_channel_memberships membership
         JOIN chat_channels channel ON channel.conversation_id = membership.conversation_id
         WHERE membership.access_profile_id = ? AND membership.removed_at IS NULL
         ORDER BY membership.teammate_id, channel.id",
    )
    .bind(request.access_profile_id.as_str())
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let affected_teammate_ids = membership_rows
        .iter()
        .map(|row| {
            ChatParticipantId::new(
                row.try_get::<String, _>("teammate_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)
        })
        .collect::<ChatResult<BTreeSet<_>>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let affected_channel_ids = membership_rows
        .iter()
        .map(|row| {
            ChatChannelId::new(
                row.try_get::<String, _>("channel_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)
        })
        .collect::<ChatResult<BTreeSet<_>>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let active_authorization_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_access_profile_revisions revision
           ON revision.id = authorization.access_profile_revision_id
         WHERE revision.access_profile_id = ?
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL",
    )
    .bind(request.access_profile_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    let mut issues = if current.builtin_key.is_some() {
        vec![issue(
            ChatAccessIssueCode::ProfileCeilingExceeded,
            "accessProfileId",
            "Built-in access profiles are immutable",
        )]
    } else {
        Vec::new()
    };
    if current.builtin_key.is_none() && is_expansion {
        let mut transaction = pool.begin().await.map_err(persistence_error)?;
        let preview_now = now_timestamp()?;
        publish_access_profile_revision(
            &mut transaction,
            &request.access_profile_id,
            &request.revision,
            &preview_now,
        )
        .await?;
        if is_reduction {
            apply_profile_reduction(
                &mut transaction,
                &request.access_profile_id,
                &request.revision,
                &preview_now,
            )
            .await?;
        }
        issues.extend(
            retained_profile_reference_issues_in_transaction(
                &mut transaction,
                &request.access_profile_id,
            )
            .await?,
        );
        transaction.rollback().await.map_err(persistence_error)?;
    }
    Ok(ChatAccessProfileImpactPreview {
        access_profile_id: request.access_profile_id,
        current_revision: current.latest_revision,
        is_expansion,
        is_reduction,
        affected_teammate_ids,
        affected_channel_ids,
        active_authorization_count: u64_value(active_authorization_count)?,
        issues,
    })
}

#[tauri::command]
pub async fn chat_publish_access_profile_revision(
    app: tauri::AppHandle,
    db_url: String,
    request: PublishChatAccessProfileRevisionCommand,
) -> ChatResult<ChatAccessProfileRead> {
    validate_profile_revision_input(&request.revision)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let current = read_access_profile(&pool, &request.access_profile_id).await?;
    if current.builtin_key.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Built-in access profiles cannot be edited",
            true,
        ));
    }
    let is_expansion = profile_revision_is_expansion(
        current.latest_revision.default_channel_capabilities,
        &current.latest_revision.default_history_boundary,
        current.latest_revision.maximum_folder_capability,
        request.revision.default_channel_capabilities,
        &request.revision.default_history_boundary,
        request.revision.maximum_folder_capability,
    );
    let is_reduction = profile_revision_is_reduction(&current.latest_revision, &request.revision);
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let locked = sqlx::query(
        "UPDATE chat_access_profiles SET updated_at = updated_at
         WHERE id = ? AND revision = ? AND builtin_key IS NULL",
    )
    .bind(request.access_profile_id.as_str())
    .bind(i64_value(request.expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if locked.rows_affected() != 1 {
        return Err(stale_access_profile_error());
    }
    publish_access_profile_revision(
        &mut transaction,
        &request.access_profile_id,
        &request.revision,
        &now,
    )
    .await?;
    if is_reduction {
        apply_profile_reduction(
            &mut transaction,
            &request.access_profile_id,
            &request.revision,
            &now,
        )
        .await?;
    }
    if is_expansion {
        let retained_issues = retained_profile_reference_issues_in_transaction(
            &mut transaction,
            &request.access_profile_id,
        )
        .await?;
        if let Some(issue) = retained_issues.first() {
            return Err(ChatError::validation(
                &issue.field_path,
                issue.message.clone(),
            ));
        }
    }
    if is_reduction {
        revoke_profile_authorizations(
            &mut transaction,
            &request.access_profile_id,
            &now,
            "Access profile authority was reduced",
        )
        .await?;
    }
    // Existing authorization revisions keep their narrower immutable snapshots.
    transaction.commit().await.map_err(persistence_error)?;
    if is_reduction {
        crate::chat::revocation::drain_access_revocation_jobs(&app, &pool).await?;
    }
    read_access_profile(&pool, &request.access_profile_id).await
}

async fn apply_profile_reduction(
    transaction: &mut Transaction<'_, Sqlite>,
    access_profile_id: &ChatAccessProfileId,
    revision: &ChatAccessProfileRevisionInput,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    if matches!(
        revision.default_history_boundary,
        ChatHistoryBoundary::FromGrant { .. }
    ) {
        stamp_inherited_from_grant_boundaries(transaction, access_profile_id, now).await?;
    }
    sqlx::query(
        "UPDATE chat_teammate_access_state
         SET access_revision = access_revision + 1, updated_at = ?
         WHERE teammate_id IN (
           SELECT teammate_id FROM chat_teammate_channel_memberships
           WHERE access_profile_id = ? AND removed_at IS NULL
         )",
    )
    .bind(now.as_str())
    .bind(access_profile_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

#[tauri::command]
pub async fn chat_archive_access_profile(
    app: tauri::AppHandle,
    db_url: String,
    request: ArchiveChatAccessProfileCommand,
) -> ChatResult<ChatAccessProfileRead> {
    let pool = chat_pool(app, db_url).await?;
    let current = read_access_profile(&pool, &request.access_profile_id).await?;
    if current.builtin_key.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Built-in access profiles cannot be archived",
            true,
        ));
    }
    if request.archived {
        let membership_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_teammate_channel_memberships
             WHERE access_profile_id = ? AND removed_at IS NULL",
        )
        .bind(request.access_profile_id.as_str())
        .fetch_one(&pool)
        .await
        .map_err(persistence_error)?;
        if membership_count > 0 {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Reassign channel access before archiving this profile",
                true,
            ));
        }
    }
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE chat_access_profiles
         SET archived_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ? AND builtin_key IS NULL",
    )
    .bind(request.archived.then(|| now.as_str()))
    .bind(now.as_str())
    .bind(request.access_profile_id.as_str())
    .bind(i64_value(request.expected_revision)?)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(stale_access_profile_error());
    }
    read_access_profile(&pool, &request.access_profile_id).await
}

pub(super) async fn read_access_profile(
    pool: &SqlitePool,
    access_profile_id: &ChatAccessProfileId,
) -> ChatResult<ChatAccessProfileRead> {
    let row = sqlx::query(
        "SELECT profile.builtin_key, profile.display_name, revision.revision AS latest_revision,
                profile.revision AS profile_revision, profile.archived_at,
                revision.id AS access_profile_revision_id,
                revision.default_read_history, revision.default_participate,
                revision.default_history_boundary, revision.maximum_folder_capability,
                revision.created_at
         FROM chat_access_profiles profile
         JOIN chat_access_profile_effective_revisions revision
           ON revision.access_profile_id = profile.id
         WHERE profile.id = ?",
    )
    .bind(access_profile_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Access profile was not found",
            true,
        )
    })?;
    let latest_revision = u64_value(row.try_get("latest_revision").map_err(persistence_error)?)?;
    Ok(ChatAccessProfileRead {
        id: access_profile_id.clone(),
        builtin_key: parse_access_profile_builtin_key(
            row.try_get::<Option<String>, _>("builtin_key")
                .map_err(persistence_error)?
                .as_deref(),
        )?,
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        latest_revision: ChatAccessProfileRevision {
            id: ChatAccessProfileRevisionId::new(
                row.try_get::<String, _>("access_profile_revision_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            access_profile_id: access_profile_id.clone(),
            revision: latest_revision,
            default_channel_capabilities: ChatChannelCapabilities {
                read_history: row
                    .try_get::<i64, _>("default_read_history")
                    .map_err(persistence_error)?
                    != 0,
                participate: row
                    .try_get::<i64, _>("default_participate")
                    .map_err(persistence_error)?
                    != 0,
            },
            default_history_boundary: parse_history_boundary(
                &row.try_get::<String, _>("default_history_boundary")
                    .map_err(persistence_error)?,
                None,
            )?,
            maximum_folder_capability: parse_folder_capability(
                &row.try_get::<String, _>("maximum_folder_capability")
                    .map_err(persistence_error)?,
            )?,
            created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        },
        revision: u64_value(row.try_get("profile_revision").map_err(persistence_error)?)?,
        archived_at: optional_timestamp(row.try_get("archived_at").map_err(persistence_error)?)?,
    })
}

fn profile_revision_is_reduction(
    current: &ChatAccessProfileRevision,
    proposed: &ChatAccessProfileRevisionInput,
) -> bool {
    (current.default_channel_capabilities.read_history
        && !proposed.default_channel_capabilities.read_history)
        || (current.default_channel_capabilities.participate
            && !proposed.default_channel_capabilities.participate)
        || history_boundary_is_expansion(
            &proposed.default_history_boundary,
            &current.default_history_boundary,
        )
        || proposed.maximum_folder_capability.rank() < current.maximum_folder_capability.rank()
}

/// Appends a revision to an existing access profile, points the profile's current revision at
/// it, and advances the profile's edit counter.
async fn publish_access_profile_revision(
    transaction: &mut Transaction<'_, Sqlite>,
    access_profile_id: &ChatAccessProfileId,
    input: &ChatAccessProfileRevisionInput,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let revision_id = new_id("access-profile-revision")?;
    insert_access_profile_revision(transaction, access_profile_id, &revision_id, input, now)
        .await?;
    sqlx::query(
        "UPDATE chat_access_profiles
         SET current_revision_id = ?, current_maximum_folder_capability = ?,
             revision = revision + 1, updated_at = ?
         WHERE id = ?",
    )
    .bind(revision_id.as_str())
    .bind(wire_folder_capability(input.maximum_folder_capability))
    .bind(now.as_str())
    .bind(access_profile_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Inserts one immutable access profile revision row.
async fn insert_access_profile_revision(
    transaction: &mut Transaction<'_, Sqlite>,
    access_profile_id: &ChatAccessProfileId,
    revision_id: &str,
    input: &ChatAccessProfileRevisionInput,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_access_profile_revisions
            (id, access_profile_id, default_read_history,
             default_participate, default_history_boundary,
             maximum_folder_capability, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(revision_id)
    .bind(access_profile_id.as_str())
    .bind(input.default_channel_capabilities.read_history)
    .bind(input.default_channel_capabilities.participate)
    .bind(wire_history_boundary(&input.default_history_boundary))
    .bind(wire_folder_capability(input.maximum_folder_capability))
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn revoke_profile_authorizations(
    transaction: &mut Transaction<'_, Sqlite>,
    access_profile_id: &ChatAccessProfileId,
    now: &UtcTimestamp,
    reason: &str,
) -> ChatResult<()> {
    let teammate_ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT teammate_id FROM chat_teammate_channel_memberships
         WHERE access_profile_id = ? AND removed_at IS NULL",
    )
    .bind(access_profile_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for teammate_id in teammate_ids {
        let teammate_id = ChatParticipantId::new(teammate_id).map_err(identifier_error)?;
        revoke_teammate_authorizations(transaction, &teammate_id, now, reason).await?;
    }
    Ok(())
}

/// Stamps a `from_grant` boundary at the current end of each channel on every active membership
/// that inherits the profile boundary and has no stamped lower order key yet.
async fn stamp_inherited_from_grant_boundaries(
    transaction: &mut Transaction<'_, Sqlite>,
    access_profile_id: &ChatAccessProfileId,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let memberships = sqlx::query_as::<_, (String, String)>(
        "SELECT conversation_id, teammate_id FROM chat_teammate_channel_memberships
         WHERE access_profile_id = ? AND removed_at IS NULL
           AND history_boundary_inherits_profile = 1
           AND (history_boundary != 'from_grant' OR history_from_order_key IS NULL)",
    )
    .bind(access_profile_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for (conversation_id, teammate_id) in memberships {
        let lower_order_key = from_grant_lower_order_key(transaction, &conversation_id).await?;
        sqlx::query(
            "UPDATE chat_teammate_channel_memberships
             SET history_from_order_key = ?, history_boundary = 'from_grant', updated_at = ?
             WHERE conversation_id = ? AND teammate_id = ?",
        )
        .bind(lower_order_key)
        .bind(now.as_str())
        .bind(&conversation_id)
        .bind(&teammate_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

fn validate_profile_revision_input(input: &ChatAccessProfileRevisionInput) -> ChatResult<()> {
    if matches!(
        input.default_history_boundary,
        ChatHistoryBoundary::FromGrant {
            lower_order_key: Some(_)
        }
    ) {
        return Err(ChatError::validation(
            "revision.defaultHistoryBoundary.lowerOrderKey",
            "History lower order keys are captured per channel",
        ));
    }
    Ok(())
}

fn validate_access_profile_name(value: &str) -> ChatResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 160 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "displayName",
            "Access profile name must contain 1 to 160 characters",
        ));
    }
    Ok(value.to_string())
}

/// Refuses a custom profile name that an active custom profile already uses, ignoring case and
/// surrounding spaces.
///
/// The check covers the profiles this device holds. Devices that create profiles concurrently
/// can still end up with equal names, which readers tell apart by id.
async fn require_unused_access_profile_name(
    transaction: &mut Transaction<'_, Sqlite>,
    display_name: &str,
) -> ChatResult<()> {
    let in_use: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_access_profiles
           WHERE builtin_key IS NULL AND archived_at IS NULL
             AND lower(trim(display_name)) = lower(trim(?))
         )",
    )
    .bind(display_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if in_use {
        return Err(ChatError::validation(
            "displayName",
            "Access profile name already in use",
        ));
    }
    Ok(())
}

fn stale_access_profile_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::StaleRevision,
        "The access profile changed before the update",
        true,
    )
}
