//! Writer certificates: a device key vouching for an installation's writer key.
//!
//! Big endian: version (1, value 2) | writer key (32) | device id length (1) and device id |
//! device public key (32) | created at milliseconds (8) | predecessor flag (1) and optional writer
//! id (16) | device signature (64) over the certificate domain and everything before it. The
//! device key is the one the device's enrollment records in the authorization log, so a device
//! mints its own successor writers and a certificate verifies from the log alone. Version 1, a
//! person-signed form, is unsupported and decodes as an unknown version.

use crate::bounds::{MAX_CERTIFICATE_BYTES, MAX_DEVICE_ID_BYTES};
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::{ID_BYTES, WriterId};
use crate::keys::{DeviceKeyPair, DevicePublicKey};
use crate::signing::{CERTIFICATE_DOMAIN, WRITER_PUBLIC_KEY_BYTES, WriterPublicKey};
use ganbaru_contacts::{PUBLIC_KEY_BYTES, SIGNATURE_BYTES};
use std::fmt;

const CERTIFICATE_VERSION: u8 = 2;

/// A certificate that cannot be issued, decoded, or verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateError {
    /// The encoding is malformed or exceeds a bound.
    Codec(CodecError),
    /// The certificate version is unknown.
    Version,
    /// The device id is not a valid identifier.
    DeviceId,
    /// The signing key is not the certified device key.
    WrongDevice,
    /// The device signature does not verify.
    Signature,
}

impl fmt::Display for CertificateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(formatter, "writer certificate: {error}"),
            Self::Version => formatter.write_str("writer certificate version is unsupported"),
            Self::DeviceId => formatter.write_str("writer certificate device id is invalid"),
            Self::WrongDevice => formatter.write_str("writer certificate device key mismatch"),
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

/// Unsigned certificate contents, vouched for by the owning device's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterCertificate {
    /// The certified writer key.
    pub writer_key: WriterPublicKey,
    /// Handoff device id of the installation that owns the writer.
    pub device_id: String,
    /// The device key that vouches for the writer, as its enrollment records it.
    pub device_key: DevicePublicKey,
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

/// A certificate with its device signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCertificate {
    /// Certified contents.
    pub certificate: WriterCertificate,
    /// Device signature over the certificate domain and the unsigned encoding.
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

/// Reads a device id with a one-byte length and validates it.
pub(crate) fn read_device_id(reader: &mut Reader<'_>) -> Result<String, CertificateError> {
    let device_id = reader.bytes_u8(MAX_DEVICE_ID_BYTES, "device id")?;
    let device_id = std::str::from_utf8(device_id)
        .map_err(|_| CertificateError::DeviceId)?
        .to_string();
    validate_device_id(&device_id)?;
    Ok(device_id)
}

fn write_predecessor(predecessor: Option<WriterId>, writer: &mut Writer) {
    match predecessor {
        Some(predecessor) => {
            writer.u8(1);
            writer.raw(predecessor.as_bytes());
        }
        None => writer.u8(0),
    }
}

fn read_predecessor(reader: &mut Reader<'_>) -> Result<Option<WriterId>, CertificateError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(WriterId::from_bytes(reader.array::<ID_BYTES>()?))),
        _ => Err(CodecError::Malformed("certificate predecessor").into()),
    }
}

fn encode_unsigned(certificate: &WriterCertificate, writer: &mut Writer) {
    writer.u8(CERTIFICATE_VERSION);
    writer.raw(certificate.writer_key.as_bytes());
    writer.bytes_u8(certificate.device_id.as_bytes());
    writer.raw(certificate.device_key.as_bytes());
    writer.i64(certificate.created_at_ms);
    write_predecessor(certificate.predecessor, writer);
}

/// Signs a certificate with the device key it names.
pub fn sign_certificate(
    certificate: &WriterCertificate,
    device: &DeviceKeyPair,
) -> Result<SignedCertificate, CertificateError> {
    validate_device_id(&certificate.device_id)?;
    if device.public_key() != certificate.device_key {
        return Err(CertificateError::WrongDevice);
    }
    let mut writer = Writer::default();
    encode_unsigned(certificate, &mut writer);
    let signature = device.sign(CERTIFICATE_DOMAIN, writer.as_bytes());
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

    /// Reads one certificate and verifies its device signature. Whether the device key belongs
    /// to an enrolled device is the authorization log's question, not the certificate's.
    pub fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CertificateError> {
        let start = reader.position();
        if reader.u8()? != CERTIFICATE_VERSION {
            return Err(CertificateError::Version);
        }
        let writer_key = WriterPublicKey::from_bytes(reader.array::<WRITER_PUBLIC_KEY_BYTES>()?);
        let device_id = read_device_id(reader)?;
        let device_key = DevicePublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
        let created_at_ms = reader.i64()?;
        let predecessor = read_predecessor(reader)?;
        let certificate = WriterCertificate {
            writer_key,
            device_id,
            device_key,
            created_at_ms,
            predecessor,
        };
        let mut unsigned = Writer::default();
        encode_unsigned(&certificate, &mut unsigned);
        debug_assert_eq!(unsigned.len(), reader.position() - start);
        let signature = reader.array::<SIGNATURE_BYTES>()?;
        if !certificate
            .device_key
            .verify(CERTIFICATE_DOMAIN, unsigned.as_bytes(), &signature)
        {
            return Err(CertificateError::Signature);
        }
        Ok(Self {
            certificate,
            signature,
        })
    }

    /// Decodes a complete certificate encoding and verifies its device signature.
    pub fn decode(bytes: &[u8]) -> Result<Self, CertificateError> {
        if bytes.len() > MAX_CERTIFICATE_BYTES {
            return Err(CodecError::Bounds("writer certificate").into());
        }
        let mut reader = Reader::new(bytes);
        let certificate = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(certificate)
    }
}
