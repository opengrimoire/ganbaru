//! Device-local Chat state derived from replicated rows that sync materialized.
//!
//! Commands keep access state, audience revisions, and message search current through triggers
//! that stand down while sync applies rows. After the sync engine writes rows of one table,
//! [`after_materialize`] does the same work for the changed keys inside the engine transaction,
//! with plain SQL and no dependency on the engine, so any caller holding the connection can run
//! it. It never writes replicated rows.

use crate::attachment_files;
use crate::coordination::revocation::revoke_teammate_authorizations;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::SqliteConnection;
use std::collections::BTreeSet;
use std::time::SystemTime;

/// Replicated table of AI teammates, keyed by participant id.
pub const TEAMMATES_TABLE: &str = "chat_teammates";
/// Replicated table of teammate policy revisions, keyed by id.
pub const POLICY_REVISIONS_TABLE: &str = "chat_teammate_policy_revisions";
/// Replicated table of conversations, keyed by id.
pub const CONVERSATIONS_TABLE: &str = "chat_conversations";
/// Replicated table of access profiles, keyed by id.
pub const ACCESS_PROFILES_TABLE: &str = "chat_access_profiles";
/// Replicated table of access profile revisions, keyed by id.
pub const ACCESS_PROFILE_REVISIONS_TABLE: &str = "chat_access_profile_revisions";
/// Replicated table of conversation memberships, keyed by sync key.
pub const MEMBERSHIPS_TABLE: &str = "chat_conversation_memberships";
/// Replicated table of AI channel memberships, keyed by sync key.
pub const CHANNEL_MEMBERSHIPS_TABLE: &str = "chat_teammate_channel_memberships";
/// Replicated table of working folder grants, keyed by sync key.
pub const FOLDER_GRANTS_TABLE: &str = "chat_teammate_working_folder_grants";
/// Replicated table of messages, keyed by item id.
pub const MESSAGES_TABLE: &str = "chat_communication_messages";
/// Replicated table of message revisions, keyed by id.
pub const MESSAGE_REVISIONS_TABLE: &str = "chat_communication_message_revisions";

/// Reason recorded on authorizations revoked because an applied profile revision narrowed it.
pub const PROFILE_REDUCTION_REASON: &str = "Access profile authority was reduced";
/// Reason recorded on authorizations revoked because an applied membership or folder grant no
/// longer covers them, matching the command that saves a reduction.
pub const ACCESS_REDUCTION_REASON: &str = "Teammate access was reduced";
/// Reason recorded on authorizations revoked because a synced teammate or profile pointer
/// disagrees with the revision it names.
pub const MISMATCHED_POINTER_REASON: &str =
    "Synced teammate policy or access profile disagrees with its revision";

/// Device-local state [`after_materialize`] changed, so the caller can wake the services and
/// views that read it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChatSyncEffects {
    /// Teammates whose access state row was created.
    pub access_state_created: BTreeSet<String>,
    /// Teammates whose access revision was bumped.
    pub access_revised: BTreeSet<String>,
    /// Conversations whose audience state was created or whose audience revision was bumped.
    pub audience_revised: BTreeSet<String>,
    /// Teammates with authorizations revoked and access revocation jobs queued for them.
    pub revoked_teammates: BTreeSet<String>,
    /// Messages whose search row was rebuilt or removed.
    pub search_rebuilt: BTreeSet<String>,
    /// Teammates and access profiles whose current pointer copies restrictiveness that
    /// disagrees with the revision it names, so they have no effective revision.
    pub mismatched_pointers: BTreeSet<String>,
    /// Conversations the apply deleted. The delete cascades away the authorizations that
    /// target them, with their runs, so no revocation job can name those runs; the caller stops
    /// running work whose authorization no longer exists.
    pub removed_conversations: BTreeSet<String>,
}

impl ChatSyncEffects {
    /// Returns whether no device-local state changed and nothing was found to report.
    pub fn is_empty(&self) -> bool {
        self.access_state_created.is_empty()
            && self.access_revised.is_empty()
            && self.audience_revised.is_empty()
            && self.revoked_teammates.is_empty()
            && self.search_rebuilt.is_empty()
            && self.mismatched_pointers.is_empty()
            && self.removed_conversations.is_empty()
    }
}

