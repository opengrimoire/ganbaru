//! Admit aggregate inputs before grouping, without truncating canonical evidence.

use std::fmt;

use sqlx::SqliteConnection;

const HISTORY_AGGREGATE_ROWS: i64 = 100_000;
const REPLAY_AGGREGATE_ROWS: i64 = 1_000_000;

/// Why adaptive evidence could not be read.
///
/// Replicated rows can exceed a bound on any linked device, so `Oversized`
/// describes the data and callers skip the adaptive decision. `Storage` means
/// the read itself failed and the transition cannot continue.
#[derive(Debug)]
pub(crate) enum AdaptiveEvidenceError {
    Oversized(String),
    Storage(String),
}

impl fmt::Display for AdaptiveEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversized(message) | Self::Storage(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for AdaptiveEvidenceError {
    fn from(message: String) -> Self {
        Self::Storage(message)
    }
}

/// One allowance is shared by every aggregate in a history or replay read.
/// Count probes stop at one excess row and run in the caller's same snapshot.
pub(crate) struct AggregateBudget {
    remaining: i64,
}

impl AggregateBudget {
    pub(crate) fn history() -> Self {
        Self {
            remaining: HISTORY_AGGREGATE_ROWS,
        }
    }

    pub(crate) fn replay() -> Self {
        Self {
            remaining: REPLAY_AGGREGATE_ROWS,
        }
    }

    /// Source SQL and arguments belong to native code, never to IPC input.
    /// Callers use indexed filters and no ordering in these preliminary reads.
    pub(crate) async fn admit(
        &mut self,
        connection: &mut SqliteConnection,
        source_sql: &str,
        policy_id: &str,
        before: Option<&str>,
        domain: &str,
    ) -> Result<(), AdaptiveEvidenceError> {
        let query = format!("SELECT COUNT(*) FROM ({source_sql} LIMIT ?)");
        let mut probe =
            sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(query.as_str())).bind(policy_id);
        if let Some(before) = before {
            probe = probe.bind(before);
        }
        let count = probe
            .bind(self.remaining + 1)
            .fetch_one(connection)
            .await
            .map_err(|error| format!("admit adaptive {domain} aggregate: {error}"))?;
        if count > self.remaining {
            return Err(AdaptiveEvidenceError::Oversized(format!(
                "Adaptive {domain} aggregate exceeds its input work budget"
            )));
        }
        self.remaining -= count;
        Ok(())
    }
}
