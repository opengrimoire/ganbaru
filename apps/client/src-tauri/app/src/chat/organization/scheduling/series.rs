//! Repeating scheduled messages: the stored rule, the occurrence a claimed send posts, and how
//! the series moves past it.
//!
//! A series keeps its scheduled message and posts each occurrence under ids derived from the
//! occurrence key. Its executing device advances the replicated progress in the transaction of
//! each post, and skips an occurrence missed by more than the grace without posting it, counting
//! the skip in its local dispatch row.

use super::super::{ScheduledOccurrence, identifier_error, persistence_error};
use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_chat::coordination::contracts::ChatScheduledRecurrenceRead;
use ganbaru_chat::ids::{SCHEDULED_RECURRENCE_ID_PREFIX, derived_id};
use ganbaru_chat::scheduling::{
    MAX_RECURRING_SCHEDULES_PER_CHANNEL, MISSED_OCCURRENCE_GRACE_MINUTES, QuietPolicy,
    RecurrenceRule, RecurrenceSpec, WeekdaySet, next_occurrence, occurrence_key,
};
use ganbaru_chat_contracts::models::*;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

const MILLIS_PER_MINUTE: i64 = 60_000;

/// Columns of a recurrence row that hold its rule, selected under these names.
pub(super) const RECURRENCE_RULE_COLUMNS: &str = "recurrence.rule_kind, \
     recurrence.interval_minutes, recurrence.local_minute, recurrence.weekdays_mask, \
     recurrence.time_zone, recurrence.quiet_policy";

/// The stored rule and progress of one repeating scheduled message.
#[derive(Clone, Debug)]
pub(super) struct StoredSeries {
    /// Rule, zone, and quiet policy.
    pub(super) spec: RecurrenceSpec,
    /// The scheduled message's `scheduled_for`, which interval rules count from.
    pub(super) anchor_ms: i64,
    /// Stored due time of the pending occurrence.
    pub(super) next_occurrence_at: String,
}

/// The occurrence of a series that a claimed send posts, with owned values for a
/// [`ScheduledOccurrence`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingOccurrence {
    pub(super) key: String,
    pub(super) due_at: String,
    pub(super) next_at: String,
}

impl PendingOccurrence {
    /// The borrowed occurrence a send carries.
    pub(super) fn as_send(&self) -> ScheduledOccurrence<'_> {
        ScheduledOccurrence {
            key: &self.key,
            due_at: &self.due_at,
            next_at: &self.next_at,
        }
    }
}

/// What a claimed series does with its pending occurrence at a given time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum OccurrenceDecision {
    /// Post the occurrence now.
    Post(PendingOccurrence),
    /// The occurrence was missed by more than the grace: skip to `next_at` without posting.
    Skip { due_at: String, next_at: String },
    /// The occurrence is not due yet, as when the dispatch row is older than the progress.
    Wait { due_at: String },
}

/// What a claimed scheduled message posts.
#[derive(Clone, Debug)]
pub(super) enum ClaimedSeries {
    /// A one-time message.
    OneTime,
    /// The occurrence a repeating message posts now, with its rule for consent.
    Post {
        occurrence: PendingOccurrence,
        spec: RecurrenceSpec,
    },
    /// The claim was settled without a post: the occurrence was skipped, is not due yet, or the
    /// rule cannot be resolved here, which fails the message until it is retried.
    Settled,
}

/// Milliseconds since the Unix epoch of the stored timestamp `value`.
pub(super) fn timestamp_millis(value: &str) -> ChatResult<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.timestamp_millis())
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Stored Chat timestamp is invalid",
                false,
            )
        })
}

/// The stored timestamp of `millis` since the Unix epoch.
fn timestamp_at_millis(millis: i64) -> ChatResult<String> {
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| ChatError::validation("recurrence", "The repeat time is out of range"))
}

/// The id of the recurrence row of `scheduled_message_id`, the same on every device.
pub(super) fn recurrence_id(scheduled_message_id: &ChatScheduledMessageId) -> ChatResult<String> {
    derived_id(
        SCHEDULED_RECURRENCE_ID_PREFIX,
        &[scheduled_message_id.as_str()],
    )
}

/// The due time of the first occurrence of `spec` anchored at `scheduled_for`: the anchor
/// itself for an interval rule, and the first chosen local time at or after it otherwise.
pub(super) fn first_occurrence_at(
    spec: &RecurrenceSpec,
    scheduled_for: &UtcTimestamp,
) -> ChatResult<String> {
    let anchor_ms = timestamp_millis(scheduled_for.as_str())?;
    timestamp_at_millis(next_occurrence(
        spec,
        anchor_ms,
        anchor_ms.saturating_sub(1),
    )?)
}