/// Recomputes the device-local Chat state derived from the rows of `table` whose keys are in
/// `changed_keys`, after sync inserted, updated, or deleted them.
///
/// It mirrors the gated command triggers: a teammate gets its access state and a conversation
/// its audience state, a profile whose effective revision arrived or changed bumps the audience
/// of the conversations whose teammates use that profile, and a message or revision rebuilds the
/// message's search row (a deleted message loses it). A revision also creates the attachment and
/// resource rows of the attachments it names in a working folder this device has. An effective profile revision that narrows
/// authority also bumps the access revision of those teammates and revokes the active
/// authorizations granted under a wider revision, as the command that saves a reduction does.
/// A conversation membership, AI channel membership, or folder grant bumps the audience (for
/// memberships) and the AI teammate's access revision, and revokes the teammate's authorizations
/// once one of them is no longer covered by the rows sync kept. A deleted conversation is
/// reported, and deleted rows sweep every teammate with an active authorization. Timestamps
/// come from the changed rows, except revocations, which happen now on this device.
/// Running it again for the same keys leaves the same rows, apart from the counters a profile
/// revision bumps again. Tables without derived state return no effects.
///
/// When a teammate or profile pointer copies a safety mode or maximum folder capability that
/// differs from the revision it names, once both rows are present, it revokes the affected
/// teammates' authorizations and reports the pointer instead of failing the apply. The effective
/// revision views already give that pointer no effective revision.
pub async fn after_materialize(
    connection: &mut SqliteConnection,
    table: &str,
    changed_keys: &[String],
) -> ChatResult<ChatSyncEffects> {
    let mut effects = ChatSyncEffects::default();
    let keys: BTreeSet<&str> = changed_keys.iter().map(String::as_str).collect();
    match table {
        TEAMMATES_TABLE => {
            for key in keys {
                create_access_state(connection, key, &mut effects).await?;
                contain_mismatched_pointer(
                    connection,
                    POLICY_COPY_MISMATCH_BY_TEAMMATE,
                    PointerOwner::Teammate,
                    key,
                    &mut effects,
                )
                .await?;
            }
        }
        POLICY_REVISIONS_TABLE => {
            for key in keys {
                contain_mismatched_pointer(
                    connection,
                    POLICY_COPY_MISMATCH_BY_REVISION,
                    PointerOwner::Teammate,
                    key,
                    &mut effects,
                )
                .await?;
            }
        }
        CONVERSATIONS_TABLE => {
            for key in keys {
                if !create_audience_state(connection, key, &mut effects).await? {
                    effects.removed_conversations.insert(key.to_string());
                }
            }
            if !effects.removed_conversations.is_empty() {
                sweep_uncovered_authorizations(connection, &mut effects).await?;
            }
        }
        ACCESS_PROFILES_TABLE => {
            for key in keys {
                contain_mismatched_pointer(
                    connection,
                    PROFILE_COPY_MISMATCH_BY_PROFILE,
                    PointerOwner::Profile,
                    key,
                    &mut effects,
                )
                .await?;
                apply_effective_profile_revision(connection, key, &mut effects).await?;
            }
        }
        ACCESS_PROFILE_REVISIONS_TABLE => {
            let mut profile_ids = BTreeSet::new();
            for key in keys {
                contain_mismatched_pointer(
                    connection,
                    PROFILE_COPY_MISMATCH_BY_REVISION,
                    PointerOwner::Profile,
                    key,
                    &mut effects,
                )
                .await?;
                let profile_id: Option<String> = sqlx::query_scalar(
                    "SELECT revision.access_profile_id
                     FROM chat_access_profile_revisions revision
                     JOIN chat_access_profile_effective_revisions effective
                       ON effective.id = revision.id
                     WHERE revision.id = ?",
                )
                .bind(key)
                .fetch_optional(&mut *connection)
                .await
                .map_err(persistence_error)?;
                profile_ids.extend(profile_id);
            }
            for profile_id in profile_ids {
                apply_effective_profile_revision(connection, &profile_id, &mut effects).await?;
            }
        }
        MEMBERSHIPS_TABLE => {
            let rows = changed_access_rows(connection, MEMBERSHIP_ROW_BY_KEY, &keys).await?;
            apply_access_rows(connection, &rows.present, true, &mut effects).await?;
            if rows.deleted {
                sweep_uncovered_authorizations(connection, &mut effects).await?;
            }
        }
        CHANNEL_MEMBERSHIPS_TABLE => {
            let rows =
                changed_access_rows(connection, CHANNEL_MEMBERSHIP_ROW_BY_KEY, &keys).await?;
            apply_access_rows(connection, &rows.present, true, &mut effects).await?;
            if rows.deleted {
                sweep_uncovered_authorizations(connection, &mut effects).await?;
            }
        }
        FOLDER_GRANTS_TABLE => {
            let rows = changed_access_rows(connection, FOLDER_GRANT_ROW_BY_KEY, &keys).await?;
            apply_access_rows(connection, &rows.present, false, &mut effects).await?;
            if rows.deleted {
                sweep_uncovered_authorizations(connection, &mut effects).await?;
            }
        }
        MESSAGES_TABLE => {
            for key in keys {
                rebuild_message_search(connection, key, &mut effects).await?;
            }
        }
        MESSAGE_REVISIONS_TABLE => {
            let mut message_ids = BTreeSet::new();
            for key in keys {
                record_applied_attachments(connection, key).await?;
                let message_id: Option<String> = sqlx::query_scalar(
                    "SELECT message_item_id FROM chat_communication_message_revisions
                     WHERE id = ?",
                )
                .bind(key)
                .fetch_optional(&mut *connection)
                .await
                .map_err(persistence_error)?;
                message_ids.extend(message_id);
            }
            for message_id in message_ids {
                rebuild_message_search(connection, &message_id, &mut effects).await?;
            }
        }
        _ => {}
    }
    Ok(effects)
}

