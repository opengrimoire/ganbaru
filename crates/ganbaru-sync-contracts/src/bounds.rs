//! Size limits every replica enforces when encoding and decoding operations.
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
