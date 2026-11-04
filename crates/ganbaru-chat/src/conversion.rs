//! Chat conversion pass run at vault open, before sync captures are seeded, and the derived
//! keys of the Chat tables keyed by several columns.
//!
//! The conversations and timeline replication migrations leave `'pending:'` placeholders in the
//! `sync_key` of conversation memberships, AI channel memberships, working folder grants, and
//! conversation and reply thread read cursors. This pass rewrites each placeholder to the key every device derives from the row's key columns. It is
//! idempotent, so a vault that is already converted is left unchanged.

use ganbaru_sync_contracts::{PENDING_KEY_PREFIX, derived_key};
use sqlx::{AssertSqlSafe, SqliteConnection, SqlitePool};

/// Prefix of conversation membership keys, over the conversation and participant.
pub const MEMBERSHIP_KEY_PREFIX: &str = "conversation-membership";
/// Prefix of AI channel membership keys, over the conversation and teammate.
pub const CHANNEL_MEMBERSHIP_KEY_PREFIX: &str = "ai-channel-membership";
/// Prefix of working folder grant keys, over the conversation, teammate, and working folder.
pub const FOLDER_GRANT_KEY_PREFIX: &str = "folder-grant";
/// Prefix of conversation read cursor keys, over the conversation and participant.
pub const CONVERSATION_CURSOR_KEY_PREFIX: &str = "conversation-read-cursor";
/// Prefix of reply thread read cursor keys, over the reply thread and participant.
pub const REPLY_CURSOR_KEY_PREFIX: &str = "reply-thread-read-cursor";

fn key(prefix: &str, parts: &[&str]) -> Result<String, String> {
    derived_key(prefix, parts)
        .map(|key| key.as_str().to_string())
        .map_err(|error| format!("derive {prefix} key: {error}"))
}

/// The `sync_key` of a conversation membership.
pub fn membership_key(conversation_id: &str, participant_id: &str) -> Result<String, String> {
    key(MEMBERSHIP_KEY_PREFIX, &[conversation_id, participant_id])
}

/// The `sync_key` of an AI channel membership.
pub fn channel_membership_key(conversation_id: &str, teammate_id: &str) -> Result<String, String> {
    key(
        CHANNEL_MEMBERSHIP_KEY_PREFIX,
        &[conversation_id, teammate_id],
    )
}

/// The `sync_key` of a working folder grant.
pub fn folder_grant_key(
    conversation_id: &str,
    teammate_id: &str,
    working_folder_id: &str,
) -> Result<String, String> {
    key(
        FOLDER_GRANT_KEY_PREFIX,
        &[conversation_id, teammate_id, working_folder_id],
    )
}

/// The `sync_key` of a participant's conversation read cursor.
pub fn conversation_cursor_key(
    conversation_id: &str,
    participant_id: &str,
) -> Result<String, String> {
    key(
        CONVERSATION_CURSOR_KEY_PREFIX,
        &[conversation_id, participant_id],
    )
}

/// The `sync_key` of a participant's reply thread read cursor.
pub fn reply_cursor_key(reply_thread_id: &str, participant_id: &str) -> Result<String, String> {
    key(REPLY_CURSOR_KEY_PREFIX, &[reply_thread_id, participant_id])
}

/// A table whose placeholder keys are rewritten from its primary key columns.
struct KeyedTable {
    table: &'static str,
    parts: &'static [&'static str],
    derive: fn(&[String]) -> Result<String, String>,
}

const KEYED_TABLES: [KeyedTable; 5] = [
    KeyedTable {
        table: "chat_conversation_memberships",
        parts: &["conversation_id", "participant_id"],
        derive: |parts| membership_key(&parts[0], &parts[1]),
    },
    KeyedTable {
        table: "chat_teammate_channel_memberships",
        parts: &["conversation_id", "teammate_id"],
        derive: |parts| channel_membership_key(&parts[0], &parts[1]),
    },
    KeyedTable {
        table: "chat_teammate_working_folder_grants",
        parts: &["conversation_id", "teammate_id", "working_folder_id"],
        derive: |parts| folder_grant_key(&parts[0], &parts[1], &parts[2]),
    },
    KeyedTable {
        table: "chat_conversation_read_cursors",
        parts: &["conversation_id", "participant_id"],
        derive: |parts| conversation_cursor_key(&parts[0], &parts[1]),
    },
    KeyedTable {
        table: "chat_reply_thread_read_cursors",
        parts: &["reply_thread_id", "participant_id"],
        derive: |parts| reply_cursor_key(&parts[0], &parts[1]),
    },
];

/// Matches placeholder keys.
const PENDING_CONDITION: &str = "substr(sync_key, 1, length(?1)) = ?1";

/// Converts the Chat tables in one transaction and returns how many keys it rewrote.
pub async fn convert(pool: &SqlitePool) -> Result<u64, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin chat conversion: {error}"))?;
    let mut rekeyed = 0;
    for table in &KEYED_TABLES {
        rekeyed += rekey_table(&mut tx, table).await?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit chat conversion: {error}"))?;
    Ok(rekeyed)
}

/// Refuses a vault where any Chat key is still a placeholder, so sync never publishes one.
pub async fn assert_converted(pool: &SqlitePool) -> Result<(), String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire chat conversion check: {error}"))?;
    for table in &KEYED_TABLES {
        let remaining: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT count(*) FROM {} WHERE {PENDING_CONDITION}",
            table.table
        )))
        .bind(PENDING_KEY_PREFIX)
        .fetch_one(&mut *conn)
        .await
        .map_err(|error| format!("check {} keys: {error}", table.table))?;
        if remaining > 0 {
            return Err(format!(
                "{} still has {remaining} placeholder keys; run the chat conversion first",
                table.table
            ));
        }
    }
    Ok(())
}

/// Rewrites the placeholder keys of `table`. The tables have no rowid, so each row is addressed
/// by its primary key columns.
async fn rekey_table(conn: &mut SqliteConnection, table: &KeyedTable) -> Result<u64, String> {
    let columns = table.parts.join(", ");
    let rows = sqlx::query_as::<_, (String,)>(AssertSqlSafe(format!(
        "SELECT json_array({columns}) FROM {} WHERE {PENDING_CONDITION} ORDER BY {columns}",
        table.table
    )))
    .bind(PENDING_KEY_PREFIX)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load {} placeholder keys: {error}", table.table))?;
    let filter = table
        .parts
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ?{}", index + 2))
        .collect::<Vec<_>>()
        .join(" AND ");
    let update = format!("UPDATE {} SET sync_key = ?1 WHERE {filter}", table.table);
    for (parts,) in &rows {
        let parts = serde_json::from_str::<Vec<String>>(parts)
            .map_err(|error| format!("read {} key columns: {error}", table.table))?;
        if parts.len() != table.parts.len() {
            return Err(format!("read {} key columns: wrong count", table.table));
        }
        let key = (table.derive)(&parts)?;
        let mut query = sqlx::query(AssertSqlSafe(update.clone())).bind(key);
        for part in &parts {
            query = query.bind(part);
        }
        query
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("write {} key: {error}", table.table))?;
    }
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests;
