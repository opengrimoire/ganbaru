//! Space charter: revision 0 of a space's authorization log.
//!
//! Big endian: version (1) | space id (16) | anchor device id length (1) and device id | anchor
//! device public key (32) | anchor TLS fingerprint (32) | anchor admin public key (32) | recovery
//! admin public key (32) | initial discovery key (32) | created at milliseconds (8) | person
//! signature (64) over the charter domain and everything before it.
//!
//! The charter enrolls the first anchor as an admin desktop holding the anchor admin key, so the
//! anchor signs its own proposals from revision 1 on. The person key that signs a charter is the vault owner's key. Decoding does not verify the
//! signature, because a receiver may hold the charter before the person key arrives; verification
//! is a separate step against the person key the receiver trusts. The charter hash is the
//! previous entry hash of authorization revision 1.

use crate::bounds::{DISCOVERY_KEY_BYTES, MAX_CHARTER_BYTES, TLS_FINGERPRINT_BYTES};
use crate::certificate::{CertificateError, read_device_id, validate_device_id};
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::SpaceId;
use crate::keys::{AdminPublicKey, DevicePublicKey, PUBLIC_KEY_BYTES};
use crate::signing::{
    CHARTER_DOMAIN, CHARTER_HASH_DOMAIN, Digest32, domain_digest, domain_message,
};
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey, SIGNATURE_BYTES, verify};
use std::fmt;

const CHARTER_VERSION: u8 = 1;

/// A charter that cannot be signed, decoded, or verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharterError {
    /// The encoding is malformed or exceeds a bound.
    Codec(CodecError),
    /// The charter version is unknown.
    Version,
    /// The anchor device id is not a valid identifier.
    DeviceId,
    /// The person signature does not verify against the expected person key.
    Signature,
}

impl fmt::Display for CharterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(formatter, "space charter: {error}"),
            Self::Version => formatter.write_str("space charter version is unsupported"),
            Self::DeviceId => formatter.write_str("space charter anchor device id is invalid"),
            Self::Signature => formatter.write_str("space charter signature does not verify"),
        }
    }
}

impl std::error::Error for CharterError {}

impl From<CodecError> for CharterError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<CertificateError> for CharterError {
    fn from(error: CertificateError) -> Self {
        match error {
            CertificateError::Codec(error) => Self::Codec(error),
            _ => Self::DeviceId,
        }
    }
}

/// Unsigned charter contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charter {
    /// The space the charter founds.
    pub space: SpaceId,
    /// Device id of the first anchor.
    pub anchor_device_id: String,
    /// Device public key of the first anchor.
    pub anchor_device_key: DevicePublicKey,
    /// TLS certificate fingerprint of the first anchor.
    pub anchor_tls_fingerprint: [u8; TLS_FINGERPRINT_BYTES],
    /// Admin public key the first anchor signs its proposals with.
    pub anchor_admin_key: AdminPublicKey,
    /// Recovery admin public key derived from the recovery kit.
    pub recovery_admin_key: AdminPublicKey,
    /// Discovery key in force until the first removal rotates it.
    pub discovery_key: [u8; DISCOVERY_KEY_BYTES],
    /// Creation time in Unix milliseconds.
    pub created_at_ms: i64,
}

/// A charter with its person signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCharter {
    /// Charter contents.
    pub charter: Charter,
    /// Person signature over the charter domain and the unsigned encoding.
    pub signature: [u8; SIGNATURE_BYTES],
}

fn encode_unsigned(charter: &Charter, writer: &mut Writer) {
    writer.u8(CHARTER_VERSION);
    writer.raw(charter.space.as_bytes());
    writer.bytes_u8(charter.anchor_device_id.as_bytes());
    writer.raw(charter.anchor_device_key.as_bytes());
    writer.raw(&charter.anchor_tls_fingerprint);
    writer.raw(charter.anchor_admin_key.as_bytes());
    writer.raw(charter.recovery_admin_key.as_bytes());
    writer.raw(&charter.discovery_key);
    writer.i64(charter.created_at_ms);
}

/// Signs a charter with the vault owner's person key.
pub fn sign_charter(
    charter: &Charter,
    person: &PersonKeyPair,
) -> Result<SignedCharter, CharterError> {
    validate_device_id(&charter.anchor_device_id)?;
    let mut writer = Writer::default();
    encode_unsigned(charter, &mut writer);
    let signature = person.sign(&domain_message(CHARTER_DOMAIN, writer.as_bytes()));
    Ok(SignedCharter {
        charter: charter.clone(),
        signature,
    })
}

impl SignedCharter {
    /// Appends the canonical encoding.
    pub fn encode_into(&self, writer: &mut Writer) {
        encode_unsigned(&self.charter, writer);
        writer.raw(&self.signature);
    }

    /// Canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        self.encode_into(&mut writer);
        writer.into_bytes()
    }

    /// Reads one charter without verifying its signature.
    pub fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CharterError> {
        if reader.u8()? != CHARTER_VERSION {
            return Err(CharterError::Version);
        }
        let space = SpaceId::from_bytes(reader.array()?);
        let anchor_device_id = read_device_id(reader)?;
        let anchor_device_key = DevicePublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
        let anchor_tls_fingerprint = reader.array()?;
        let anchor_admin_key = AdminPublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
        let recovery_admin_key = AdminPublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
        let discovery_key = reader.array()?;
        let created_at_ms = reader.i64()?;
        let signature = reader.array()?;
        Ok(Self {
            charter: Charter {
                space,
                anchor_device_id,
                anchor_device_key,
                anchor_tls_fingerprint,
                anchor_admin_key,
                recovery_admin_key,
                discovery_key,
                created_at_ms,
            },
            signature,
        })
    }

    /// Decodes a complete charter encoding without verifying its signature.
    pub fn decode(bytes: &[u8]) -> Result<Self, CharterError> {
        if bytes.len() > MAX_CHARTER_BYTES {
            return Err(CodecError::Bounds("space charter").into());
        }
        let mut reader = Reader::new(bytes);
        let charter = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(charter)
    }

    /// Verifies the person signature against the vault owner's person key.
    pub fn verify(&self, person: &PersonPublicKey) -> Result<(), CharterError> {
        let mut writer = Writer::default();
        encode_unsigned(&self.charter, &mut writer);
        verify(
            person,
            &domain_message(CHARTER_DOMAIN, writer.as_bytes()),
            &self.signature,
        )
        .map_err(|_| CharterError::Signature)
    }

    /// Charter hash over the signed encoding: the previous entry hash of revision 1.
    pub fn hash(&self) -> Digest32 {
        domain_digest(CHARTER_HASH_DOMAIN, &[&self.encode()])
    }
}
