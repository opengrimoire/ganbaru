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
    /// The manifest names an adapter that is not registered.
    UnregisteredAdapter(&'static str),
    /// A space has no row in `sync_spaces`.
    UnknownSpace,
    /// The local writer cannot seal: its certificate does not match its key or the space anchor.
    WriterMismatch,
    /// The local writer is no longer active in this space.
    WriterInactive(WriterId),
    /// Sealing would need more dependency entries than an operation may carry.
    TooManyWriters,
    /// The sequence reservation could not be made durable.
    Reservation(std::io::Error),
    /// An operation could not be sealed.
    Seal(OperationError),
    /// A re-seal request does not match the stored chain.
    InvalidReseal(&'static str),
    /// A revocation request does not match the stored chain.
    InvalidRevoke(&'static str),
    /// A request names a row, group, or table the engine cannot act on.
    InvalidRequest(&'static str),
    /// A test failpoint aborted the transaction.
    #[cfg(test)]
    Failpoint(&'static str),
}

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "sync database error: {error}"),
            Self::Corrupt(detail) => write!(formatter, "sync state is inconsistent: {detail}"),
            Self::UnregisteredAdapter(name) => {
                write!(formatter, "sync adapter {name} is not registered")
            }
            Self::UnknownSpace => formatter.write_str("sync space is not initialized"),
            Self::WriterMismatch => {
                formatter.write_str("writer certificate does not match the key or space")
            }
            Self::WriterInactive(writer) => {
                write!(formatter, "writer {} is not active", writer.to_hex())
            }
            Self::TooManyWriters => formatter.write_str("too many writers for one operation"),
            Self::Reservation(error) => {
                write!(formatter, "sequence reservation failed: {error}")
            }
            Self::Seal(error) => write!(formatter, "operation could not be sealed: {error}"),
            Self::InvalidReseal(detail) => write!(formatter, "invalid re-seal: {detail}"),
            Self::InvalidRevoke(detail) => write!(formatter, "invalid revocation: {detail}"),
            Self::InvalidRequest(detail) => write!(formatter, "invalid sync request: {detail}"),
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