/// Decides what the series `series` of `scheduled_message_id` does with its pending occurrence
/// at `now_ms`. An occurrence posts while it is at most [`MISSED_OCCURRENCE_GRACE_MINUTES`]
/// late; the occurrence after a post or a skip is the first one after both the pending one and
/// `now_ms`, so a late post never leaves an occurrence that is already due behind.
pub(super) fn decide_occurrence(
    scheduled_message_id: &str,
    series: &StoredSeries,
    now_ms: i64,
) -> ChatResult<OccurrenceDecision> {
    let due_ms = timestamp_millis(&series.next_occurrence_at)?;
    if due_ms > now_ms {
        return Ok(OccurrenceDecision::Wait {
            due_at: series.next_occurrence_at.clone(),
        });
    }
    let next_at = timestamp_at_millis(next_occurrence(&series.spec, series.anchor_ms, now_ms)?)?;
    if now_ms - due_ms > MISSED_OCCURRENCE_GRACE_MINUTES * MILLIS_PER_MINUTE {
        return Ok(OccurrenceDecision::Skip {
            due_at: series.next_occurrence_at.clone(),
            next_at,
        });
    }
    Ok(OccurrenceDecision::Post(PendingOccurrence {
        key: occurrence_key(scheduled_message_id, due_ms),
        due_at: series.next_occurrence_at.clone(),
        next_at,
    }))
}

/// The pending occurrence of `series` posted at once by Send now at `now_ms`, early or late.
pub(super) fn occurrence_sent_now(
    scheduled_message_id: &str,
    series: &StoredSeries,
    now_ms: i64,
) -> ChatResult<PendingOccurrence> {
    let due_ms = timestamp_millis(&series.next_occurrence_at)?;
    let next_ms = next_occurrence(&series.spec, series.anchor_ms, due_ms.max(now_ms))?;
    Ok(PendingOccurrence {
        key: occurrence_key(scheduled_message_id, due_ms),
        due_at: series.next_occurrence_at.clone(),
        next_at: timestamp_at_millis(next_ms)?,
    })
}

fn invalid_stored_rule() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored repeat rule is invalid",
        false,
    )
}

/// The rule of a row that selected [`RECURRENCE_RULE_COLUMNS`].
fn spec_from_row(row: &SqliteRow) -> ChatResult<RecurrenceSpec> {
    let kind: String = row.try_get("rule_kind").map_err(persistence_error)?;
    let rule = match kind.as_str() {
        "interval" => RecurrenceRule::Interval {
            minutes: row
                .try_get::<Option<i64>, _>("interval_minutes")
                .map_err(persistence_error)?
                .and_then(|minutes| u32::try_from(minutes).ok())
                .ok_or_else(invalid_stored_rule)?,
        },
        "fixed_time" => RecurrenceRule::FixedTime {
            local_minute: row
                .try_get::<Option<i64>, _>("local_minute")
                .map_err(persistence_error)?
                .and_then(|minute| u16::try_from(minute).ok())
                .ok_or_else(invalid_stored_rule)?,
            weekdays: row
                .try_get::<Option<i64>, _>("weekdays_mask")
                .map_err(persistence_error)?
                .and_then(|mask| u8::try_from(mask).ok())
                .and_then(WeekdaySet::from_mask)
                .ok_or_else(invalid_stored_rule)?,
        },
        _ => return Err(invalid_stored_rule()),
    };
    let quiet_policy: String = row.try_get("quiet_policy").map_err(persistence_error)?;
    Ok(RecurrenceSpec {
        rule,
        time_zone: row.try_get("time_zone").map_err(persistence_error)?,
        quiet_policy: QuietPolicy::parse(&quiet_policy).ok_or_else(invalid_stored_rule)?,
    })
}

/// The rule and progress of `scheduled_message_id`, or `None` for a one-time message.
pub(super) async fn read_series<'e, E>(
    executor: E,
    scheduled_message_id: &ChatScheduledMessageId,
) -> ChatResult<Option<StoredSeries>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    // The fragment is a constant column list, so the query carries no external text.
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {RECURRENCE_RULE_COLUMNS}, recurrence.next_occurrence_at, scheduled.scheduled_for
         FROM chat_scheduled_message_recurrences recurrence
         JOIN chat_scheduled_messages scheduled ON scheduled.id = recurrence.scheduled_message_id
         WHERE recurrence.scheduled_message_id = ?"
    )))
    .bind(scheduled_message_id.as_str())
    .fetch_optional(executor)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let scheduled_for: String = row.try_get("scheduled_for").map_err(persistence_error)?;
    Ok(Some(StoredSeries {
        spec: spec_from_row(&row)?,
        anchor_ms: timestamp_millis(&scheduled_for)?,
        next_occurrence_at: row
            .try_get("next_occurrence_at")
            .map_err(persistence_error)?,
    }))
}

