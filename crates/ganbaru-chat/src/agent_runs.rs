//! Durable bridge between organizational assignments and provider turns.

use super::models::{
    ChatAgentRunId, ChatAuthorizationRevisionId, ChatError, ChatErrorCode, ChatResult,
    ChatTeammatePolicyRevisionId, ChatThreadId, ChatTurnId, ChatWorkAssignmentId, UtcTimestamp,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};

/// Run states that end a run. A settled run never changes state again, so every run write
/// excludes them.
pub const SETTLED_RUN_STATES: [&str; 3] = ["completed", "failed", "cancelled"];

/// SQL condition that holds while the run state in `column` is not settled, built from
/// `SETTLED_RUN_STATES` so run writes and the projection check share one list.
pub fn unsettled_run_state_sql(column: &str) -> String {
    let settled = SETTLED_RUN_STATES
        .iter()
        .map(|state| format!("'{state}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{column} NOT IN ({settled})")
}

#[derive(Clone, Debug)]
pub struct AgentRunBinding {
    pub run_id: ChatAgentRunId,
    pub assignment_id: ChatWorkAssignmentId,
    pub project_id: String,
    pub teammate_policy_revision_id: ChatTeammatePolicyRevisionId,
    pub authorization_revision_id: ChatAuthorizationRevisionId,
    pub authorization_scope_digest: String,
    pub working_folder_id: Option<String>,
    pub scratch_generation_id: Option<String>,
    pub execution_environment_id: Option<String>,
    pub run_ordinal: u64,
}

/// Why the native session of a handoff run was not continued.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoffReason {
    /// The continuation key of the earlier run is missing or differs.
    KeyChanged,
    /// The authorization of the earlier run is no longer allowed.
    AuthorityRevoked,
    /// The provider could not resume the earlier native session.
    NativeSessionLost,
}

impl HandoffReason {
    /// The stored value of this reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KeyChanged => "key_changed",
            Self::AuthorityRevoked => "authority_revoked",
            Self::NativeSessionLost => "native_session_lost",
        }
    }
}

/// Why a run started, in the precedence order the dispatcher decides it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptReason {
    /// A new native session for an assignment with no earlier session to continue.
    Fresh,
    /// The native session of an earlier run in the same reply thread continues.
    Continuation,
    /// A new native session replaces one that could have continued.
    Handoff(HandoffReason),
    /// A new native session for an assignment whose earlier run did not finish.
    Retry,
}

impl AttemptReason {
    /// The stored value of this reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Continuation => "continuation",
            Self::Handoff(_) => "handoff",
            Self::Retry => "retry",
        }
    }

    /// The stored handoff reason, present only for a handoff.
    pub fn handoff_reason(self) -> Option<HandoffReason> {
        match self {
            Self::Handoff(reason) => Some(reason),
            Self::Fresh | Self::Continuation | Self::Retry => None,
        }
    }
}

/// Whether a run sent the whole context package or only the sources its session lacks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextDelivery {
    Full,
    Delta,
}

impl ContextDelivery {
    /// The stored value of this delivery.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Delta => "delta",
        }
    }
}

/// How a context source reached a native session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryKind {
    /// A record of the context package that started the run.
    Package,
    /// A shortened record that stands for a long earlier reply.
    Pointer,
    /// A follow-up steered into the running turn.
    Steer,
    /// A follow-up queued while no turn was running, sent as the next turn.
    QueuedContinuation,
    /// A record announcing that a source was deleted.
    Removal,
}

impl DeliveryKind {
    /// The stored value of this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Pointer => "pointer",
            Self::Steer => "steer",
            Self::QueuedContinuation => "queued_continuation",
            Self::Removal => "removal",
        }
    }
}

/// A context source a run sends, identified by its source id and the hash of the text sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveredSource {
    pub source_id: String,
    pub content_sha256: String,
    pub kind: DeliveryKind,
}

/// Why a run started and what context it sends, fixed once the run leaves `starting`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunAttempt {
    pub reason: AttemptReason,
    /// Hash of the system-level instructions the run sends, part of its continuation key.
    pub instructions_sha256: Option<String>,
    /// Hash of the model selection the run sends, part of its continuation key.
    pub model_selection_sha256: Option<String>,
    pub context_delivery: ContextDelivery,
    /// Bytes of context the run sends in its first turn.
    pub delivered_context_bytes: u64,
    /// Sources the run sends, recorded only once its launch reaches the provider.
    pub delivered_sources: Vec<DeliveredSource>,
}

