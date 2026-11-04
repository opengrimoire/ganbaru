//! Rows a domain constraint refused while materializing.
//!
//! Operation bytes and merge state decide almost every invalid row the same way on every replica.
//! What remains is a domain constraint that refuses a row only once it is written, such as a
//! constraint a newer build added or one that reads other rows. Apply must not stop for it, so the
//! row alone is refused and listed here, and every operation stays applied. The list is local
//! state: nothing replicated changes on a refusal, and duplicate keepers, detached trees, severed
//! edges, and repairs never read it, see [`crate::visibility::Visibility::admit_refusals`].
//!
//! A refused write hides its row and the rows that cascade from it, until a retry writes it. A
//! refused delete keeps the domain row as it is and hides nothing. Rows the engine write touches
//! always retry, and so do the [`MAX_REFUSED_RETRIES_PER_FINISH`] rows retried least recently, so
//! refused rows never make work for themselves and the valid write that fixes a row brings it
//! back whatever the size of the list.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::TableId;
use sqlx::SqliteConnection;

use crate::error::{SyncResult, corrupt};
use crate::merge::table_param;
use crate::seal::{InvalidRow, SealReport};
use crate::validate;
use crate::visibility::{RowRef, Visibility};

/// Refused rows retried per finish beyond the rows dirty in that finish.
pub(crate) const MAX_REFUSED_RETRIES_PER_FINISH: usize = 16;

/// What a refusal leaves in the domain table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusalKind {
    /// The winners do not fit, so the row is absent and hides the rows that cascade from it.
    Hidden,
    /// The row could not be deleted, so it stays as it is and hides nothing.
    Kept,
}

impl RefusalKind {
    const fn stored(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Kept => "kept",
        }
    }
}

/// A row a domain constraint refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) row: RowRef,
    pub(crate) kind: RefusalKind,
    pub(crate) reason: validate::Invalid,
}

/// Whether a row is listed, and with which stored kind.
async fn listed_kind(
    conn: &mut SqliteConnection,
    (table, key): &RowRef,
) -> SyncResult<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT kind FROM sync_refused_rows WHERE table_id = ? AND row_key = ?")
            .bind(table_param(*table))
            .bind(key.as_str())
            .fetch_optional(&mut *conn)
            .await?,
    )
}

/// Whether a row is listed as hidden.
pub(crate) async fn is_hidden(conn: &mut SqliteConnection, row: &RowRef) -> SyncResult<bool> {
    Ok(listed_kind(conn, row).await?.as_deref() == Some(RefusalKind::Hidden.stored()))
}

/// Whether a row's refusal explains how its domain table differs from what should show: listed
/// as hidden while it should show, or as kept while it should be absent. Such a row waits for
/// its retry instead of being planned again.
pub(crate) async fn explains(
    conn: &mut SqliteConnection,
    row: &RowRef,
    shown: bool,
) -> SyncResult<bool> {
    let kind = if shown {
        RefusalKind::Hidden
    } else {
        RefusalKind::Kept
    };
    Ok(listed_kind(conn, row).await?.as_deref() == Some(kind.stored()))
}

/// Whether any row is listed.
pub(crate) async fn any(conn: &mut SqliteConnection) -> SyncResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sync_refused_rows)")
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// The refusal state of one finish.
#[derive(Debug)]
pub(crate) struct Refusals {
    /// Attempt written for every row retried or refused in this finish.
    attempt: i64,
    /// The rows retried least recently, picked once per finish.
    oldest: BTreeSet<RowRef>,
    /// Rows refused in this finish, which it does not retry.
    refused: BTreeSet<RowRef>,
    /// Rows whose delete was refused in this finish, which it leaves unwritten.
    kept: BTreeSet<RowRef>,
}