/// The recurrence of a row that selected [`RECURRENCE_RULE_COLUMNS`] with the progress columns
/// `next_occurrence_at`, `fired_count`, `skipped_occurrences`, and `deferred_until`, or `None`
/// when the row has no recurrence.
pub(super) fn recurrence_read_from_row(
    row: &SqliteRow,
) -> ChatResult<Option<ChatScheduledRecurrenceRead>> {
    let Some(next_occurrence_at) = row
        .try_get::<Option<String>, _>("next_occurrence_at")
        .map_err(persistence_error)?
    else {
        return Ok(None);
    };
    let count = |column: &str| -> ChatResult<u64> {
        let value: Option<i64> = row.try_get(column).map_err(persistence_error)?;
        Ok(value
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0))
    };
    Ok(Some(ChatScheduledRecurrenceRead {
        spec: spec_from_row(row)?,
        next_occurrence_at: UtcTimestamp::new(next_occurrence_at).map_err(identifier_error)?,
        fired_count: count("fired_count")?,
        skipped_occurrences: count("skipped_occurrences")?,
        deferred_until: row
            .try_get::<Option<String>, _>("deferred_until")
            .map_err(persistence_error)?
            .map(UtcTimestamp::new)
            .transpose()
            .map_err(identifier_error)?,
    }))
}

/// Refuses another repeating message in `channel_id` once it holds
/// [`MAX_RECURRING_SCHEDULES_PER_CHANNEL`].
pub(super) async fn require_series_room(
    transaction: &mut Transaction<'_, Sqlite>,
    channel_id: &ChatChannelId,
) -> ChatResult<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_scheduled_message_recurrences recurrence
         JOIN chat_scheduled_messages scheduled ON scheduled.id = recurrence.scheduled_message_id
         WHERE scheduled.channel_id = ?",
    )
    .bind(channel_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if count >= i64::from(MAX_RECURRING_SCHEDULES_PER_CHANNEL) {
        return Err(ChatError::validation(
            "recurrence",
            "This channel already has the most repeating messages it can hold",
        ));
    }
    Ok(())
}