/// Launch data of an assignment run that is not part of its stored authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunLaunch {
    /// Claim token of the dispatch job that launches the run. A launch whose claim was lost
    /// writes no assignment or dispatch state.
    pub claim_token: String,
    pub attempt: RunAttempt,
}

#[derive(Clone, Debug, Default)]
pub enum TurnOrigin {
    #[default]
    Direct,
    Assignment {
        developer_instructions: String,
        run: Box<AgentRunBinding>,
        launch: Box<RunLaunch>,
    },
}

impl TurnOrigin {
    /// The launch data of an assignment run.
    pub fn launch(&self) -> Option<&RunLaunch> {
        match self {
            Self::Direct => None,
            Self::Assignment { launch, .. } => Some(launch.as_ref()),
        }
    }

    pub fn developer_instructions(&self) -> Option<&str> {
        match self {
            Self::Direct => None,
            Self::Assignment {
                developer_instructions,
                ..
            } => Some(developer_instructions),
        }
    }

    pub fn run(&self) -> Option<&AgentRunBinding> {
        match self {
            Self::Direct => None,
            Self::Assignment { run, .. } => Some(run.as_ref()),
        }
    }

    /// The run binding and launch data of an assignment run.
    pub fn assignment_run(&self) -> Option<(&AgentRunBinding, &RunLaunch)> {
        match self {
            Self::Direct => None,
            Self::Assignment { run, launch, .. } => Some((run.as_ref(), launch.as_ref())),
        }
    }
}

pub struct StartingAgentRun<'a> {
    pub binding: &'a AgentRunBinding,
    pub launch: &'a RunLaunch,
    pub provider_turn_id: &'a ChatTurnId,
    pub provider_thread_id: &'a ChatThreadId,
    pub now: &'a UtcTimestamp,
}

/// How a launch ended for the assignment that started it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchSettlement {
    /// The launch result was recorded on the assignment.
    Recorded,
    /// The assignment settled while the provider turn launched, so the run was cancelled and
    /// the caller must stop the provider session it started.
    Superseded,
    /// The run had already settled, for example through a revocation, so nothing was written
    /// and the caller must stop the provider session it started.
    RunSettled,
    /// The dispatch job no longer holds the claim that launched the run, so the run was
    /// cancelled, the assignment and job were left to the new owner, and the caller must stop
    /// the provider session it started.
    ClaimLost,
}

impl LaunchSettlement {
    /// Whether the caller must stop the provider session the launch started.
    pub fn stops_session(self) -> bool {
        matches!(self, Self::Superseded | Self::RunSettled | Self::ClaimLost)
    }
}

/// Whether the dispatch job of `assignment_id` is still claimed under `claim_token`.
async fn holds_dispatch_claim(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    claim_token: &str,
) -> ChatResult<bool> {
    let held: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM chat_assignment_dispatch_jobs
         WHERE assignment_id = ? AND state = 'claimed' AND claim_token = ?",
    )
    .bind(assignment_id.as_str())
    .bind(claim_token)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(held.is_some())
}

