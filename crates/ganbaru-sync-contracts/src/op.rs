//! The signed operation format, version 1.
//!
//! Envelope, big endian and stable across format versions: magic `GBSO` (4) | format version (1)
//! | kind (1) | space id (16) | writer id (16) | sequence (8) | previous operation hash (32) |
//! header length (4) and header | body length (4) and body | Ed25519 signature (64).
//!
//! The signature covers the operation signature domain and every envelope byte before the body
//! length, and the operation hash covers the same bytes under the hash domain. The body is
//! excluded so a later compaction can drop superseded values; each value stays bound to the
//! signature through its hash in the header.
//!
//! Header: clock (8) | manifest version (4) | authorization revision (8) | key epoch (4) |
//! dependency count (2) and `(writer id, sequence)` pairs | kind payload. A changes payload is a
//! change count (2) and, per change, table id (2) | row key length (1) and key | action (1) |
//! group mask (8) | one value hash (32) per set group in ascending order | for tombstones a
//! redirect flag (1) and optional row key. A genesis payload is a certificate length (2) and the
//! certificate. A revoke payload is target writer (16) | cutoff sequence (8) | reason (1).
//!
//! Body: per change and set group, a presence byte (1) and the value: field count (1) and typed
//! fields, tag `0` null, `1` i64 (8), `2` text with length (4), `3` blob with length (4). Format
//! version 1 requires every value to be present.

use crate::bounds::{
    MAX_BLOB_FIELD_BYTES, MAX_CHANGES_PER_OPERATION, MAX_FIELDS_PER_VALUE, MAX_HEADER_BYTES,
    MAX_OPERATION_BYTES, MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES,
};
use crate::certificate::{CertificateError, SignedCertificate};
use crate::codec::{CodecError, Reader, Writer};
use crate::hlc::Hlc;
use crate::ids::{GroupId, GroupMask, ID_BYTES, OpId, RowKey, Seq, SpaceId, TableId, WriterId};
use crate::signing::{
    DIGEST_BYTES, Digest32, OPERATION_HASH_DOMAIN, VALUE_HASH_DOMAIN, WRITER_SIGNATURE_BYTES,
    WriterKeyPair, WriterPublicKey, domain_digest,
};
use crate::vector::validate_dependencies;
use std::collections::BTreeSet;
use std::fmt;

/// Leading bytes of every operation.
pub const OPERATION_MAGIC: &[u8; 4] = b"GBSO";
/// Operation format this build encodes and fully decodes.
pub const FORMAT_VERSION: u8 = 1;
/// Previous hash of a writer genesis.
pub const GENESIS_PREVIOUS_HASH: Digest32 = [0u8; DIGEST_BYTES];

const PRESENT: u8 = 1;
const FIELD_NULL: u8 = 0;
const FIELD_INTEGER: u8 = 1;
const FIELD_TEXT: u8 = 2;
const FIELD_BLOB: u8 = 3;

/// What an operation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    /// Row changes.
    Changes,
    /// The first operation of a writer, carrying its certificate.
    Genesis,
    /// A writer revocation with a cutoff sequence.
    Revoke,
}

impl OperationKind {
    /// Wire value. `3` is reserved for checkpoints.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Changes => 0,
            Self::Genesis => 1,
            Self::Revoke => 2,
        }
    }

    /// Kind for a wire value known to this format.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Changes),
            1 => Some(Self::Genesis),
            2 => Some(Self::Revoke),
            _ => None,
        }
    }
}

/// What a change does to its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeAction {
    /// Creates the row with every replicated group.
    Create,
    /// Writes some groups of an existing row.
    Write,
    /// Deletes the row.
    Tombstone,
}

impl ChangeAction {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Create => 0,
            Self::Write => 1,
            Self::Tombstone => 2,
        }
    }

    const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Create),
            1 => Some(Self::Write),
            2 => Some(Self::Tombstone),
            _ => None,
        }
    }
}

