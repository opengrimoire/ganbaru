//! Contacts: `contacts` (table 32), `contact_requests` (table 33), and the shared local card in
//! `contact_card` (table 34). The person identity in `contacts_local_identity` is an engine
//! table: it anchors the sync space, is written once, and never replicates.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::contacts::{
    CARD_NONCE_CHARS, DIGEST_CHARS, MAX_CARD_CHARS, MAX_CONTACT_STATE_CHARS, MAX_DIRECTION_CHARS,
    MAX_DISPLAY_NAME_CHARS, MAX_ENDPOINT_HINT_CHARS, MAX_REQUEST_ID_CHARS, MAX_REQUEST_STATE_CHARS,
    MAX_TRUST_CHARS, PUBLIC_KEY_CHARS,
};
use crate::manifest::{
    ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, DomainTrigger, GroupSpec,
    GroupStorage, KeyDerivedColumn, LocalColumn, MergeKind, TableIdRange, TableSpec, TriggerPolicy,
    group_id,
};

/// Domain name for contacts.
pub const DOMAIN_NAME: &str = "contacts";

/// Domain adapter name for contacts.
pub const ADAPTER: &str = "contacts";

/// `contacts`.
pub const CONTACTS_TABLE: TableId = TableId(32);
/// `contact_requests`.
pub const REQUESTS_TABLE: TableId = TableId(33);
/// `contact_card`.
pub const CARD_TABLE: TableId = TableId(34);

/// Prefix of a contact id before the public key.
pub const CONTACT_ID_PREFIX: &str = "contact-";

/// Contact field groups.
pub mod contact_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `display_name` and `color`.
    pub const PROFILE: GroupId = group_id(1);
    /// The standing, acceptance and block times, and both trust scopes.
    pub const STANDING: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// Existence register of the derived-key row.
    pub const EXISTS: GroupId = group_id(4);
}

/// Contact request field groups.
pub mod request_group {
    use super::{GroupId, group_id};

    /// Everything fixed when the request is stored.
    pub const CREATED: GroupId = group_id(0);
    /// `state` and the trust the request carries.
    pub const STATE: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Local card field groups.
pub mod card_group {
    use super::{GroupId, group_id};

    /// `card_nonce` and `card_revision`.
    pub const CARD: GroupId = group_id(0);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(1);
}

const fn group(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    columns: &'static [ColumnSpec],
    bumps_revision: bool,
) -> GroupSpec {
    GroupSpec {
        id,
        name,
        kind,
        storage: GroupStorage::Columns(columns),
        references: &[],
        surfaced: false,
        recoverable: false,
        bumps_revision,
    }
}

const fn text(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Text { max_chars },
    }
}

const fn timestamp(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Timestamp,
    }
}

const fn optional_timestamp(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalTimestamp,
    }
}

const DISPLAY_NAME: ColumnSpec = ColumnSpec {
    name: "display_name",
    ty: ColumnType::TrimmedText {
        min_chars: 1,
        max_chars: MAX_DISPLAY_NAME_CHARS,
    },
};

const COLOR: ColumnSpec = ColumnSpec {
    name: "color",
    ty: ColumnType::Integer { min: 0, max: 31 },
};

const REVISION: LocalColumn = LocalColumn {
    name: "revision",
    materialize: "1",
};

const CONTACT_GROUPS: [GroupSpec; 5] = [
    group(
        contact_group::CREATED,
        "created",
        MergeKind::Register,
        &[timestamp("created_at")],
        false,
    ),
    group(
        contact_group::PROFILE,
        "profile",
        MergeKind::Register,
        &[DISPLAY_NAME, COLOR],
        true,
    ),
    group(
        contact_group::STANDING,
        "standing",
        MergeKind::Coupled,
        &[
            text("state", MAX_CONTACT_STATE_CHARS),
            optional_timestamp("accepted_at"),
            optional_timestamp("blocked_at"),
            text("invite_trust", MAX_TRUST_CHARS),
            optional_timestamp("invite_trust_expires_at"),
            text("message_trust", MAX_TRUST_CHARS),
            optional_timestamp("message_trust_expires_at"),
        ],
        true,
    ),
    group(
        contact_group::UPDATED,
        "updated",
        MergeKind::Max,
        &[timestamp("updated_at")],
        false,
    ),
    group(
        contact_group::EXISTS,
        "exists",
        MergeKind::Register,
        &[],
        false,
    ),
];