/// One attachment a revision names by value, as its owned reference row holds it.
#[derive(sqlx::FromRow)]
struct AppliedAttachment {
    attachment_id: String,
    working_folder_id: String,
    kind: String,
    original_display_name: String,
    mime_type: String,
    byte_size: i64,
    sha256: String,
    created_at: String,
}

/// Creates the device's attachment row and its resource for every attachment the revision
/// names whose working folder exists here, so the revision's attachments read like local ones.
/// The file arrives through the asset transfer under its digest name; an attachment row the
/// device already has is kept unchanged.
async fn record_applied_attachments(
    connection: &mut SqliteConnection,
    revision_id: &str,
) -> ChatResult<()> {
    let attachments: Vec<AppliedAttachment> = sqlx::query_as(
        "SELECT reference.attachment_id, reference.working_folder_id, reference.kind,
                reference.original_display_name, reference.mime_type, reference.byte_size,
                reference.sha256, revision.created_at
         FROM chat_communication_attachment_references reference
         JOIN chat_communication_message_revisions revision
           ON revision.id = reference.message_revision_id
         JOIN project_working_folders folder ON folder.id = reference.working_folder_id
         WHERE reference.message_revision_id = ?
         ORDER BY reference.ordinal",
    )
    .bind(revision_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    for attachment in attachments {
        let relative_path =
            attachment_files::relative_path(&attachment.sha256, &attachment.mime_type);
        let created = sqlx::query(
            "INSERT OR IGNORE INTO chat_attachments
                 (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                  sha256, managed_relative_path, signature_kind, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&attachment.attachment_id)
        .bind(&attachment.working_folder_id)
        .bind(&attachment.kind)
        .bind(&attachment.original_display_name)
        .bind(&attachment.mime_type)
        .bind(attachment.byte_size)
        .bind(&attachment.sha256)
        .bind(&relative_path)
        .bind(attachment_files::signature_kind(&attachment.mime_type))
        .bind(&attachment.created_at)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?
        .rows_affected();
        if created == 0 {
            continue;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO chat_resources
                 (id, working_folder_id, attachment_id, resource_kind, display_name, mime_type,
                  byte_size, sha256, managed_relative_path, resource_uri, integrity_state,
                  created_at)
             VALUES (?1, ?2, ?1, ?3, ?4, ?5, ?6, ?7, ?8, 'ganbaru://chat/resource/' || ?1,
                     'verified', ?9)",
        )
        .bind(&attachment.attachment_id)
        .bind(&attachment.working_folder_id)
        .bind(&attachment.kind)
        .bind(&attachment.original_display_name)
        .bind(&attachment.mime_type)
        .bind(attachment.byte_size)
        .bind(&attachment.sha256)
        .bind(&relative_path)
        .bind(&attachment.created_at)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

async fn create_access_state(
    connection: &mut SqliteConnection,
    teammate_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let created = sqlx::query(
        "INSERT OR IGNORE INTO chat_teammate_access_state (teammate_id, updated_at)
         SELECT participant_id, updated_at FROM chat_teammates WHERE participant_id = ?",
    )
    .bind(teammate_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if created > 0 {
        effects.access_state_created.insert(teammate_id.to_string());
    }
    Ok(())
}

/// Creates the audience state of the conversation and returns whether the conversation is
/// still present.
async fn create_audience_state(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<bool> {
    let created = sqlx::query(
        "INSERT OR IGNORE INTO chat_conversation_audience_state (conversation_id, updated_at)
         SELECT id, updated_at FROM chat_conversations WHERE id = ?",
    )
    .bind(conversation_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if created > 0 {
        effects.audience_revised.insert(conversation_id.to_string());
        return Ok(true);
    }
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chat_conversations WHERE id = ?)")
        .bind(conversation_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// Conversation, participant, and change time of the conversation membership with sync key `?`.
const MEMBERSHIP_ROW_BY_KEY: &str = "SELECT conversation_id, participant_id, updated_at
     FROM chat_conversation_memberships WHERE sync_key = ?";

/// Conversation, teammate, and change time of the AI channel membership with sync key `?`.
const CHANNEL_MEMBERSHIP_ROW_BY_KEY: &str = "SELECT conversation_id, teammate_id, updated_at
     FROM chat_teammate_channel_memberships WHERE sync_key = ?";

/// Conversation, teammate, and change time of the working folder grant with sync key `?`.
const FOLDER_GRANT_ROW_BY_KEY: &str =
    "SELECT conversation_id, teammate_id, coalesce(revoked_at, created_at)
     FROM chat_teammate_working_folder_grants WHERE sync_key = ?";

/// Changed membership or grant rows of one apply.
struct ChangedAccessRows {
    /// Conversation, participant, and change time of each changed row that is still present.
    present: BTreeSet<(String, String, String)>,
    /// Whether the apply deleted any changed row, directly or by a cascade from a deleted
    /// conversation, teammate, project, or working folder.
    deleted: bool,
}

/// Reads the changed rows of one membership or grant table by sync key.
async fn changed_access_rows(
    connection: &mut SqliteConnection,
    row_by_key: &'static str,
    keys: &BTreeSet<&str>,
) -> ChatResult<ChangedAccessRows> {
    let mut rows = ChangedAccessRows {
        present: BTreeSet::new(),
        deleted: false,
    };
    for key in keys {
        let row: Option<(String, String, String)> = sqlx::query_as(row_by_key)
            .bind(key)
            .fetch_optional(&mut *connection)
            .await
            .map_err(persistence_error)?;
        match row {
            Some(row) => {
                rows.present.insert(row);
            }
            None => rows.deleted = true,
        }
    }
    Ok(rows)
}

/// Revokes the active authorizations of every teammate that holds one its current rows no
/// longer cover, after an apply deleted memberships, grants, or conversations.
///
/// A deleted row no longer names its teammate, so every teammate with an active authorization
/// is checked. Authorizations that a cascade already removed need no revocation, and covered
/// ones are kept, so a repeated apply revokes nothing.
async fn sweep_uncovered_authorizations(
    connection: &mut SqliteConnection,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let teammates: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT assignment.teammate_id
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.teammate_id IS NOT NULL
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL
         ORDER BY assignment.teammate_id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let now = now();
    for teammate_id in teammates {
        if holds_uncovered_authorization(connection, &teammate_id).await? {
            revoke(
                connection,
                &teammate_id,
                &now,
                ACCESS_REDUCTION_REASON,
                effects,
            )
            .await?;
        }
    }
    Ok(())
}

/// The current time on this device, as stored timestamps write it.
fn now() -> String {
    let now: DateTime<Utc> = SystemTime::now().into();
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Revokes the teammate's active authorizations, bumps its access revision, and records both.
async fn revoke(
    connection: &mut SqliteConnection,
    teammate_id: &str,
    now: &str,
    reason: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let revoked = revoke_teammate_authorizations(connection, teammate_id, now, reason).await?;
    if revoked > 0 {
        effects.revoked_teammates.insert(teammate_id.to_string());
    }
    let revised: Option<String> = sqlx::query_scalar(
        "UPDATE chat_teammate_access_state
         SET access_revision = access_revision + 1, updated_at = ?
         WHERE teammate_id = ?
         RETURNING teammate_id",
    )
    .bind(now)
    .bind(teammate_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    effects.access_revised.extend(revised);
    Ok(())
}

/// Mirrors the command side of saving teammate access for applied membership or grant rows.
///
/// A membership change bumps the audience of its conversation when `revises_audience` holds, as
/// the gated membership triggers do. A change for an AI teammate bumps the teammate's access
/// revision, and revokes the teammate's active authorizations when one of them is no longer
/// covered by the current rows, so a concurrent removal or narrowing that sync kept also stops
/// the runs it granted. Revoking only uncovered authorizations keeps a widening or a repeated
/// apply from revoking anything.
async fn apply_access_rows(
    connection: &mut SqliteConnection,
    rows: &BTreeSet<(String, String, String)>,
    revises_audience: bool,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let mut teammates = BTreeSet::new();
    for (conversation_id, participant_id, changed_at) in rows {
        if revises_audience {
            let revised = sqlx::query(
                "UPDATE chat_conversation_audience_state
                 SET revision = revision + 1, updated_at = ?
                 WHERE conversation_id = ?",
            )
            .bind(changed_at)
            .bind(conversation_id)
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?
            .rows_affected();
            if revised > 0 {
                effects.audience_revised.insert(conversation_id.clone());
            }
        }
        let revised: Option<String> = sqlx::query_scalar(
            "UPDATE chat_teammate_access_state
             SET access_revision = access_revision + 1, updated_at = ?
             WHERE teammate_id = ?
             RETURNING teammate_id",
        )
        .bind(changed_at)
        .bind(participant_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(persistence_error)?;
        teammates.extend(revised);
    }
    let now: DateTime<Utc> = SystemTime::now().into();
    let now = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    for teammate_id in teammates {
        if holds_uncovered_authorization(connection, &teammate_id).await? {
            let revoked = revoke_teammate_authorizations(
                connection,
                &teammate_id,
                &now,
                ACCESS_REDUCTION_REASON,
            )
            .await?;
            if revoked > 0 {
                effects.revoked_teammates.insert(teammate_id.clone());
            }
        }
        effects.access_revised.insert(teammate_id);
    }
    Ok(())
}

/// SQL ranks of runtime approval policies, from least to most restrictive.
const APPROVAL_RANKS: &str = "approval_rank(policy, rank) AS (
    VALUES ('provider_custom', 1), ('unattended', 2), ('auto_approve', 3), ('ask', 4)
)";

/// SQL condition that holds when the channel membership `access`, with its profile's effective
/// revision joined as `profile_revision`, lets the teammate post, as assignment context requires.
const PARTICIPATES: &str = "CASE
    WHEN access.participate_inherits_profile = 1 THEN profile_revision.default_participate
    ELSE access.participate AND profile_revision.default_participate
END = 1";

/// SQL condition that holds when the channel membership `access`, with its profile's effective
/// revision joined as `profile_revision`, lets the teammate read history from the lower bound
/// of the disclosed channel `source`, as the context checks of channel disclosure do.
const READS_HISTORY_FROM: &str = "(
    CASE
        WHEN access.read_history_inherits_profile = 1 THEN profile_revision.default_read_history
        ELSE access.read_history AND profile_revision.default_read_history
    END = 1
    AND (
        CASE
            WHEN access.history_boundary_inherits_profile = 1
                THEN profile_revision.default_history_boundary
            WHEN access.history_boundary = 'from_grant'
                OR profile_revision.default_history_boundary = 'from_grant'
                THEN 'from_grant'
            ELSE 'entire'
        END = 'entire'
        OR (
            access.history_from_order_key IS NOT NULL
            AND access.history_from_order_key <= source.lower_order_key
        )
    )
)";

/// SQL expression for the runtime approval override that governs `authorization`, resolved
/// from the destination channel membership `access` in the runtime's precedence: a scratch
/// target takes the scratch override, a working folder target takes the execution target's
/// grant override, and either falls back to the channel override. Null when none is set, so
/// the teammate's own approval behavior applies.
const RESOLVED_OVERRIDE: &str = "CASE
    WHEN EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = authorization.execution_environment_id
          AND environment.kind = 'scratch'
    ) THEN coalesce(access.scratch_runtime_approval_policy, access.runtime_approval_policy)
    ELSE coalesce(
        (
            SELECT folder_grant.runtime_approval_policy
            FROM chat_assignment_authorized_folder_sources source
            JOIN chat_teammate_working_folder_grants folder_grant
              ON folder_grant.conversation_id = access.conversation_id
             AND folder_grant.teammate_id = access.teammate_id
             AND folder_grant.working_folder_id = source.working_folder_id
            WHERE source.authorization_revision_id = authorization.id
              AND source.is_execution_target = 1
        ),
        access.runtime_approval_policy
    )
END";

/// Returns whether the teammate holds an active authorization that its current memberships and
/// folder grants no longer cover.
///
/// An authorization is uncovered when the teammate's conversation membership or AI channel
/// membership of its destination has ended, when a channel it disclosed can no longer be read
/// from the disclosed lower bound, when a folder it was granted has no active grant of at least the
/// authorized capability, or when the runtime approval override that governs its target (see
/// [`RESOLVED_OVERRIDE`]) is now stricter than the authorized policy.
async fn holds_uncovered_authorization(
    connection: &mut SqliteConnection,
    teammate_id: &str,
) -> ChatResult<bool> {
    let sql = format!(
        "WITH {CAPABILITY_RANKS}, {APPROVAL_RANKS}
         SELECT EXISTS (
             SELECT 1
             FROM chat_assignment_authorization_revisions authorization
             JOIN chat_work_assignments assignment
               ON assignment.id = authorization.assignment_id
             JOIN approval_rank authorized_approval
               ON authorized_approval.policy = authorization.resolved_runtime_approval_policy
             WHERE assignment.teammate_id = ?1
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
               AND (
                   NOT EXISTS (
                       SELECT 1 FROM chat_conversation_memberships membership
                       JOIN chat_teammate_channel_memberships access
                         ON access.conversation_id = membership.conversation_id
                        AND access.teammate_id = membership.participant_id
                        AND access.removed_at IS NULL
                       JOIN chat_access_profile_effective_revisions profile_revision
                         ON profile_revision.access_profile_id = access.access_profile_id
                       WHERE membership.conversation_id = authorization.destination_conversation_id
                         AND membership.participant_id = ?1
                         AND membership.removed_at IS NULL
                         AND {PARTICIPATES}
                   )
                   OR EXISTS (
                       SELECT 1 FROM chat_teammate_channel_memberships access
                       JOIN approval_rank override ON override.policy = {RESOLVED_OVERRIDE}
                       WHERE access.conversation_id = authorization.destination_conversation_id
                         AND access.teammate_id = ?1
                         AND override.rank > authorized_approval.rank
                   )
                   OR EXISTS (
                       SELECT 1 FROM chat_assignment_authorized_channel_sources source
                       WHERE source.authorization_revision_id = authorization.id
                         AND NOT EXISTS (
                             SELECT 1
                             FROM chat_conversation_memberships membership
                             JOIN chat_teammate_channel_memberships access
                               ON access.conversation_id = membership.conversation_id
                              AND access.teammate_id = membership.participant_id
                              AND access.removed_at IS NULL
                             JOIN chat_access_profile_effective_revisions profile_revision
                               ON profile_revision.access_profile_id = access.access_profile_id
                             WHERE membership.conversation_id = source.conversation_id
                               AND membership.participant_id = ?1
                               AND membership.removed_at IS NULL
                               AND {READS_HISTORY_FROM}
                         )
                   )
                   OR EXISTS (
                       SELECT 1 FROM chat_assignment_authorized_folder_sources source
                       JOIN capability_rank authorized_rank
                         ON authorized_rank.capability = source.capability
                       WHERE source.authorization_revision_id = authorization.id
                         AND NOT EXISTS (
                             SELECT 1
                             FROM chat_teammate_working_folder_grants folder_grant
                             JOIN chat_teammate_channel_memberships access
                               ON access.conversation_id = folder_grant.conversation_id
                              AND access.teammate_id = folder_grant.teammate_id
                             JOIN chat_access_profile_effective_revisions profile_revision
                               ON profile_revision.access_profile_id = access.access_profile_id
                             JOIN capability_rank profile_rank
                               ON profile_rank.capability = profile_revision.maximum_folder_capability
                             JOIN capability_rank grant_rank
                               ON grant_rank.capability = folder_grant.capability
                             WHERE folder_grant.conversation_id
                                   = authorization.destination_conversation_id
                               AND folder_grant.teammate_id = ?1
                               AND folder_grant.working_folder_id = source.working_folder_id
                               AND folder_grant.revoked_at IS NULL
                               AND CASE
                                       WHEN folder_grant.capability_inherits_profile = 1
                                           THEN profile_rank.rank
                                       ELSE min(grant_rank.rank, profile_rank.rank)
                                   END >= authorized_rank.rank
                         )
                   )
               )
         )"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(teammate_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// Bumps the audiences of the conversations whose teammates use the profile and, when the
/// profile's effective revision narrows authority, the access of those teammates.
///
/// The effective revision is the one the profile's current pointer names, or the newest arrived
/// revision while that one is missing. It narrows authority when it is narrower than the revision
/// before it or than any later revision, since a pointer that names an older revision chose it as
/// the more restrictive one. Teammates are revoked only while one of their active authorizations
/// was granted under a wider revision of the profile, so applying the same effective revision
/// again never revokes authorizations granted under it. The audience and access revisions are
/// change counters, so an extra bump only makes readers refresh.
async fn apply_effective_profile_revision(
    connection: &mut SqliteConnection,
    profile_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let effective: Option<(String, String)> = sqlx::query_as(
        "SELECT id, created_at FROM chat_access_profile_effective_revisions
         WHERE access_profile_id = ?",
    )
    .bind(profile_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some((revision_id, created_at)) = effective else {
        return Ok(());
    };
    let audiences: Vec<String> = sqlx::query_scalar(
        "UPDATE chat_conversation_audience_state
         SET revision = revision + 1, updated_at = ?
         WHERE conversation_id IN (
             SELECT membership.conversation_id
             FROM chat_teammate_channel_memberships membership
             WHERE membership.access_profile_id = ? AND membership.removed_at IS NULL
         )
         RETURNING conversation_id",
    )
    .bind(&created_at)
    .bind(profile_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    effects.audience_revised.extend(audiences);
    if !effective_revision_narrows(connection, &revision_id).await? {
        return Ok(());
    }
    let teammates: Vec<String> = sqlx::query_scalar(
        "UPDATE chat_teammate_access_state
         SET access_revision = access_revision + 1, updated_at = ?
         WHERE teammate_id IN (
             SELECT teammate_id FROM chat_teammate_channel_memberships
             WHERE access_profile_id = ? AND removed_at IS NULL
         )
         RETURNING teammate_id",
    )
    .bind(&created_at)
    .bind(profile_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let now: DateTime<Utc> = SystemTime::now().into();
    let now = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    for teammate_id in teammates {
        if holds_wider_authorization(connection, &teammate_id, &revision_id).await? {
            let revoked = revoke_teammate_authorizations(
                connection,
                &teammate_id,
                &now,
                PROFILE_REDUCTION_REASON,
            )
            .await?;
            if revoked > 0 {
                effects.revoked_teammates.insert(teammate_id.clone());
            }
        }
        effects.access_revised.insert(teammate_id);
    }
    Ok(())
}

/// SQL ranks of folder capabilities, from no access to publishing.
const CAPABILITY_RANKS: &str = "capability_rank(capability, rank) AS (
    VALUES ('none', 0), ('read', 1), ('edit', 2), ('execute', 3), ('publish', 4)
)";

/// SQL condition that holds when revision `next` removes a default capability, narrows the
/// default history boundary, or lowers the folder capability ceiling of revision `previous`,
/// with their ranks joined as `next_rank` and `previous_rank`.
const NARROWER_THAN_PREVIOUS: &str = "(
    (previous.default_read_history = 1 AND next.default_read_history = 0)
    OR (previous.default_participate = 1 AND next.default_participate = 0)
    OR (previous.default_history_boundary = 'entire'
        AND next.default_history_boundary = 'from_grant')
    OR next_rank.rank < previous_rank.rank
)";

/// Returns whether the effective revision is narrower than the revision before it or than any
/// later revision of its profile, in `(created_at, id)` order.
async fn effective_revision_narrows(
    connection: &mut SqliteConnection,
    revision_id: &str,
) -> ChatResult<bool> {
    let sql = format!(
        "WITH {CAPABILITY_RANKS}
         SELECT EXISTS (
             SELECT 1
             FROM chat_access_profile_revisions next
             JOIN chat_access_profile_revisions previous
               ON previous.access_profile_id = next.access_profile_id
              AND previous.id <> next.id
              AND (
                  (previous.created_at, previous.id) > (next.created_at, next.id)
                  OR previous.id = (
                      SELECT earlier.id FROM chat_access_profile_revisions earlier
                      WHERE earlier.access_profile_id = next.access_profile_id
                        AND (earlier.created_at, earlier.id) < (next.created_at, next.id)
                      ORDER BY earlier.created_at DESC, earlier.id DESC
                      LIMIT 1
                  )
              )
             JOIN capability_rank next_rank ON next_rank.capability = next.maximum_folder_capability
             JOIN capability_rank previous_rank
               ON previous_rank.capability = previous.maximum_folder_capability
             WHERE next.id = ? AND {NARROWER_THAN_PREVIOUS}
         )"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(revision_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// Returns whether the teammate holds an active authorization granted under a revision of the
/// same profile that is wider than the effective revision.
async fn holds_wider_authorization(
    connection: &mut SqliteConnection,
    teammate_id: &str,
    revision_id: &str,
) -> ChatResult<bool> {
    let sql = format!(
        "WITH {CAPABILITY_RANKS}
         SELECT EXISTS (
             SELECT 1
             FROM chat_assignment_authorization_revisions authorization
             JOIN chat_work_assignments assignment
               ON assignment.id = authorization.assignment_id
             JOIN chat_access_profile_revisions next ON next.id = ?
             JOIN chat_access_profile_revisions previous
               ON previous.id = authorization.access_profile_revision_id
              AND previous.access_profile_id = next.access_profile_id
              AND previous.id <> next.id
             JOIN capability_rank next_rank ON next_rank.capability = next.maximum_folder_capability
             JOIN capability_rank previous_rank
               ON previous_rank.capability = previous.maximum_folder_capability
             WHERE assignment.teammate_id = ?
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
               AND {NARROWER_THAN_PREVIOUS}
         )"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(revision_id)
        .bind(teammate_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// Replaces the search row of a message with its current revision, or removes it when the
/// message is gone or has no current revision yet.
async fn rebuild_message_search(
    connection: &mut SqliteConnection,
    message_item_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let removed =
        sqlx::query("DELETE FROM chat_communication_search_fts WHERE message_item_id = ?")
            .bind(message_item_id)
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?
            .rows_affected();
    let inserted = sqlx::query(
        "INSERT INTO chat_communication_search_fts (
             message_item_id, conversation_id, reply_thread_id,
             author_display_name, normalized_markdown
         )
         SELECT message.item_id, item.conversation_id, item.reply_thread_id,
                coalesce(participant.display_name, message.author_label_snapshot),
                revision.normalized_markdown
         FROM chat_communication_messages message
         JOIN chat_conversation_items item ON item.id = message.item_id
         LEFT JOIN chat_participants participant
           ON participant.id = message.author_participant_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE message.item_id = ?",
    )
    .bind(message_item_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if removed > 0 || inserted > 0 {
        effects.search_rebuilt.insert(message_item_id.to_string());
    }
    Ok(())
}

/// Teammates with key `?` whose copied safety mode or owner disagrees with the policy revision
/// their pointer names.
const POLICY_COPY_MISMATCH_BY_TEAMMATE: &str = "SELECT teammate.participant_id
     FROM chat_teammates teammate
     JOIN chat_teammate_policy_revisions policy ON policy.id = teammate.current_policy_revision_id
     WHERE teammate.participant_id = ?
       AND (policy.teammate_id IS NOT teammate.participant_id
            OR policy.safety_mode IS NOT teammate.current_policy_safety_mode)";

/// Teammates whose pointer names policy revision `?` and whose copied safety mode or owner
/// disagrees with it.
const POLICY_COPY_MISMATCH_BY_REVISION: &str = "SELECT teammate.participant_id
     FROM chat_teammates teammate
     JOIN chat_teammate_policy_revisions policy ON policy.id = teammate.current_policy_revision_id
     WHERE policy.id = ?
       AND (policy.teammate_id IS NOT teammate.participant_id
            OR policy.safety_mode IS NOT teammate.current_policy_safety_mode)";

/// Profiles with key `?` whose copied maximum folder capability or owner disagrees with the
/// revision their pointer names.
const PROFILE_COPY_MISMATCH_BY_PROFILE: &str = "SELECT profile.id
     FROM chat_access_profiles profile
     JOIN chat_access_profile_revisions revision ON revision.id = profile.current_revision_id
     WHERE profile.id = ?
       AND (revision.access_profile_id IS NOT profile.id
            OR revision.maximum_folder_capability
               IS NOT profile.current_maximum_folder_capability)";

/// Profiles whose pointer names revision `?` and whose copied maximum folder capability or owner
/// disagrees with it.
const PROFILE_COPY_MISMATCH_BY_REVISION: &str = "SELECT profile.id
     FROM chat_access_profiles profile
     JOIN chat_access_profile_revisions revision ON revision.id = profile.current_revision_id
     WHERE revision.id = ?
       AND (revision.access_profile_id IS NOT profile.id
            OR revision.maximum_folder_capability
               IS NOT profile.current_maximum_folder_capability)";

/// Which pointer a mismatch query finds.
#[derive(Clone, Copy)]
enum PointerOwner {
    /// A teammate's current policy pointer.
    Teammate,
    /// An access profile's current revision pointer.
    Profile,
}

/// Contains a pointer for `key` that `mismatch` finds copying restrictiveness that disagrees
/// with the revision it names, without failing the apply.
///
/// The copy exists only so the sync merge can rank concurrent pointers by restrictiveness.
/// Commands write the pointer and its copy together and the merge keeps them in one coupled
/// group, so a disagreement can only come from a corrupt or dishonest operation that would let
/// a wider revision outrank a stricter one. The effective revision views give such a pointer no
/// effective revision, so dispatch, context, and tool checks refuse the teammate (or every
/// teammate on the profile) until an honest edit replaces the pointer. This hook records the
/// pointer and revokes the active authorizations of the affected teammates, which were granted
/// before the mismatch arrived. Failing the apply instead would stop every later apply on this
/// device. The check runs here rather than in a trigger because a pointer row may apply before
/// the revision it names; this hook runs again when the revision arrives.
async fn contain_mismatched_pointer(
    connection: &mut SqliteConnection,
    mismatch: &'static str,
    owner: PointerOwner,
    key: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let mismatched: Vec<String> = sqlx::query_scalar(mismatch)
        .bind(key)
        .fetch_all(&mut *connection)
        .await
        .map_err(persistence_error)?;
    let now = now();
    for owner_id in mismatched {
        let teammates = match owner {
            PointerOwner::Teammate => vec![owner_id.clone()],
            PointerOwner::Profile => sqlx::query_scalar(
                "SELECT DISTINCT teammate_id FROM chat_teammate_channel_memberships
                 WHERE access_profile_id = ? AND removed_at IS NULL ORDER BY teammate_id",
            )
            .bind(&owner_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(persistence_error)?,
        };
        for teammate_id in teammates {
            revoke(
                connection,
                &teammate_id,
                &now,
                MISMATCHED_POINTER_REASON,
                effects,
            )
            .await?;
        }
        effects.mismatched_pointers.insert(owner_id);
    }
    Ok(())
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat state derived from synced rows could not be saved",
        true,
    )
}

#[cfg(test)]
mod tests;