/// One typed field of a group value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Field {
    /// SQL null.
    Null,
    /// A 64-bit integer.
    Integer(i64),
    /// UTF-8 text.
    Text(String),
    /// Bytes, such as a composite value encoded by a domain adapter.
    Blob(Vec<u8>),
}

/// The value of one field group: 1 to 16 fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Value(Vec<Field>);

impl Value {
    /// Validates field count and field sizes.
    pub fn new(fields: Vec<Field>) -> Result<Self, CodecError> {
        if fields.is_empty() || fields.len() > MAX_FIELDS_PER_VALUE {
            return Err(CodecError::Bounds("value fields"));
        }
        for field in &fields {
            match field {
                Field::Text(text) if text.len() > MAX_TEXT_FIELD_BYTES => {
                    return Err(CodecError::Bounds("text field"));
                }
                Field::Blob(blob) if blob.len() > MAX_BLOB_FIELD_BYTES => {
                    return Err(CodecError::Bounds("blob field"));
                }
                _ => {}
            }
        }
        Ok(Self(fields))
    }

    /// Fields in declaration order.
    pub fn fields(&self) -> &[Field] {
        &self.0
    }

    /// Consumes the value into its fields.
    pub fn into_fields(self) -> Vec<Field> {
        self.0
    }

    /// Appends the canonical encoding.
    pub fn encode_into(&self, writer: &mut Writer) {
        writer.u8(self.0.len() as u8);
        for field in &self.0 {
            match field {
                Field::Null => writer.u8(FIELD_NULL),
                Field::Integer(value) => {
                    writer.u8(FIELD_INTEGER);
                    writer.i64(*value);
                }
                Field::Text(text) => {
                    writer.u8(FIELD_TEXT);
                    writer.bytes_u32(text.as_bytes());
                }
                Field::Blob(blob) => {
                    writer.u8(FIELD_BLOB);
                    writer.bytes_u32(blob);
                }
            }
        }
    }

    /// Canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        self.encode_into(&mut writer);
        writer.into_bytes()
    }

    /// Reads one value.
    pub fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        let count = usize::from(reader.u8()?);
        if count == 0 || count > MAX_FIELDS_PER_VALUE {
            return Err(CodecError::Bounds("value fields"));
        }
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            let field = match reader.u8()? {
                FIELD_NULL => Field::Null,
                FIELD_INTEGER => Field::Integer(reader.i64()?),
                FIELD_TEXT => {
                    let bytes = reader.bytes_u32(MAX_TEXT_FIELD_BYTES, "text field")?;
                    let text = std::str::from_utf8(bytes)
                        .map_err(|_| CodecError::Malformed("text field"))?;
                    Field::Text(text.to_string())
                }
                FIELD_BLOB => Field::Blob(
                    reader
                        .bytes_u32(MAX_BLOB_FIELD_BYTES, "blob field")?
                        .to_vec(),
                ),
                _ => return Err(CodecError::Malformed("field tag")),
            };
            fields.push(field);
        }
        Ok(Self(fields))
    }

    /// Decodes a complete value encoding.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut reader = Reader::new(bytes);
        let value = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }

    /// Hash of this value as group `group` of table `table`.
    pub fn hash(&self, table: TableId, group: GroupId) -> Digest32 {
        value_hash(table, group, &self.encode())
    }
}

/// Hash of an encoded value as group `group` of table `table`.
pub fn value_hash(table: TableId, group: GroupId, encoded_value: &[u8]) -> Digest32 {
    domain_digest(
        VALUE_HASH_DOMAIN,
        &[&table.0.to_be_bytes(), &[group.get()], encoded_value],
    )
}

/// One group written by a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupValue {
    /// The group.
    pub group: GroupId,
    /// Value hash, bound to the signature through the header.
    pub hash: Digest32,
    /// The value.
    pub value: Value,
}

impl GroupValue {
    /// Pairs a value with its hash.
    pub fn new(table: TableId, group: GroupId, value: Value) -> Self {
        Self {
            group,
            hash: value.hash(table, group),
            value,
        }
    }
}

