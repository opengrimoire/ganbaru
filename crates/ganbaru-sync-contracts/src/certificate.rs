//! Writer certificates: a person key vouching for an installation's writer key.
//!
//! Binary layout, big endian: version (1) | writer key (32) | person key (32) | device id length
//! (1) and device id | created at milliseconds (8) | predecessor flag (1) and optional writer id
//! (16) | person signature (64) over the certificate domain and everything before it.

use crate::bounds::MAX_DEVICE_ID_BYTES;
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::{ID_BYTES, WriterId};
use crate::signing::{
    CERTIFICATE_DOMAIN, WRITER_PUBLIC_KEY_BYTES, WriterPublicKey, domain_message,
};
use ganbaru_contacts::{PUBLIC_KEY_BYTES, PersonKeyPair, PersonPublicKey, SIGNATURE_BYTES, verify};
use std::fmt;

const CERTIFICATE_VERSION: u8 = 1;

/// A certificate that cannot be issued, decoded, or verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateError {
    /// The encoding is malformed or exceeds a bound.
    Codec(CodecError),
    /// The certificate version is unknown.
    Version,
    /// The device id is not a valid identifier.
    DeviceId,
    /// The signing key is not the certified person key.
    WrongPerson,
    /// The person signature does not verify.
    Signature,
}

impl fmt::Display for CertificateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(formatter, "writer certificate: {error}"),
            Self::Version => formatter.write_str("writer certificate version is unsupported"),
            Self::DeviceId => formatter.write_str("writer certificate device id is invalid"),
            Self::WrongPerson => formatter.write_str("writer certificate person key mismatch"),
            Self::Signature => formatter.write_str("writer certificate signature does not verify"),
        }
    }
}

impl std::error::Error for CertificateError {}

impl From<CodecError> for CertificateError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

/// Unsigned certificate contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterCertificate {
    /// The certified writer key.
    pub writer_key: WriterPublicKey,
    /// The person key that vouches for it.
    pub person_key: PersonPublicKey,
    /// Handoff device id of the installation that owns the writer.
    pub device_id: String,
    /// Creation time in Unix milliseconds.
    pub created_at_ms: i64,
    /// Retired writer this one replaces on the same installation. Informational only.
    pub predecessor: Option<WriterId>,
}

impl WriterCertificate {
    /// Writer id the certificate binds.
    pub fn writer_id(&self) -> WriterId {
        WriterId::for_public_key(&self.writer_key)
    }
}

/// A certificate with its person signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCertificate {
    /// Certified contents.
    pub certificate: WriterCertificate,
    /// Person signature over the certificate domain and the unsigned encoding.
    pub signature: [u8; SIGNATURE_BYTES],
}

/// Validates a device id with the handoff identifier rule: 1 to 160 ASCII letters, digits, `-`,
/// `_`, or `.`.
pub fn validate_device_id(device_id: &str) -> Result<(), CertificateError> {
    let valid = !device_id.is_empty()
        && device_id.len() <= MAX_DEVICE_ID_BYTES
        && device_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(CertificateError::DeviceId)
    }
}

fn encode_unsigned(certificate: &WriterCertificate, writer: &mut Writer) {
    writer.u8(CERTIFICATE_VERSION);
    writer.raw(certificate.writer_key.as_bytes());
    writer.raw(certificate.person_key.as_bytes());
    writer.bytes_u8(certificate.device_id.as_bytes());
    writer.i64(certificate.created_at_ms);
    match certificate.predecessor {
        Some(predecessor) => {
            writer.u8(1);
            writer.raw(predecessor.as_bytes());
        }
        None => writer.u8(0),
    }
}

/// Signs a certificate with the person key it names.
pub fn sign_certificate(
    certificate: &WriterCertificate,
    person: &PersonKeyPair,
) -> Result<SignedCertificate, CertificateError> {
    validate_device_id(&certificate.device_id)?;
    if person.public_key() != certificate.person_key {
        return Err(CertificateError::WrongPerson);
    }
    let mut writer = Writer::default();
    encode_unsigned(certificate, &mut writer);
    let signature = person.sign(&domain_message(CERTIFICATE_DOMAIN, writer.as_bytes()));
    Ok(SignedCertificate {
        certificate: certificate.clone(),
        signature,
    })
}

impl SignedCertificate {
    /// Appends the canonical encoding.
    pub fn encode_into(&self, writer: &mut Writer) {
        encode_unsigned(&self.certificate, writer);
        writer.raw(&self.signature);
    }

    /// Canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        self.encode_into(&mut writer);
        writer.into_bytes()
    }

    /// Reads one certificate and verifies its person signature.
    pub fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CertificateError> {
        let start = reader.position();
        if reader.u8()? != CERTIFICATE_VERSION {
            return Err(CertificateError::Version);
        }
        let writer_key = WriterPublicKey::from_bytes(reader.array::<WRITER_PUBLIC_KEY_BYTES>()?);
        let person_key = PersonPublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
        let device_id = reader.bytes_u8(MAX_DEVICE_ID_BYTES, "device id")?;
        let device_id = std::str::from_utf8(device_id)
            .map_err(|_| CertificateError::DeviceId)?
            .to_string();
        validate_device_id(&device_id)?;
        let created_at_ms = reader.i64()?;
        let predecessor = match reader.u8()? {
            0 => None,
            1 => Some(WriterId::from_bytes(reader.array::<ID_BYTES>()?)),
            _ => return Err(CodecError::Malformed("certificate predecessor").into()),
        };
        let certificate = WriterCertificate {
            writer_key,
            person_key,
            device_id,
            created_at_ms,
            predecessor,
        };
        let mut unsigned = Writer::default();
        encode_unsigned(&certificate, &mut unsigned);
        debug_assert_eq!(unsigned.len(), reader.position() - start);
        let signature = reader.array::<SIGNATURE_BYTES>()?;
        verify(
            &certificate.person_key,
            &domain_message(CERTIFICATE_DOMAIN, unsigned.as_bytes()),
            &signature,
        )
        .map_err(|_| CertificateError::Signature)?;
        Ok(Self {
            certificate,
            signature,
        })
    }

    /// Decodes a complete certificate encoding and verifies its person signature.
    pub fn decode(bytes: &[u8]) -> Result<Self, CertificateError> {
        let mut reader = Reader::new(bytes);
        let certificate = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(certificate)
    }
}
