//! Device-local Chat state derived from replicated rows that sync materialized.
//!
//! Commands keep access state, audience revisions, and message search current through triggers
//! that stand down while sync applies rows. After the sync engine writes rows of one table,
//! [`after_materialize`] does the same work for the changed keys inside the engine transaction,
//! with plain SQL and no dependency on the engine, so any caller holding the connection can run
//! it. It never writes replicated rows.

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
/// Replicated table of messages, keyed by item id.
pub const MESSAGES_TABLE: &str = "chat_communication_messages";
/// Replicated table of message revisions, keyed by id.
pub const MESSAGE_REVISIONS_TABLE: &str = "chat_communication_message_revisions";

/// Reason recorded on authorizations revoked because an applied profile revision narrowed it.
pub const PROFILE_REDUCTION_REASON: &str = "Access profile authority was reduced";

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
}

impl ChatSyncEffects {
    /// Returns whether no device-local state changed.
    pub fn is_empty(&self) -> bool {
        self.access_state_created.is_empty()
            && self.access_revised.is_empty()
            && self.audience_revised.is_empty()
            && self.revoked_teammates.is_empty()
            && self.search_rebuilt.is_empty()
    }
}

/// Recomputes the device-local Chat state derived from the rows of `table` whose keys are in
/// `changed_keys`, after sync inserted, updated, or deleted them.
///
/// It mirrors the gated command triggers: a teammate gets its access state and a conversation
/// its audience state, a profile whose effective revision arrived or changed bumps the audience
/// of the conversations whose teammates use that profile, and a message or revision rebuilds the
/// message's search row (a deleted message loses it). An effective profile revision that narrows
/// authority also bumps the access revision of those teammates and revokes the active
/// authorizations granted under a wider revision, as the command that saves a reduction does.
/// Timestamps come from the changed rows, except revocations, which happen now on this device.
/// Running it again for the same keys leaves the same rows, apart from the counters a profile
/// revision bumps again. Tables without derived state return no effects.
///
/// It also refuses the applied rows, with an error that aborts the engine transaction, when a
/// teammate or profile pointer copies a safety mode or maximum folder capability that differs
/// from the revision it names, once both rows are present.
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
                require_matching_copies(connection, POLICY_COPY_MISMATCH_BY_TEAMMATE, key).await?;
                create_access_state(connection, key, &mut effects).await?;
            }
        }
        POLICY_REVISIONS_TABLE => {
            for key in keys {
                require_matching_copies(connection, POLICY_COPY_MISMATCH_BY_REVISION, key).await?;
            }
        }
        CONVERSATIONS_TABLE => {
            for key in keys {
                create_audience_state(connection, key, &mut effects).await?;
            }
        }
        ACCESS_PROFILES_TABLE => {
            for key in keys {
                require_matching_copies(connection, PROFILE_COPY_MISMATCH_BY_PROFILE, key).await?;
                apply_effective_profile_revision(connection, key, &mut effects).await?;
            }
        }
        ACCESS_PROFILE_REVISIONS_TABLE => {
            let mut profile_ids = BTreeSet::new();
            for key in keys {
                require_matching_copies(connection, PROFILE_COPY_MISMATCH_BY_REVISION, key).await?;
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
        MESSAGES_TABLE => {
            for key in keys {
                rebuild_message_search(connection, key, &mut effects).await?;
            }
        }
        MESSAGE_REVISIONS_TABLE => {
            let mut message_ids = BTreeSet::new();
            for key in keys {
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

async fn create_audience_state(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
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
    }
    Ok(())
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
                participant.display_name, revision.normalized_markdown
         FROM chat_communication_messages message
         JOIN chat_conversation_items item ON item.id = message.item_id
         JOIN chat_participants participant ON participant.id = message.author_participant_id
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

/// Refuses the applied rows when `mismatch` finds a pointer for `key` whose copied
/// restrictiveness disagrees with the revision it names.
///
/// The copy exists only so the sync merge can rank concurrent pointers by restrictiveness, while
/// readers enforce the revision row itself. Commands write the pointer and its copy together and
/// the merge keeps them in one coupled group, so a disagreement can only come from a corrupt or
/// dishonest operation that would let a wider revision outrank a stricter one. The check runs
/// here rather than in a trigger because a pointer row may apply before the revision it names; a
/// trigger would refuse that correct order, while this hook runs again when the revision arrives.
async fn require_matching_copies(
    connection: &mut SqliteConnection,
    mismatch: &'static str,
    key: &str,
) -> ChatResult<()> {
    let mismatched: Option<String> = sqlx::query_scalar(mismatch)
        .bind(key)
        .fetch_optional(&mut *connection)
        .await
        .map_err(persistence_error)?;
    match mismatched {
        Some(owner) => Err(ChatError::new(
            ChatErrorCode::Validation,
            format!(
                "Synced Chat row {owner} copies restrictiveness that differs from the revision \
                 it names"
            ),
            false,
        )),
        None => Ok(()),
    }
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