/// One row change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Replicated table.
    pub table: TableId,
    /// Row primary key.
    pub row: RowKey,
    /// What happens to the row.
    pub action: ChangeAction,
    /// Written groups in ascending order; empty for a tombstone.
    pub groups: Vec<GroupValue>,
    /// For a tombstone, the row that replaces this one; references follow it.
    pub replaced_by: Option<RowKey>,
}

impl Change {
    /// Groups written by the change.
    pub fn mask(&self) -> GroupMask {
        let mut mask = GroupMask::EMPTY;
        for group in &self.groups {
            mask.insert(group.group);
        }
        mask
    }

    fn validate(&self) -> Result<(), CodecError> {
        let ascending = self
            .groups
            .windows(2)
            .all(|pair| pair[0].group < pair[1].group);
        if !ascending {
            return Err(CodecError::Malformed("change groups"));
        }
        match self.action {
            ChangeAction::Create | ChangeAction::Write => {
                if self.groups.is_empty() || self.replaced_by.is_some() {
                    return Err(CodecError::Malformed("change"));
                }
            }
            ChangeAction::Tombstone => {
                if !self.groups.is_empty() || self.replaced_by.as_ref() == Some(&self.row) {
                    return Err(CodecError::Malformed("tombstone"));
                }
            }
        }
        for group in &self.groups {
            if group.value.hash(self.table, group.group) != group.hash {
                return Err(CodecError::Malformed("value hash"));
            }
        }
        Ok(())
    }
}

/// Why a writer was revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RevokeReason {
    /// The device was unlinked.
    Revoked,
    /// The writer's chain forked, for example after a disk clone.
    Forked,
}

impl RevokeReason {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Revoked => 0,
            Self::Forked => 1,
        }
    }

    const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Revoked),
            1 => Some(Self::Forked),
            _ => None,
        }
    }
}

/// A writer revocation: operations of `writer` above `cutoff` are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revocation {
    /// The revoked writer.
    pub writer: WriterId,
    /// Highest sequence that stays valid.
    pub cutoff: Seq,
    /// Why it was revoked.
    pub reason: RevokeReason,
}

/// Header fields shared by every kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Hybrid logical clock at seal time.
    pub clock: Hlc,
    /// Manifest version the changes were sealed under.
    pub manifest_version: u32,
    /// Authorization revision; zero until multi-person spaces.
    pub authorization_revision: u64,
    /// Key epoch; zero until operation encryption.
    pub key_epoch: u32,
    /// Writers whose applied sequence advanced since this writer's previous operation, ascending
    /// by writer.
    pub dependencies: Vec<(WriterId, Seq)>,
}

/// Kind-specific contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Row changes, at most one per row.
    Changes(Vec<Change>),
    /// The writer certificate.
    Genesis(SignedCertificate),
    /// A writer revocation.
    Revoke(Revocation),
}

/// A logical operation before signing or after verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// Space the operation belongs to.
    pub space: SpaceId,
    /// Writer that sealed it.
    pub writer: WriterId,
    /// Position in the writer's chain, from 1.
    pub seq: Seq,
    /// Hash of the writer's previous operation, zero for the genesis.
    pub previous_hash: Digest32,
    /// Shared header fields.
    pub header: Header,
    /// Kind-specific contents.
    pub content: Content,
}

/// An operation that cannot be encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationError {
    /// A structural rule or bound is violated.
    Codec(CodecError),
    /// The genesis certificate is invalid.
    Certificate(CertificateError),
    /// A key or certificate names a different writer than the operation.
    WriterMismatch,
    /// The envelope signature does not verify.
    Signature,
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(formatter, "operation: {error}"),
            Self::Certificate(error) => write!(formatter, "operation: {error}"),
            Self::WriterMismatch => formatter.write_str("operation writer mismatch"),
            Self::Signature => formatter.write_str("operation signature does not verify"),
        }
    }
}

