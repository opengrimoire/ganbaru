//! Music: table ids 48 to 95 and the artwork asset class.
//!
//! No Music table replicates yet; the domain declares its asset class so artwork overrides are
//! stored, carried, and transferred as managed assets under one engine-named path.

use crate::assets::AssetClass;
use crate::domains::music::MAX_ARTWORK_BYTES;
use crate::manifest::{DomainManifest, TableIdRange};

/// Domain name for Music.
pub const DOMAIN_NAME: &str = "music";

/// Artwork overrides of Music items, at most 10 MiB each.
pub const ARTWORK_CLASS: AssetClass = AssetClass {
    name: "music/artwork",
    max_bytes: MAX_ARTWORK_BYTES,
};

/// Music domain. Owner-only on linked replicas until its tables replicate.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 48,
        last: 95,
    },
    tables: &[],
    derived: &[],
    local: &[],
    engine: &[],
    replica_writable: false,
    assets: &[ARTWORK_CLASS],
};