/// Records the starting run of `run`, refusing when its assignment already settled or its
/// dispatch claim was lost so the launch aborts before the provider starts. A settle from any
/// device wins, so launch writes leave a completed, failed, or cancelled assignment untouched.
pub async fn insert_starting_run(
    transaction: &mut Transaction<'_, Sqlite>,
    run: StartingAgentRun<'_>,
) -> ChatResult<()> {
    let settled: Option<bool> = sqlx::query_scalar(
        "SELECT state IN ('completed', 'failed', 'cancelled')
         FROM chat_work_assignments WHERE id = ?",
    )
    .bind(run.binding.assignment_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if settled != Some(false) {
        return Err(ChatError::invalid_transition(
            "The work assignment settled before its run started",
        ));
    }
    if !holds_dispatch_claim(
        transaction,
        &run.binding.assignment_id,
        &run.launch.claim_token,
    )
    .await?
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The assignment dispatch claim was lost before its run started",
            true,
        ));
    }
    let attempt = &run.launch.attempt;
    sqlx::query(
        "INSERT INTO chat_agent_runs
            (id, assignment_id, project_id, working_folder_id,
             execution_environment_id, scratch_generation_id,
             teammate_policy_revision_id, authorization_revision_id,
             authorization_scope_digest,
             provider_turn_id, provider_thread_id, state, run_ordinal,
             attempt_reason, handoff_reason, instructions_sha256, model_selection_sha256,
             context_delivery, delivered_context_bytes, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'starting', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run.binding.run_id.as_str())
    .bind(run.binding.assignment_id.as_str())
    .bind(&run.binding.project_id)
    .bind(&run.binding.working_folder_id)
    .bind(&run.binding.execution_environment_id)
    .bind(&run.binding.scratch_generation_id)
    .bind(run.binding.teammate_policy_revision_id.as_str())
    .bind(run.binding.authorization_revision_id.as_str())
    .bind(&run.binding.authorization_scope_digest)
    .bind(run.provider_turn_id.as_str())
    .bind(run.provider_thread_id.as_str())
    .bind(i64_value(run.binding.run_ordinal)?)
    .bind(attempt.reason.as_str())
    .bind(attempt.reason.handoff_reason().map(HandoffReason::as_str))
    .bind(&attempt.instructions_sha256)
    .bind(&attempt.model_selection_sha256)
    .bind(attempt.context_delivery.as_str())
    .bind(i64_value(attempt.delivered_context_bytes)?)
    .bind(run.now.as_str())
    .bind(run.now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Records the launch result of `binding` on its run, assignment, and dispatch job, and the
/// context sources a launch that reached the provider delivered.
///
/// A launch whose dispatch claim was lost, for example because a stale claim was requeued and
/// claimed again, writes only its run: the run is cancelled, the assignment, dispatch job, and
/// messages stay with the new claim, and the result reports the claim as lost so the caller
/// stops the provider session.
///
/// An assignment that settled meanwhile, for example through a cancel applied from another
/// device, keeps its settled state: the run is cancelled, the dispatch job ends, and the result
/// reports the launch as superseded so the caller stops the provider session.
///
/// A run that already settled, for example through a revocation, or that was deleted with its
/// assignment, thread, or turn, is never rewritten: the transaction rolls back, leaving the
/// assignment, dispatch job, and messages as they were, and the result reports the run as
/// settled so the caller stops the provider session.
pub async fn settle_launch(
    pool: &sqlx::SqlitePool,
    binding: &AgentRunBinding,
    launch: &RunLaunch,
    launch_error: Option<&ChatError>,
    now: &UtcTimestamp,
) -> ChatResult<LaunchSettlement> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if !holds_dispatch_claim(
        &mut transaction,
        &binding.assignment_id,
        &launch.claim_token,
    )
    .await?
    {
        if !settle_run_state(&mut transaction, binding, "cancelled", now).await? {
            transaction.rollback().await.map_err(persistence_error)?;
            return Ok(LaunchSettlement::RunSettled);
        }
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(LaunchSettlement::ClaimLost);
    }
    let assignment_updated = match launch_error {
        Some(error) => sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'failed', state_reason = ?, settled_at = ?,
                 revision = revision + 1, updated_at = ?
             WHERE id = ? AND state NOT IN ('completed', 'failed', 'cancelled')",
        )
        .bind(&error.message)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(binding.assignment_id.as_str()),
        None => sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'working', state_reason = NULL,
                 revision = revision + 1, updated_at = ?
             WHERE id = ? AND state NOT IN ('completed', 'failed', 'cancelled')",
        )
        .bind(now.as_str())
        .bind(binding.assignment_id.as_str()),
    }
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected()
        > 0;
    let (run_state, job_state, settlement) = match (assignment_updated, launch_error) {
        (false, _) => ("cancelled", "cancelled", LaunchSettlement::Superseded),
        (true, Some(_)) => ("failed", "failed", LaunchSettlement::Recorded),
        (true, None) => ("working", "completed", LaunchSettlement::Recorded),
    };
    if !settle_run_state(&mut transaction, binding, run_state, now).await? {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(LaunchSettlement::RunSettled);
    }
    sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = ?, last_error = ?, updated_at = ?
         WHERE assignment_id = ? AND state = 'claimed' AND claim_token = ?",
    )
    .bind(job_state)
    .bind(launch_error.map(|error| error.message.as_str()))
    .bind(now.as_str())
    .bind(binding.assignment_id.as_str())
    .bind(&launch.claim_token)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if run_state == "working" {
        insert_delivered_sources(
            &mut transaction,
            &binding.run_id,
            &launch.attempt.delivered_sources,
            now,
        )
        .await?;
    }
    if let (true, Some(error)) = (assignment_updated, launch_error) {
        insert_assignment_failure_message(
            &mut transaction,
            &binding.assignment_id,
            LaunchAttempt::Run(&binding.run_id),
            &error.message,
            now,
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(settlement)
}

/// Moves the unsettled run of `binding` to `state`, stamping its start or settle time. Returns
/// false when the run already settled or no longer exists.
async fn settle_run_state(
    connection: &mut SqliteConnection,
    binding: &AgentRunBinding,
    state: &str,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let run_update = format!(
        "UPDATE chat_agent_runs
         SET state = ?,
             started_at = CASE WHEN ? = 'working' THEN ? ELSE started_at END,
             settled_at = CASE WHEN ? = 'working' THEN settled_at ELSE ? END,
             updated_at = ?
         WHERE id = ? AND {}",
        unsettled_run_state_sql("state")
    );
    Ok(sqlx::query(sqlx::AssertSqlSafe(run_update.as_str()))
        .bind(state)
        .bind(state)
        .bind(now.as_str())
        .bind(state)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(binding.run_id.as_str())
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?
        .rows_affected()
        > 0)
}

/// Records that `run_id` delivered `sources` to its native session. A source delivered twice
/// under the same content keeps its first record, so a replayed delivery never fails after the
/// provider already received it.
pub async fn insert_delivered_sources(
    connection: &mut SqliteConnection,
    run_id: &ChatAgentRunId,
    sources: &[DeliveredSource],
    now: &UtcTimestamp,
) -> ChatResult<()> {
    for source in sources {
        sqlx::query(
            "INSERT INTO chat_agent_run_delivered_sources
                (run_id, source_id, content_sha256, delivery_kind, delivered_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (run_id, source_id, content_sha256) DO NOTHING",
        )
        .bind(run_id.as_str())
        .bind(&source.source_id)
        .bind(&source.content_sha256)
        .bind(source.kind.as_str())
        .bind(now.as_str())
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

/// The launch attempt a failure message reports, which keys the message so each attempt posts
/// at most one.
#[derive(Clone, Copy, Debug)]
pub enum LaunchAttempt<'a> {
    /// A dispatch that failed before it recorded a run, identified by its claim token.
    Claim(&'a str),
    /// A recorded run whose launch failed.
    Run(&'a ChatAgentRunId),
}

impl LaunchAttempt<'_> {
    fn key(self) -> String {
        match self {
            Self::Claim(token) => format!("claim:{token}"),
            Self::Run(run_id) => format!("run:{}", run_id.as_str()),
        }
    }
}

/// Posts the failure message of `attempt` in the reply thread of `assignment_id`.
///
/// The message id derives from the assignment and the attempt, so a repeated settlement of the
/// same attempt writes nothing and leaves the thread and conversation revisions as they were.
/// Returns whether a message was written.
pub async fn insert_assignment_failure_message(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    attempt: LaunchAttempt<'_>,
    reason: &str,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let row = sqlx::query(
        "SELECT assignment.reply_thread_id, assignment.teammate_id, thread.conversation_id,
                participant.display_name AS author_label_snapshot
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_participants participant ON participant.id = assignment.teammate_id
         WHERE assignment.id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let reply_thread_id: String = row.try_get("reply_thread_id").map_err(persistence_error)?;
    let teammate_id: String = row.try_get("teammate_id").map_err(persistence_error)?;
    let conversation_id: String = row.try_get("conversation_id").map_err(persistence_error)?;
    let author_label_snapshot: String = row
        .try_get("author_label_snapshot")
        .map_err(persistence_error)?;
    let hash = sha256_hex(format!("{}:{}", assignment_id.as_str(), attempt.key()).as_bytes());
    let item_id = format!("organizational-item:{hash}");
    let revision_id = format!("organizational-revision:{hash}");
    let order_key =
        crate::timeline::next_order_key(connection, &conversation_id, Some(&reply_thread_id))
            .await?;
    let inserted = sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&item_id)
    .bind(&conversation_id)
    .bind(&reply_thread_id)
    .bind(&order_key)
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected()
        > 0;
    if !inserted {
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&item_id)
    .bind(&teammate_id)
    .bind(&author_label_snapshot)
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let rich_content = json!({
        "type": "agent_update",
        "updateKind": "failure",
        "assignmentId": assignment_id,
        "agentRunId": null,
        "payload": { "launchFailure": true },
    });
    let rich_content_data = serde_json::to_string(&rich_content).map_err(serialization_error)?;
    let bounded = crate::timeline::bounded_projected_revision(reason, rich_content)?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(&revision_id)
    .bind(&item_id)
    .bind(&bounded.normalized_markdown)
    .bind(&bounded.rich_content_data)
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(&revision_id)
        .bind(&item_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, 'failure', ?, ?)",
    )
    .bind(&item_id)
    .bind(assignment_id.as_str())
    .bind(&rich_content_data)
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_reply_threads SET last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&reply_thread_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversations SET last_activity_at = ?, revision = revision + 1,
             updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&conversation_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(true)
}

fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| ChatError::validation("number", "Value is too large"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat agent-run persistence failed",
        true,
    )
}

fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat agent-run content could not be encoded",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AttemptReason, ContextDelivery, DeliveryKind, HandoffReason, LaunchSettlement,
        SETTLED_RUN_STATES, unsettled_run_state_sql,
    };

    #[tokio::test]
    async fn the_unsettled_condition_admits_only_active_run_states() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let condition = format!("SELECT {}", unsettled_run_state_sql("?"));
        for state in [
            "queued",
            "starting",
            "working",
            "waiting",
            "completed",
            "failed",
            "cancelled",
        ] {
            let unsettled: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(condition.as_str()))
                .bind(state)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(unsettled, !SETTLED_RUN_STATES.contains(&state), "{state}");
        }
    }

    #[test]
    fn launches_that_lost_their_run_stop_the_provider_session() {
        assert!(!LaunchSettlement::Recorded.stops_session());
        assert!(LaunchSettlement::Superseded.stops_session());
        assert!(LaunchSettlement::RunSettled.stops_session());
        assert!(LaunchSettlement::ClaimLost.stops_session());
    }

    #[test]
    fn only_handoffs_carry_a_handoff_reason() {
        for reason in [
            AttemptReason::Fresh,
            AttemptReason::Continuation,
            AttemptReason::Retry,
        ] {
            assert_eq!(reason.handoff_reason(), None, "{reason:?}");
        }
        for handoff in [
            HandoffReason::KeyChanged,
            HandoffReason::AuthorityRevoked,
            HandoffReason::NativeSessionLost,
        ] {
            let reason = AttemptReason::Handoff(handoff);
            assert_eq!(reason.as_str(), "handoff");
            assert_eq!(reason.handoff_reason(), Some(handoff));
        }
    }

    /// Asserts that `table` has `column` and that its stored definition names `value`.
    async fn assert_column_accepts(
        pool: &sqlx::SqlitePool,
        table: &str,
        column: &str,
        value: &str,
    ) {
        let check =
            format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name = '{column}'");
        let exists: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(check.as_str()))
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(exists, 1, "{table}.{column}");
        let sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?")
                .bind(table)
                .fetch_one(pool)
                .await
                .unwrap();
        assert!(
            sql.contains(&format!("'{value}'")),
            "{table}.{column} accepts {value}"
        );
    }

    #[tokio::test]
    async fn stored_attempt_values_match_the_schema_vocabulary() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        for reason in [
            AttemptReason::Fresh,
            AttemptReason::Continuation,
            AttemptReason::Handoff(HandoffReason::KeyChanged),
            AttemptReason::Retry,
        ] {
            assert_column_accepts(&pool, "chat_agent_runs", "attempt_reason", reason.as_str())
                .await;
        }
        for handoff in [
            HandoffReason::KeyChanged,
            HandoffReason::AuthorityRevoked,
            HandoffReason::NativeSessionLost,
        ] {
            assert_column_accepts(&pool, "chat_agent_runs", "handoff_reason", handoff.as_str())
                .await;
        }
        for delivery in [ContextDelivery::Full, ContextDelivery::Delta] {
            assert_column_accepts(
                &pool,
                "chat_agent_runs",
                "context_delivery",
                delivery.as_str(),
            )
            .await;
        }
        for kind in [
            DeliveryKind::Package,
            DeliveryKind::Pointer,
            DeliveryKind::Steer,
            DeliveryKind::QueuedContinuation,
            DeliveryKind::Removal,
        ] {
            assert_column_accepts(
                &pool,
                "chat_agent_run_delivered_sources",
                "delivery_kind",
                kind.as_str(),
            )
            .await;
        }
    }
}
