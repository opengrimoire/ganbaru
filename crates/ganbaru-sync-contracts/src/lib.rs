//! Sync identifiers, clocks, version vectors, order keys, writer certificates, and the signed
//! operation format.
//!
//! The crate has no database or Tauri dependency, so every replica and test decodes and verifies
//! operations with the same code. Encodings are hand-written, bounded, and big endian, with one
//! canonical form per value; golden vectors in the tests freeze them.

pub mod bounds;
pub mod certificate;
pub mod codec;
pub mod hlc;
pub mod ids;
pub mod op;
pub mod order_key;
pub mod signing;
pub mod vector;

#[cfg(test)]
mod tests;

pub use certificate::{
    CertificateError, SignedCertificate, WriterCertificate, sign_certificate, validate_device_id,
};
pub use codec::CodecError;
pub use hlc::{Hlc, HlcClock};
pub use ids::{GroupId, GroupMask, OpId, RowKey, Seq, SpaceId, TableId, WriterId};
pub use op::{
    Change, ChangeAction, Content, Envelope, Field, GroupValue, Header, Operation, OperationError,
    OperationKind, PayloadError, Revocation, RevokeReason, SealedOperation, Value,
};
pub use order_key::{OrderKey, OrderKeyError};
pub use signing::{Digest32, WriterKeyError, WriterKeyPair, WriterPublicKey};
pub use vector::VersionVector;