impl std::error::Error for OperationError {}

impl From<CodecError> for OperationError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<CertificateError> for OperationError {
    fn from(error: CertificateError) -> Self {
        Self::Certificate(error)
    }
}

impl Operation {
    /// Kind of the contents.
    pub fn kind(&self) -> OperationKind {
        match self.content {
            Content::Changes(_) => OperationKind::Changes,
            Content::Genesis(_) => OperationKind::Genesis,
            Content::Revoke(_) => OperationKind::Revoke,
        }
    }

    /// Operation identity.
    pub fn id(&self) -> OpId {
        OpId {
            space: self.space,
            writer: self.writer,
            seq: self.seq,
        }
    }

    /// Checks every rule of the format that does not depend on the manifest.
    pub fn validate(&self) -> Result<(), OperationError> {
        let genesis = self.kind() == OperationKind::Genesis;
        if self.seq == 0 || genesis != (self.seq == 1) {
            return Err(CodecError::Malformed("sequence").into());
        }
        if (self.previous_hash == GENESIS_PREVIOUS_HASH) != genesis {
            return Err(CodecError::Malformed("previous hash").into());
        }
        validate_dependencies(&self.header.dependencies, &self.writer)?;
        match &self.content {
            Content::Changes(changes) => {
                if changes.is_empty() || changes.len() > MAX_CHANGES_PER_OPERATION {
                    return Err(CodecError::Bounds("changes").into());
                }
                let mut rows = BTreeSet::new();
                for change in changes {
                    if !rows.insert((change.table, &change.row)) {
                        return Err(CodecError::Malformed("duplicate row change").into());
                    }
                    change.validate()?;
                }
            }
            Content::Genesis(certificate) => {
                if certificate.certificate.writer_id() != self.writer {
                    return Err(OperationError::WriterMismatch);
                }
            }
            Content::Revoke(revocation) => {
                if revocation.writer == self.writer {
                    return Err(CodecError::Malformed("revocation target").into());
                }
            }
        }
        Ok(())
    }

    fn encode_header(&self) -> Writer {
        let mut writer = Writer::with_capacity(256);
        writer.u64(self.header.clock.as_u64());
        writer.u32(self.header.manifest_version);
        writer.u64(self.header.authorization_revision);
        writer.u32(self.header.key_epoch);
        writer.u16(self.header.dependencies.len() as u16);
        for (dependency, seq) in &self.header.dependencies {
            writer.raw(dependency.as_bytes());
            writer.u64(*seq);
        }
        match &self.content {
            Content::Changes(changes) => {
                writer.u16(changes.len() as u16);
                for change in changes {
                    writer.u16(change.table.0);
                    writer.bytes_u8(change.row.as_str().as_bytes());
                    writer.u8(change.action.as_u8());
                    writer.u64(change.mask().0);
                    for group in &change.groups {
                        writer.raw(&group.hash);
                    }
                    if change.action == ChangeAction::Tombstone {
                        match &change.replaced_by {
                            Some(target) => {
                                writer.u8(1);
                                writer.bytes_u8(target.as_str().as_bytes());
                            }
                            None => writer.u8(0),
                        }
                    }
                }
            }
            Content::Genesis(certificate) => {
                let encoded = certificate.encode();
                writer.u16(encoded.len() as u16);
                writer.raw(&encoded);
            }
            Content::Revoke(revocation) => {
                writer.raw(revocation.writer.as_bytes());
                writer.u64(revocation.cutoff);
                writer.u8(revocation.reason.as_u8());
            }
        }
        writer
    }

    fn encode_body(&self) -> Writer {
        let mut writer = Writer::default();
        if let Content::Changes(changes) = &self.content {
            for change in changes {
                for group in &change.groups {
                    writer.u8(PRESENT);
                    group.value.encode_into(&mut writer);
                }
            }
        }
        writer
    }

