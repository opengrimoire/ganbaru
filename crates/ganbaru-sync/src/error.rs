//! Engine errors.

use std::fmt;

use ganbaru_sync_contracts::{OperationError, WriterId};

/// An engine failure. Store refusals and held operations are outcomes, not errors.
#[derive(Debug)]
pub enum SyncError {
    /// The database failed.
    Database(sqlx::Error),
    /// Engine state breaks an invariant the engine maintains.
    Corrupt(String),
    /// The manifest is malformed or does not match the registered adapters.
    InvalidManifest(String),
    /// A space has no row in `sync_spaces`.
    UnknownSpace,
    /// The local writer cannot seal: its certificate does not match its key or the enrolled
    /// device key.
    WriterMismatch,
    /// The local writer is no longer active in this space.
    WriterInactive(WriterId),
    /// The sequence reservation could not be made durable.
    Reservation(std::io::Error),
    /// An operation could not be sealed.
    Seal(OperationError),
    /// A re-seal request does not match the stored chain.
    InvalidReseal(&'static str),
    /// The space holds sync state this build does not read; resetting sync state recovers it.
    ResetRequired(&'static str),
    /// A charter does not verify with the space person key, names another space, or differs
    /// from the installed one.
    InvalidCharter,
    /// The selected authorization chain removed the local device.
    DeviceRemoved,
    /// The selected authorization chain has not enrolled the local device yet, so its writer
    /// cannot seal a genesis.
    NotEnrolled,
    /// The authorization record store crossed its storage backstop: record exchange stores only
    /// kit and contest records and their ancestors, and the operation lanes stop, until the user
    /// frees space, raises the bound, or re-founds the space.
    AuthorizationStorageFull,
    /// A request names a row, group, or table the engine cannot act on.
    InvalidRequest(&'static str),
    /// A replicated row of the named table still holds a `pending:` placeholder key, which a
    /// domain must replace with its final key before the transaction that inserted it commits.
    PendingKey(&'static str),
    /// A peer lacks operations of `writer` whose bodies compaction dropped, up to `seq`, so it
    /// must bootstrap from a snapshot.
    Compacted {
        /// The writer whose operations were compacted.
        writer: WriterId,
        /// Highest compacted sequence of the writer.
        seq: u64,
    },
    /// A materialize hook of the named table failed.
    Hook {
        /// Table of the hook.
        table: &'static str,
        /// What the hook reported.
        error: crate::hooks::HookError,
    },
    /// A test failpoint aborted the transaction.
    #[cfg(test)]
    Failpoint(&'static str),
}

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "sync database error: {error}"),
            Self::Corrupt(detail) => write!(formatter, "sync state is inconsistent: {detail}"),
            Self::InvalidManifest(detail) => {
                write!(formatter, "sync manifest is invalid: {detail}")
            }
            Self::UnknownSpace => formatter.write_str("sync space is not initialized"),
            Self::WriterMismatch => {
                formatter.write_str("writer certificate does not match the key or space")
            }
            Self::WriterInactive(writer) => {
                write!(formatter, "writer {} is not active", writer.to_hex())
            }
            Self::Reservation(error) => {
                write!(formatter, "sequence reservation failed: {error}")
            }
            Self::Seal(error) => write!(formatter, "operation could not be sealed: {error}"),
            Self::InvalidReseal(detail) => write!(formatter, "invalid re-seal: {detail}"),
            Self::ResetRequired(detail) => {
                write!(formatter, "reset sync state: {detail}")
            }
            Self::InvalidCharter => formatter.write_str("sync charter is invalid for this space"),
            Self::DeviceRemoved => formatter.write_str("this device was removed from the space"),
            Self::NotEnrolled => {
                formatter.write_str("this device is not enrolled in the space yet")
            }
            Self::AuthorizationStorageFull => formatter.write_str(
                "authorization storage is full; sync stores only recovery and contest records",
            ),
            Self::InvalidRequest(detail) => write!(formatter, "invalid sync request: {detail}"),
            Self::PendingKey(table) => write!(
                formatter,
                "a replicated row of {table} still holds a pending placeholder key"
            ),
            Self::Compacted { writer, seq } => write!(
                formatter,
                "operations of writer {} through {seq} were compacted",
                writer.to_hex()
            ),
            Self::Hook { table, error } => {
                write!(formatter, "materialize hook of {table}: {error}")
            }
            #[cfg(test)]
            Self::Failpoint(name) => write!(formatter, "failpoint {name}"),
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Reservation(error) => Some(error),
            Self::Seal(error) => Some(error),
            Self::Hook { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for SyncError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Builds a [`SyncError::Corrupt`].
pub(crate) fn corrupt(detail: impl Into<String>) -> SyncError {
    SyncError::Corrupt(detail.into())
}

/// Result alias for engine operations.
pub type SyncResult<T> = Result<T, SyncError>;

/// Runs a failpoint check in test builds and nothing otherwise.
macro_rules! failpoint {
    ($name:literal) => {
        #[cfg(test)]
        $crate::error::failpoints::check($name)?;
    };
}

pub(crate) use failpoint;

/// Test failpoints that abort a transaction before it commits.
#[cfg(test)]
pub(crate) mod failpoints {
    use std::cell::RefCell;

    thread_local! {
        static ARMED: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    /// Arms a failpoint once on this thread.
    pub(crate) fn arm(name: &'static str) {
        ARMED.with(|armed| armed.borrow_mut().push(name));
    }

    /// Disarms every failpoint on this thread.
    pub(crate) fn clear() {
        ARMED.with(|armed| armed.borrow_mut().clear());
    }

    /// Fails once when the failpoint is armed.
    pub(crate) fn check(name: &'static str) -> super::SyncResult<()> {
        let hit = ARMED.with(|armed| {
            let mut armed = armed.borrow_mut();
            match armed.iter().position(|candidate| *candidate == name) {
                Some(index) => {
                    armed.remove(index);
                    true
                }
                None => false,
            }
        });
        if hit {
            Err(super::SyncError::Failpoint(name))
        } else {
            Ok(())
        }
    }
}
