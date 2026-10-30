//! Size limits every replica enforces when encoding and decoding operations, certificates, and
//! authorization entries.
//!
//! The limits derive from the 1 MiB transport control frame. Tightening one changes which
//! operations are valid, so it requires a manifest version bump that keeps the old rule for
//! operations sealed under earlier versions.

/// Largest encoded operation, about 683 KiB as base64, inside one control frame with headroom.
pub const MAX_OPERATION_BYTES: usize = 512 * 1024;
/// Largest encoded header. Headers outlive compacted bodies, so sealers split batches on this
/// bound as well as on the change count and the operation size; it holds about 200 full Quick
/// note creates.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Changes per operation, which bounds apply transactions.
pub const MAX_CHANGES_PER_OPERATION: usize = 1_024;
/// Dependency entries per operation header.
pub const MAX_DEPENDENCIES: usize = 256;
/// Largest row key in bytes, matching the existing domain id validation.
pub const MAX_ROW_KEY_BYTES: usize = 128;
/// Largest text field in bytes: a 65,536-character body is at most 256 KiB of UTF-8.
pub const MAX_TEXT_FIELD_BYTES: usize = 256 * 1024;
/// Largest blob field in bytes: the largest text plus composite structure overhead.
pub const MAX_BLOB_FIELD_BYTES: usize = 320 * 1024;
/// Fields per group value, which keeps coupled groups small.
pub const MAX_FIELDS_PER_VALUE: usize = 16;
/// Number of field groups a table may declare; group ids are `0..MAX_GROUPS`.
pub const MAX_GROUPS: u8 = 64;
/// Largest device identifier in a writer certificate, matching the handoff identifier rule.
pub const MAX_DEVICE_ID_BYTES: usize = 160;
/// Largest order key in bytes.
pub const MAX_ORDER_KEY_BYTES: usize = 128;
/// Writers of one space, every state included: the entries a version vector carries on the
/// wire. Stores refuse geneses past it, so vector exchange keeps working.
pub const MAX_WRITERS_PER_SPACE: usize = 4_096;
/// Writers one device may certify in a space over its lifetime. An installation mints a
/// successor only after a restore, a copied database, a lost key, or a clone, so this is far
/// above honest use while it keeps one linked device from flooding the space.
pub const MAX_WRITERS_PER_DEVICE: usize = 64;
/// Largest device label in an enrollment, matching the handoff device label rule.
pub const MAX_DEVICE_LABEL_BYTES: usize = 128;
/// Length of a TLS certificate fingerprint: the SHA-256 of the DER certificate.
pub const TLS_FINGERPRINT_BYTES: usize = 32;
/// Length of a vault discovery key.
pub const DISCOVERY_KEY_BYTES: usize = 32;
/// Largest writer certificate encoding: the longest device id and a predecessor.
pub const MAX_CERTIFICATE_BYTES: usize =
    1 + 32 + (1 + MAX_DEVICE_ID_BYTES) + 32 + 8 + (1 + 16) + 64;
/// Largest space charter encoding: the longest anchor device id.
pub const MAX_CHARTER_BYTES: usize = 1
    + 16
    + (1 + MAX_DEVICE_ID_BYTES)
    + 32
    + TLS_FINGERPRINT_BYTES
    + 32
    + 32
    + DISCOVERY_KEY_BYTES
    + 8
    + 64;
/// Cutoffs one removal lists: one per writer the removed device certified.
pub const MAX_REMOVAL_CUTOFFS: usize = MAX_WRITERS_PER_DEVICE;
/// Rows one late change resolution decides.
pub const MAX_LATE_RESOLUTIONS: usize = MAX_CHANGES_PER_OPERATION;
/// Most operations one apply group spans. A group is a run of one writer's operations that a
/// receiver applies in one transaction, see [`crate::op::ends_group`]. [`MAX_GROUP_BYTES`]
/// bounds the changes of a group; this bound caps the per-operation work, such as decoding and
/// readiness checks, that a writer marking every operation as continuing adds to one transaction.
/// Honest groups stay far below it: filled operations reach hundreds of KiB, so the byte bound
/// ends a group of them after a few operations.
pub const MAX_OPERATIONS_PER_GROUP: usize = 64;
/// Envelope bytes at which an apply group ends: the operation that reaches them closes it
/// whatever its header says, so a group holds at most this plus one operation. The densest group
/// a writer can force, small Music updates each marked as continuing, holds about 15,000 changes
/// and applies in about 7 seconds on a release build.
pub const MAX_GROUP_BYTES: usize = 1 << 20;
/// Most operations of one chain: a run of a writer's operations that each mark the next as
/// continuing, which a local edit too large for one group seals as. A receiver applies no
/// group of a chain until the whole chain is stored and closed, then applies it in bounded
/// groups over several transactions. Between two of its groups, a receiver that can seal defers
/// every repair, so a state between two groups of one edit never becomes a repair; after its
/// last group, the rows it touched are taken back and repaired a bounded number per
/// transaction, together with the rows that reference them. A receiver that can seal starts one
/// chain at a time, and only once the rows the previous one deferred were taken back, so every
/// deferral ends. Of the chains that could start, the one whose last operation the receiver
/// stored first starts, whatever the clocks say, so a writer cannot keep another writer's chain
/// waiting by sending chains with low clocks. A chain closes at an operation that does not
/// continue, before its writer's cutoff, an excluded operation, or an invalid one, and at this
/// many operations. A chain that never closes applies nothing and holds back only its writer's
/// later operations. A dense Music library refresh of 6,000 items spans 9 operations and one of
/// the longest metadata 19.
pub const MAX_OPERATIONS_PER_CHAIN: usize = 256;
/// Envelopes one key epoch entry carries: one per live device key plus the recovery key. Every
/// device certifies at least one writer, so the writer bound caps the devices.
pub const MAX_KEY_RECIPIENTS: usize = MAX_WRITERS_PER_SPACE + 1;
/// Endpoints one server configuration lists.
pub const MAX_SERVER_ENDPOINTS: usize = 4;
/// Largest server endpoint, `host:port`, in bytes.
pub const MAX_SERVER_ENDPOINT_BYTES: usize = 255;
