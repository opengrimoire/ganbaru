//! Sync identifiers, clocks, version vectors, order keys, writer certificates, space charters,
//! authorization entries, recovery keys, and the signed operation format.
//!
//! The crate has no database or Tauri dependency, so every replica and test decodes and verifies
//! operations with the same code. Encodings are hand-written, bounded, and big endian, with one
//! canonical form per value; golden vectors in the tests freeze them.

pub mod authorization;
pub mod bounds;
pub mod certificate;
pub mod charter;
pub mod codec;
pub mod hlc;
pub mod ids;
pub mod keys;
pub mod op;
pub mod order_key;
pub mod order_placement;
pub mod recovery;
pub mod signing;
pub mod vector;

#[cfg(test)]
mod tests;

pub use authorization::{
    AuthorizationError, AuthorizationRecord, DeviceKind, DeviceRole, Enroll, Entry, Handover,
    LateResolution, LateRow, Proposal, ProposalRejection, Remove, ResolveLate, Sequencing, SetRole,
    SignedProposal, sign_proposal,
};
pub use certificate::{
    CertificateError, SignedCertificate, WriterCertificate, sign_certificate, validate_device_id,
};
pub use charter::{Charter, CharterError, SignedCharter, sign_charter};
pub use codec::CodecError;
pub use hlc::{Hlc, HlcClock};
pub use ids::{
    DerivedKeyError, GroupId, GroupMask, OpId, PENDING_KEY_PREFIX, RowKey, Seq, SpaceId, TableId,
    WriterId, derived_key,
};
pub use keys::{AdminKeyPair, AdminPublicKey, DeviceKeyPair, DevicePublicKey, KeyError};
pub use op::{
    Cause, Change, ChangeAction, Content, Envelope, Field, GroupValue, Header, Operation,
    OperationError, OperationKind, PayloadError, SealedOperation, Value, ends_group,
};
pub use order_key::{OrderKey, OrderKeyError};
pub use order_placement::{OrderPlacementError, OrderedRow};
pub use recovery::{RecoveryError, RecoverySeed};
pub use signing::{Digest32, WriterKeyError, WriterKeyPair, WriterPublicKey};
pub use vector::VersionVector;