impl Refusals {
    pub(crate) async fn load(conn: &mut SqliteConnection) -> SyncResult<Self> {
        let attempt: i64 =
            sqlx::query_scalar("SELECT coalesce(max(attempt), 0) + 1 FROM sync_refused_rows")
                .fetch_one(&mut *conn)
                .await?;
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT table_id, row_key FROM sync_refused_rows
             ORDER BY attempt, table_id, row_key LIMIT ?",
        )
        .bind(i64::try_from(MAX_REFUSED_RETRIES_PER_FINISH).unwrap_or(i64::MAX))
        .fetch_all(&mut *conn)
        .await?;
        let oldest = rows
            .into_iter()
            .map(|(table, key)| {
                let table = u16::try_from(table)
                    .map(TableId)
                    .map_err(|_| corrupt("refused row of a table id out of range"))?;
                Ok((table, key))
            })
            .collect::<SyncResult<_>>()?;
        Ok(Self {
            attempt,
            oldest,
            refused: BTreeSet::new(),
            kept: BTreeSet::new(),
        })
    }

    /// Rows this turn of the finish retries: the listed rows among `dirty` and the oldest rows,
    /// leaving out rows the finish already refused. Their attempt moves them to the back of the
    /// retry order.
    pub(crate) async fn retrying(
        &self,
        conn: &mut SqliteConnection,
        dirty: &BTreeSet<RowRef>,
    ) -> SyncResult<BTreeSet<RowRef>> {
        let mut retrying = self.oldest.clone();
        if self.oldest.is_empty() && self.refused.is_empty() {
            return Ok(retrying);
        }
        for row in dirty {
            if listed_kind(conn, row).await?.is_some() {
                retrying.insert(row.clone());
            }
        }
        retrying.retain(|row| !self.refused.contains(row));
        for (table, key) in &retrying {
            sqlx::query(
                "UPDATE sync_refused_rows SET attempt = ? WHERE table_id = ? AND row_key = ?",
            )
            .bind(self.attempt)
            .bind(table_param(*table))
            .bind(key.as_str())
            .execute(&mut *conn)
            .await?;
        }
        Ok(retrying)
    }

    /// Rows refused in this finish, which every later turn plans again: a row a turn refused
    /// must go absent, along with the rows that reference it.
    pub(crate) fn refused(&self) -> &BTreeSet<RowRef> {
        &self.refused
    }

    /// Tables of the rows this finish plans without marking them dirty.
    pub(crate) fn tables(&self) -> BTreeSet<TableId> {
        self.oldest
            .iter()
            .chain(&self.refused)
            .map(|(table, _)| *table)
            .collect()
    }

    /// Rows this finish leaves unwritten because their delete was refused.
    pub(crate) fn kept(&self) -> &BTreeSet<RowRef> {
        &self.kept
    }

    /// Lists `refusals` and reports the rows that were not listed yet, so a row refused again on
    /// each retry is reported once.
    pub(crate) async fn record(
        &mut self,
        conn: &mut SqliteConnection,
        refusals: Vec<Refusal>,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        for Refusal { row, kind, reason } in refusals {
            let listed = listed_kind(conn, &row).await?.is_some();
            sqlx::query(
                "INSERT INTO sync_refused_rows (table_id, row_key, kind, reason, attempt)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT (table_id, row_key) DO UPDATE
                 SET kind = excluded.kind, reason = excluded.reason, attempt = excluded.attempt",
            )
            .bind(table_param(row.0))
            .bind(row.1.as_str())
            .bind(kind.stored())
            .bind(reason)
            .bind(self.attempt)
            .execute(&mut *conn)
            .await?;
            if !listed {
                report.invalid_rows.push(InvalidRow {
                    table: row.0,
                    row_key: row.1.clone(),
                    reason,
                });
            }
            if kind == RefusalKind::Kept {
                self.kept.insert(row.clone());
            }
            self.refused.insert(row);
        }
        Ok(())
    }

    /// Stops listing the rows of `planned` that the finish wrote, and returns them. Rows a
    /// materialization planned and still hides as refused stay listed.
    pub(crate) async fn clear(
        &self,
        conn: &mut SqliteConnection,
        visibility: &mut Visibility,
        planned: Vec<RowRef>,
    ) -> SyncResult<Vec<RowRef>> {
        let mut cleared = Vec::new();
        if self.oldest.is_empty() && self.refused.is_empty() {
            return Ok(cleared);
        }
        for row in planned {
            if self.refused.contains(&row) || visibility.is_refused(conn, &row).await? {
                continue;
            }
            let removed =
                sqlx::query("DELETE FROM sync_refused_rows WHERE table_id = ? AND row_key = ?")
                    .bind(table_param(row.0))
                    .bind(row.1.as_str())
                    .execute(&mut *conn)
                    .await?
                    .rows_affected();
            if removed > 0 {
                cleared.push(row);
            }
        }
        Ok(cleared)
    }
}