/// Stores the rule `spec` of `scheduled_message_id` with its first occurrence due at
/// `first_at`.
pub(super) async fn insert_series(
    transaction: &mut Transaction<'_, Sqlite>,
    scheduled_message_id: &ChatScheduledMessageId,
    spec: &RecurrenceSpec,
    first_at: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let (interval_minutes, local_minute, weekdays_mask) = match spec.rule {
        RecurrenceRule::Interval { minutes } => (Some(i64::from(minutes)), None, None),
        RecurrenceRule::FixedTime {
            local_minute,
            weekdays,
        } => (
            None,
            Some(i64::from(local_minute)),
            Some(i64::from(weekdays.mask())),
        ),
    };
    sqlx::query(
        "INSERT INTO chat_scheduled_message_recurrences
            (id, scheduled_message_id, rule_kind, interval_minutes, local_minute, weekdays_mask,
             time_zone, quiet_policy, next_occurrence_at, fired_count, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(recurrence_id(scheduled_message_id)?)
    .bind(scheduled_message_id.as_str())
    .bind(spec.rule.kind())
    .bind(interval_minutes)
    .bind(local_minute)
    .bind(weekdays_mask)
    .bind(&spec.time_zone)
    .bind(spec.quiet_policy.as_str())
    .bind(first_at)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// How a series moves past its pending occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SeriesStep {
    /// The occurrence posted.
    Posted,
    /// The occurrence was skipped without posting.
    Skipped,
}

/// Moves the series of `scheduled_message_id` from the occurrence due at `due_at` to the one
/// due at `next_at` while the claim `claim_token` holds it, and makes its dispatch row due then
/// with a fresh attempt count. Returns `false`, changing nothing, when the claim was lost or the
/// progress no longer names that occurrence.
async fn move_series(
    transaction: &mut Transaction<'_, Sqlite>,
    scheduled_message_id: &ChatScheduledMessageId,
    claim_token: &str,
    due_at: &str,
    next_at: &str,
    step: SeriesStep,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let fired = i64::from(step == SeriesStep::Posted);
    let skipped = i64::from(step == SeriesStep::Skipped);
    let moved = sqlx::query(
        "UPDATE chat_scheduled_message_recurrences
         SET next_occurrence_at = ?1, fired_count = fired_count + ?2, updated_at = ?3
         WHERE scheduled_message_id = ?4 AND next_occurrence_at = ?5
           AND EXISTS (
             SELECT 1 FROM chat_scheduled_message_dispatch dispatch
             WHERE dispatch.scheduled_message_id = ?4 AND dispatch.claim_token = ?6
           )",
    )
    .bind(next_at)
    .bind(fired)
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .bind(due_at)
    .bind(claim_token)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if moved.rows_affected() != 1 {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE chat_scheduled_message_dispatch
         SET attempt_count = 0, available_at = ?1, claimed_at = NULL, claim_token = NULL,
             deferred_until = NULL, skipped_occurrences = skipped_occurrences + ?2,
             updated_at = ?3
         WHERE scheduled_message_id = ?4 AND claim_token = ?5",
    )
    .bind(next_at)
    .bind(skipped)
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .bind(claim_token)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if step == SeriesStep::Posted {
        sqlx::query(
            "UPDATE chat_scheduled_messages SET last_error = NULL, updated_at = ?
             WHERE id = ? AND last_error IS NOT NULL",
        )
        .bind(now.as_str())
        .bind(scheduled_message_id.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    Ok(true)
}

/// Advances the series of `scheduled_message_id` past `occurrence` in the transaction that
/// posts it. Returns `false` when the claim `claim_token` was lost or the series moved, as
/// after a cancel, so the post rolls back.
pub(in crate::chat::organization) async fn advance_claimed_series(
    transaction: &mut Transaction<'_, Sqlite>,
    scheduled_message_id: &ChatScheduledMessageId,
    claim_token: &str,
    occurrence: ScheduledOccurrence<'_>,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    move_series(
        transaction,
        scheduled_message_id,
        claim_token,
        occurrence.due_at,
        occurrence.next_at,
        SeriesStep::Posted,
        now,
    )
    .await
}

/// Settles the claim `claim_token` on an occurrence that already posted: the series advances
/// past it when it still names it, and the claim is released otherwise.
pub(in crate::chat::organization) async fn finish_posted_occurrence(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
    claim_token: &str,
    occurrence: ScheduledOccurrence<'_>,
) -> ChatResult<()> {
    let now = super::super::now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let advanced = advance_claimed_series(
        &mut transaction,
        scheduled_message_id,
        claim_token,
        occurrence,
        &now,
    )
    .await?;
    if !advanced {
        release_claim(
            &mut transaction,
            scheduled_message_id,
            claim_token,
            None,
            &now,
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

/// Releases the claim `claim_token`, making the message due at `available_at` with a fresh
/// attempt count, or leaving its due time as it is.
async fn release_claim(
    transaction: &mut Transaction<'_, Sqlite>,
    scheduled_message_id: &ChatScheduledMessageId,
    claim_token: &str,
    available_at: Option<&str>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_scheduled_message_dispatch
         SET available_at = coalesce(?1, available_at),
             attempt_count = CASE WHEN ?1 IS NULL THEN attempt_count ELSE 0 END,
             claimed_at = NULL, claim_token = NULL, updated_at = ?2
         WHERE scheduled_message_id = ?3 AND claim_token = ?4",
    )
    .bind(available_at)
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .bind(claim_token)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Resolves what the due message `scheduled_message_id`, claimed by the dispatch under
/// `claim_token`, posts at `now`. A missed occurrence is skipped and counted, an occurrence that
/// is not due yet releases the claim until it is, and a rule this device cannot resolve fails
/// the message until it is retried.
pub(super) async fn resolve_claimed_series(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
    claim_token: &str,
    now: &UtcTimestamp,
) -> ChatResult<ClaimedSeries> {
    let Some(series) = read_series(pool, scheduled_message_id).await? else {
        return Ok(ClaimedSeries::OneTime);
    };
    let decision = match decide_occurrence(
        scheduled_message_id.as_str(),
        &series,
        timestamp_millis(now.as_str())?,
    ) {
        Ok(decision) => decision,
        Err(error) => {
            super::record_send_failure(pool, scheduled_message_id, claim_token, &error, None)
                .await?;
            return Ok(ClaimedSeries::Settled);
        }
    };
    let (due_at, skip_to) = match decision {
        OccurrenceDecision::Post(occurrence) => {
            return Ok(ClaimedSeries::Post {
                occurrence,
                spec: series.spec,
            });
        }
        OccurrenceDecision::Skip { due_at, next_at } => (due_at, Some(next_at)),
        OccurrenceDecision::Wait { due_at } => (due_at, None),
    };
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    match skip_to {
        Some(next_at) => {
            let skipped = move_series(
                &mut transaction,
                scheduled_message_id,
                claim_token,
                &due_at,
                &next_at,
                SeriesStep::Skipped,
                now,
            )
            .await?;
            if !skipped {
                release_claim(
                    &mut transaction,
                    scheduled_message_id,
                    claim_token,
                    None,
                    now,
                )
                .await?;
            }
        }
        None => {
            release_claim(
                &mut transaction,
                scheduled_message_id,
                claim_token,
                Some(&due_at),
                now,
            )
            .await?;
        }
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(ClaimedSeries::Settled)
}

#[cfg(test)]
mod tests;
