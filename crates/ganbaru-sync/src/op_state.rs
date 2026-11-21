//! The lifecycle of a stored operation and its legal moves.
//!
//! A stored operation waits until it applies, is held when it cannot apply, or is excluded
//! when a selected removal cuts its writer off before it. `sync_ops` stores the state in the
//! columns `state`, `hold_reason`, and `excluded_revision`: an excluded operation is held as
//! invalid and names the removal revision, which a CHECK constraint enforces. Every writer of
//! those columns runs one [`OpMove`] guarded by [`OpMove::source`], so a row in any other
//! state is left alone and the writer sees it in the affected row count.

use std::fmt;

use crate::error::{SyncResult, corrupt};
use crate::store::HoldReason;

/// State of one stored operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpState {
    /// Stored and not applied yet.
    Waiting,
    /// Applied to the materialized rows.
    Applied,
    /// Not applied, for `reason`.
    Held(HoldReason),
    /// Past the cutoff of the removal selected at `revision`, so it never applies.
    Excluded {
        /// Authorization revision of the removal.
        revision: u64,
    },
}

impl OpState {
    /// Decodes the stored columns, refusing a combination no writer produces.
    pub(crate) fn from_columns(
        state: &str,
        hold_reason: Option<&str>,
        excluded_revision: Option<i64>,
    ) -> SyncResult<Self> {
        let reason = hold_reason.map(HoldReason::parse).transpose()?;
        match (state, reason, excluded_revision) {
            ("waiting", None, None) => Ok(Self::Waiting),
            ("applied", None, None) => Ok(Self::Applied),
            ("held", Some(reason), None) => Ok(Self::Held(reason)),
            ("held", Some(HoldReason::Invalid), Some(revision)) => u64::try_from(revision)
                .ok()
                .filter(|revision| *revision >= 1)
                .map(|revision| Self::Excluded { revision })
                .ok_or_else(|| corrupt("an excluded operation names an invalid revision")),
            _ => Err(corrupt(format!(
                "stored operation state {state} contradicts its hold reason or exclusion"
            ))),
        }
    }

    /// The stored `state` and `hold_reason` columns.
    pub(crate) const fn columns(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::Waiting => ("waiting", None),
            Self::Applied => ("applied", None),
            Self::Held(reason) => ("held", Some(reason.as_str())),
            Self::Excluded { .. } => ("held", Some(HoldReason::Invalid.as_str())),
        }
    }

    /// The stored `excluded_revision` column.
    pub(crate) const fn excluded_revision(self) -> Option<u64> {
        match self {
            Self::Excluded { revision } => Some(revision),
            Self::Waiting | Self::Applied | Self::Held(_) => None,
        }
    }

    /// The state a stored operation starts in: held for `hold`, or excluded at `excluded`.
    /// An excluded operation is held as invalid whatever it was classified as.
    pub(crate) const fn stored(hold: Option<HoldReason>, excluded: Option<u64>) -> Self {
        match (excluded, hold) {
            (Some(revision), _) => Self::Excluded { revision },
            (None, Some(reason)) => Self::Held(reason),
            (None, None) => Self::Waiting,
        }
    }
}

/// One change of a stored operation's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpMove {
    /// A waiting operation applied.
    Apply,
    /// A waiting operation turned out invalid when it was applied.
    HoldInvalid,
    /// A held operation not excluded was classified again, as waiting or held for the new
    /// reason. Only a hold for a newer format or manifest is classified again.
    Reclassify(Option<HoldReason>),
    /// The removal selected at `revision` excludes the operation, whatever its state. Effects
    /// of an applied operation are removed by the caller in the same transaction.
    Exclude {
        /// Authorization revision of the removal.
        revision: u64,
    },
    /// No selected removal excludes the operation any more, so it is classified again.
    Release(Option<HoldReason>),
    /// A forked writer's branch dropped back to before its genesis, which never applies. An
    /// excluded genesis stays excluded.
    DropGenesis,
}

/// A move the operation's current state does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IllegalOpMove {
    /// Current state.
    pub(crate) from: OpState,
    /// Refused move.
    pub(crate) attempted: OpMove,
}

impl fmt::Display for IllegalOpMove {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a stored operation in state {:?} cannot take the move {:?}",
            self.from, self.attempted
        )
    }
}

impl OpMove {
    /// SQL predicate over the row's state columns that holds exactly when [`transition`]
    /// allows the move. [`OpMove::Exclude`] compares against the revision bound as `?1`.
    pub(crate) const fn source(self) -> &'static str {
        match self {
            Self::Apply | Self::HoldInvalid => "state = 'waiting'",
            Self::Reclassify(_) => {
                "state = 'held' AND hold_reason IN ('newer_format', 'newer_manifest')
                    AND excluded_revision IS NULL"
            }
            Self::Exclude { .. } => "excluded_revision IS NOT ?1",
            Self::Release(_) => "excluded_revision IS NOT NULL",
            Self::DropGenesis => "excluded_revision IS NULL",
        }
    }
}

/// The state `from` moves to under `attempted`, or why it may not.
pub(crate) fn transition(from: OpState, attempted: OpMove) -> Result<OpState, IllegalOpMove> {
    let illegal = IllegalOpMove { from, attempted };
    match (from, attempted) {
        (OpState::Waiting, OpMove::Apply) => Ok(OpState::Applied),
        (OpState::Waiting, OpMove::HoldInvalid) => Ok(OpState::Held(HoldReason::Invalid)),
        (
            OpState::Held(HoldReason::NewerFormat | HoldReason::NewerManifest),
            OpMove::Reclassify(hold),
        ) => Ok(OpState::stored(hold, None)),
        (OpState::Excluded { revision: current }, OpMove::Exclude { revision })
            if current == revision =>
        {
            Err(illegal)
        }
        (_, OpMove::Exclude { revision }) => Ok(OpState::Excluded { revision }),
        (OpState::Excluded { .. }, OpMove::Release(hold)) => Ok(OpState::stored(hold, None)),
        (OpState::Waiting | OpState::Applied | OpState::Held(_), OpMove::DropGenesis) => {
            Ok(OpState::Held(HoldReason::Invalid))
        }
        _ => Err(illegal),
    }
}

/// The state a writer stores for `attempted` on an operation it read in state `from`; an
/// illegal move means the stored state is corrupt.
pub(crate) fn target(from: OpState, attempted: OpMove) -> SyncResult<OpState> {
    transition(from, attempted).map_err(|illegal| corrupt(illegal.to_string()))
}

/// Checks that a guarded single row move changed exactly one row.
pub(crate) fn expect_one(affected: u64, attempted: OpMove) -> SyncResult<()> {
    if affected == 1 {
        Ok(())
    } else {
        Err(corrupt(format!(
            "the stored operation for the move {attempted:?} is missing or in another state"
        )))
    }
}