    /// Validates, encodes, and signs the operation.
    pub fn seal(&self, key: &WriterKeyPair) -> Result<SealedOperation, OperationError> {
        self.validate()?;
        if WriterId::for_public_key(&key.public_key()) != self.writer {
            return Err(OperationError::WriterMismatch);
        }
        let header = self.encode_header();
        if header.len() > MAX_HEADER_BYTES {
            return Err(CodecError::Bounds("header").into());
        }
        let body = self.encode_body();
        let total = ENVELOPE_OVERHEAD + header.len() + body.len();
        if total > MAX_OPERATION_BYTES {
            return Err(CodecError::Bounds("operation").into());
        }
        let mut writer = Writer::with_capacity(total);
        writer.raw(OPERATION_MAGIC);
        writer.u8(FORMAT_VERSION);
        writer.u8(self.kind().as_u8());
        writer.raw(self.space.as_bytes());
        writer.raw(self.writer.as_bytes());
        writer.u64(self.seq);
        writer.raw(&self.previous_hash);
        writer.bytes_u32(header.as_bytes());
        let signature = key.sign_operation(writer.as_bytes());
        let hash = operation_hash(writer.as_bytes());
        writer.bytes_u32(body.as_bytes());
        writer.raw(&signature);
        Ok(SealedOperation {
            bytes: writer.into_bytes(),
            hash,
        })
    }
}

/// Envelope bytes outside the header and body.
pub const ENVELOPE_OVERHEAD: usize =
    4 + 1 + 1 + ID_BYTES + ID_BYTES + 8 + DIGEST_BYTES + 4 + 4 + WRITER_SIGNATURE_BYTES;

/// Hash of the signed envelope bytes.
pub fn operation_hash(signed_bytes: &[u8]) -> Digest32 {
    domain_digest(OPERATION_HASH_DOMAIN, &[signed_bytes])
}

/// An encoded and signed operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedOperation {
    bytes: Vec<u8>,
    hash: Digest32,
}

impl SealedOperation {
    /// Encoded bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the operation into its bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Operation hash.
    pub fn hash(&self) -> Digest32 {
        self.hash
    }
}

/// Why the payload of a well-formed envelope cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// The format version is newer than this build.
    NewerFormat,
    /// The kind is unknown to this format version.
    UnknownKind,
    /// The signed header breaks the format.
    Invalid(OperationError),
    /// The unsigned body does not match a well-formed header: it cannot be decoded or a value
    /// does not hash to its signed hash. Another copy of the same operation may be intact.
    Body(CodecError),
}

impl fmt::Display for PayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewerFormat => formatter.write_str("operation format is newer than this build"),
            Self::UnknownKind => formatter.write_str("operation kind is unknown"),
            Self::Invalid(error) => error.fmt(formatter),
            Self::Body(error) => write!(formatter, "operation body does not match: {error}"),
        }
    }
}

impl std::error::Error for PayloadError {}

impl From<CodecError> for PayloadError {
    fn from(error: CodecError) -> Self {
        Self::Invalid(OperationError::Codec(error))
    }
}

impl From<CertificateError> for PayloadError {
    fn from(error: CertificateError) -> Self {
        Self::Invalid(OperationError::Certificate(error))
    }
}

/// The format-stable part of an operation, borrowed from its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope<'a> {
    /// Format version of the header and body.
    pub format_version: u8,
    /// Wire kind.
    pub kind: u8,
    /// Space id.
    pub space: SpaceId,
    /// Writer id.
    pub writer: WriterId,
    /// Sequence, from 1.
    pub seq: Seq,
    /// Previous operation hash.
    pub previous_hash: Digest32,
    /// Encoded header.
    pub header: &'a [u8],
    /// Encoded body.
    pub body: &'a [u8],
    /// Writer signature.
    pub signature: &'a [u8],
    /// Signed bytes: everything before the body length.
    pub signed: &'a [u8],
}

