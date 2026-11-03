//! How a key epoch record an untrusted source such as a relay serves stands against the stored
//! authorization records.

/// How a key epoch record an untrusted source serves stands against the stored records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEpochStanding {
    /// A stored check-valid chain admits the record, so a real rotation stands behind it.
    Authentic,
    /// The record is provably not a key epoch record of this space and epoch.
    Forged,
    /// The store lacks the chain the record continues, so this replica may be lagging.
    Undecided,
}