/// `contacts`, keyed by the person public key, so removing a contact on one device and adding
/// the same person on another settle on one row. Removal keeps the row with the `removed`
/// standing, so a block outranks a concurrent removal and a removal outranks a concurrent
/// accept or trust widening.
pub const CONTACTS: TableSpec = TableSpec {
    id: CONTACTS_TABLE,
    name: "contacts",
    key_column: "public_key",
    groups: &CONTACT_GROUPS,
    local_columns: &[REVISION],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[KeyDerivedColumn {
        name: "id",
        prefix: CONTACT_ID_PREFIX,
    }],
    revision_column: Some("revision"),
    resolutions: &[],
    redirects: false,
    adapter: Some(ADAPTER),
    triggers: &[DomainTrigger {
        name: "contacts_revision_after_update",
        policy: TriggerPolicy::SameRow,
    }],
    derived_key: Some(DerivedKey {
        exists_group: contact_group::EXISTS,
        prefix: None,
        key_inputs: &[],
    }),
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

const REQUEST_GROUPS: [GroupSpec; 3] = [
    group(
        request_group::CREATED,
        "created",
        MergeKind::Immutable,
        &[
            text("direction", MAX_DIRECTION_CHARS),
            text("public_key", PUBLIC_KEY_CHARS),
            ColumnSpec {
                name: "peer_request_id",
                ty: ColumnType::OptionalText {
                    max_chars: MAX_REQUEST_ID_CHARS,
                },
            },
            DISPLAY_NAME,
            COLOR,
            text("card", MAX_CARD_CHARS),
            text("card_digest", DIGEST_CHARS),
            text("endpoint_hint", MAX_ENDPOINT_HINT_CHARS),
            text("coordinator_fingerprint", DIGEST_CHARS),
            timestamp("expires_at"),
            timestamp("created_at"),
        ],
        false,
    ),
    group(
        request_group::STATE,
        "state",
        MergeKind::Coupled,
        &[
            text("state", MAX_REQUEST_STATE_CHARS),
            text("invite_trust", MAX_TRUST_CHARS),
            text("message_trust", MAX_TRUST_CHARS),
        ],
        true,
    ),
    group(
        request_group::UPDATED,
        "updated",
        MergeKind::Max,
        &[timestamp("updated_at")],
        false,
    ),
];

/// `contact_requests`, keyed by a local random id. A received request also records the id the
/// requester chose, which status polls name. Delivery attempts stay on the device that made
/// them.
pub const REQUESTS: TableSpec = TableSpec {
    id: REQUESTS_TABLE,
    name: "contact_requests",
    key_column: "id",
    groups: &REQUEST_GROUPS,
    local_columns: &[
        LocalColumn {
            name: "last_attempt_at",
            materialize: "NULL",
        },
        LocalColumn {
            name: "last_error_code",
            materialize: "NULL",
        },
        REVISION,
    ],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: Some("revision"),
    resolutions: &[],
    redirects: false,
    adapter: Some(ADAPTER),
    triggers: &[DomainTrigger {
        name: "contact_requests_revision_after_update",
        policy: TriggerPolicy::SameRow,
    }],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

const CARD_GROUPS: [GroupSpec; 2] = [
    group(
        card_group::CARD,
        "card",
        MergeKind::Coupled,
        &[
            text("card_nonce", CARD_NONCE_CHARS),
            ColumnSpec {
                name: "card_revision",
                ty: ColumnType::Integer {
                    min: 1,
                    max: i64::MAX,
                },
            },
        ],
        false,
    ),
    group(
        card_group::UPDATED,
        "updated",
        MergeKind::Max,
        &[timestamp("updated_at")],
        false,
    ),
];

/// `contact_card`, the single row `card` holding the nonce every issued card carries. Any
/// device that may write contacts can rotate it; the higher revision wins while revisions stay
/// below 255, and past that concurrent rotations fall back to the clock.
pub const CARD: TableSpec = TableSpec {
    id: CARD_TABLE,
    name: "contact_card",
    key_column: "key",
    groups: &CARD_GROUPS,
    local_columns: &[],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: Some(ADAPTER),
    triggers: &[DomainTrigger {
        name: "contact_card_prevent_delete",
        policy: TriggerPolicy::ApplierVisible,
    }],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

/// The contacts domain, table ids 32 to 47.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 32,
        last: 47,
    },
    tables: &[CONTACTS, REQUESTS, CARD],
    derived: &[],
    local: &[],
    engine: &[DeviceTable {
        name: "contacts_local_identity",
        triggers: &[
            DomainTrigger {
                name: "contacts_local_identity_immutable_update",
                policy: TriggerPolicy::ApplierVisible,
            },
            DomainTrigger {
                name: "contacts_local_identity_immutable_delete",
                policy: TriggerPolicy::ApplierVisible,
            },
        ],
        assets: &[],
    }],
    replica_writable: true,
    unconverted: &[],
    assets: &[],
};