impl<'a> Envelope<'a> {
    /// Splits operation bytes into envelope fields without verifying the signature.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, CodecError> {
        if bytes.len() > MAX_OPERATION_BYTES {
            return Err(CodecError::Bounds("operation"));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(OPERATION_MAGIC.len())? != OPERATION_MAGIC {
            return Err(CodecError::Malformed("magic"));
        }
        let format_version = reader.u8()?;
        if format_version == 0 {
            return Err(CodecError::Malformed("format version"));
        }
        let kind = reader.u8()?;
        let space = SpaceId::from_bytes(reader.array()?);
        let writer = WriterId::from_bytes(reader.array()?);
        let seq = reader.u64()?;
        if seq == 0 {
            return Err(CodecError::Malformed("sequence"));
        }
        let previous_hash = reader.array()?;
        let header = reader.bytes_u32(MAX_HEADER_BYTES, "header")?;
        let signed_length = reader.position();
        let body = reader.bytes_u32(MAX_OPERATION_BYTES, "body")?;
        let signature = reader.take(WRITER_SIGNATURE_BYTES)?;
        reader.finish()?;
        Ok(Self {
            format_version,
            kind,
            space,
            writer,
            seq,
            previous_hash,
            header,
            body,
            signature,
            signed: &bytes[..signed_length],
        })
    }

    /// Operation identity.
    pub fn id(&self) -> OpId {
        OpId {
            space: self.space,
            writer: self.writer,
            seq: self.seq,
        }
    }

    /// Operation hash.
    pub fn hash(&self) -> Digest32 {
        operation_hash(self.signed)
    }

    /// Whether the writer key signed this envelope and derives its writer id.
    pub fn verify(&self, key: &WriterPublicKey) -> bool {
        WriterId::for_public_key(key) == self.writer
            && key.verify_operation(self.signed, self.signature)
    }

    /// Decodes the header and body. A genesis certificate's person signature is verified; the
    /// envelope signature is not.
    ///
    /// The header is decoded completely before the body, so [`PayloadError::Body`] means the
    /// signed header is well formed and only the unsigned body fails to match it.
    pub fn operation(&self) -> Result<Operation, PayloadError> {
        if self.format_version > FORMAT_VERSION {
            return Err(PayloadError::NewerFormat);
        }
        let kind = OperationKind::from_u8(self.kind).ok_or(PayloadError::UnknownKind)?;
        let mut header = Reader::new(self.header);
        let clock = Hlc::from_u64(header.u64()?);
        let manifest_version = header.u32()?;
        let authorization_revision = header.u64()?;
        let key_epoch = header.u32()?;
        let dependency_count = usize::from(header.u16()?);
        let mut dependencies = Vec::with_capacity(dependency_count.min(header.remaining() / 24));
        for _ in 0..dependency_count {
            let writer = WriterId::from_bytes(header.array()?);
            dependencies.push((writer, header.u64()?));
        }
        let payload = match kind {
            OperationKind::Changes => HeaderPayload::Changes(decode_change_headers(&mut header)?),
            OperationKind::Genesis => {
                let length = usize::from(header.u16()?);
                let certificate = SignedCertificate::decode(header.take(length)?)?;
                HeaderPayload::Complete(Content::Genesis(certificate))
            }
            OperationKind::Revoke => {
                let writer = WriterId::from_bytes(header.array()?);
                let cutoff = header.u64()?;
                let reason = RevokeReason::from_u8(header.u8()?)
                    .ok_or(CodecError::Malformed("revoke reason"))?;
                HeaderPayload::Complete(Content::Revoke(Revocation {
                    writer,
                    cutoff,
                    reason,
                }))
            }
        };
        header.finish()?;
        let content = match payload {
            HeaderPayload::Changes(changes) => {
                Content::Changes(attach_values(changes, self.body).map_err(PayloadError::Body)?)
            }
            HeaderPayload::Complete(content) => {
                expect_empty_body(self.body).map_err(PayloadError::Body)?;
                content
            }
        };
        let operation = Operation {
            space: self.space,
            writer: self.writer,
            seq: self.seq,
            previous_hash: self.previous_hash,
            header: Header {
                clock,
                manifest_version,
                authorization_revision,
                key_epoch,
                dependencies,
            },
            content,
        };
        operation.validate().map_err(PayloadError::Invalid)?;
        Ok(operation)
    }

    /// Decodes a genesis and verifies the envelope signature with the key it certifies.
    pub fn genesis(&self) -> Result<(Operation, SignedCertificate), PayloadError> {
        let operation = self.operation()?;
        let Content::Genesis(certificate) = &operation.content else {
            return Err(CodecError::Malformed("genesis kind").into());
        };
        let certificate = certificate.clone();
        if !self.verify(&certificate.certificate.writer_key) {
            return Err(PayloadError::Invalid(OperationError::Signature));
        }
        Ok((operation, certificate))
    }
}

/// The kind payload of a decoded header, before the body is attached.
enum HeaderPayload {
    Changes(Vec<ChangeHeader>),
    Complete(Content),
}

/// One change as the signed header describes it.
struct ChangeHeader {
    table: TableId,
    row: RowKey,
    action: ChangeAction,
    hashes: Vec<(GroupId, Digest32)>,
    replaced_by: Option<RowKey>,
}

fn expect_empty_body(body: &[u8]) -> Result<(), CodecError> {
    if body.is_empty() {
        Ok(())
    } else {
        Err(CodecError::Malformed("body"))
    }
}

fn read_row_key(reader: &mut Reader<'_>) -> Result<RowKey, CodecError> {
    let bytes = reader.bytes_u8(MAX_ROW_KEY_BYTES, "row key")?;
    let text = std::str::from_utf8(bytes).map_err(|_| CodecError::Malformed("row key"))?;
    RowKey::new(text).map_err(|_| CodecError::Malformed("row key"))
}

fn decode_change_headers(header: &mut Reader<'_>) -> Result<Vec<ChangeHeader>, CodecError> {
    let count = usize::from(header.u16()?);
    if count == 0 || count > MAX_CHANGES_PER_OPERATION {
        return Err(CodecError::Bounds("changes"));
    }
    let mut changes = Vec::with_capacity(count);
    for _ in 0..count {
        let table = TableId(header.u16()?);
        let row = read_row_key(header)?;
        let action =
            ChangeAction::from_u8(header.u8()?).ok_or(CodecError::Malformed("change action"))?;
        let mask = GroupMask(header.u64()?);
        let mut hashes = Vec::with_capacity(mask.len());
        for group in mask.iter() {
            hashes.push((group, header.array::<DIGEST_BYTES>()?));
        }
        let replaced_by = if action == ChangeAction::Tombstone {
            match header.u8()? {
                0 => None,
                1 => Some(read_row_key(header)?),
                _ => return Err(CodecError::Malformed("tombstone redirect")),
            }
        } else {
            None
        };
        changes.push(ChangeHeader {
            table,
            row,
            action,
            hashes,
            replaced_by,
        });
    }
    Ok(changes)
}

/// Reads the body values the change headers announce and checks each against its signed hash.
fn attach_values(headers: Vec<ChangeHeader>, body: &[u8]) -> Result<Vec<Change>, CodecError> {
    let mut body = Reader::new(body);
    let mut changes = Vec::with_capacity(headers.len());
    for change in headers {
        let mut groups = Vec::with_capacity(change.hashes.len());
        for (group, hash) in change.hashes {
            if body.u8()? != PRESENT {
                return Err(CodecError::Malformed("value presence"));
            }
            let value = Value::decode_from(&mut body)?;
            if value.hash(change.table, group) != hash {
                return Err(CodecError::Malformed("value hash"));
            }
            groups.push(GroupValue { group, hash, value });
        }
        changes.push(Change {
            table: change.table,
            row: change.row,
            action: change.action,
            groups,
            replaced_by: change.replaced_by,
        });
    }
    body.finish()?;
    Ok(changes)
}
