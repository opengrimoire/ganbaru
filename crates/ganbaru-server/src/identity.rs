//! The relay's long-lived self-signed TLS identity, kept in its data directory. Devices pin the
//! certificate's SHA-256 fingerprint from the setup code, so the certificate never needs a CA
//! and never rotates on its own.

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, read_regular_file_capped, write_file_atomically,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// File name of the DER certificate in the data directory.
pub const CERTIFICATE_FILE: &str = "relay-certificate.der";
/// File name of the PKCS#8 private key in the data directory.
pub const PRIVATE_KEY_FILE: &str = "relay-key.der";
/// Name the certificate is issued for; devices pin the fingerprint, not the name.
pub const CERTIFICATE_NAME: &str = "ganbaru-relay.local";
/// Largest certificate or key file the relay reads.
const MAX_IDENTITY_FILE_BYTES: u64 = 64 * 1024;

/// The relay's TLS certificate and key. `Debug` leaves the key out.
pub struct RelayIdentity {
    certificate: CertificateDer<'static>,
    private_key: PrivatePkcs8KeyDer<'static>,
}

impl fmt::Debug for RelayIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayIdentity")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

impl RelayIdentity {
    /// Loads the identity from `data_dir`, creating and storing a new one when neither file
    /// exists. One file without the other is an error, so a damaged directory never silently
    /// changes the pinned certificate.
    pub fn load_or_create(data_dir: &Path) -> Result<Self, String> {
        let certificate_path = data_dir.join(CERTIFICATE_FILE);
        let key_path = data_dir.join(PRIVATE_KEY_FILE);
        match (certificate_path.exists(), key_path.exists()) {
            (true, true) => {
                let certificate = read_identity_file(&certificate_path, "relay certificate")?;
                let key = read_identity_file(&key_path, "relay private key")?;
                let identity = Self {
                    certificate: CertificateDer::from(certificate),
                    private_key: PrivatePkcs8KeyDer::from(key),
                };
                identity.server_config()?;
                Ok(identity)
            }
            (false, false) => {
                let identity = Self::generate()?;
                write_file_atomically(
                    &key_path,
                    identity.private_key.secret_pkcs8_der(),
                    FileAccess::OwnerOnly,
                )
                .map_err(|error| format!("store relay private key: {error}"))?;
                write_file_atomically(
                    &certificate_path,
                    identity.certificate.as_ref(),
                    FileAccess::OwnerOnly,
                )
                .map_err(|error| format!("store relay certificate: {error}"))?;
                Ok(identity)
            }
            _ => Err(format!(
                "the data directory holds only one of {CERTIFICATE_FILE} and {PRIVATE_KEY_FILE}; \
                 restore the missing file or remove both to create a new identity"
            )),
        }
    }

    /// A new identity that is not stored anywhere.
    pub fn generate() -> Result<Self, String> {
        let key =
            rcgen::KeyPair::generate().map_err(|error| format!("generate relay key: {error}"))?;
        let mut params = rcgen::CertificateParams::new(vec![CERTIFICATE_NAME.to_string()])
            .map_err(|error| format!("configure relay certificate: {error}"))?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, CERTIFICATE_NAME);
        let certificate = params
            .self_signed(&key)
            .map_err(|error| format!("generate relay certificate: {error}"))?;
        Ok(Self {
            certificate: certificate.der().clone(),
            private_key: PrivatePkcs8KeyDer::from(key.serialize_der()),
        })
    }

    /// SHA-256 of the DER certificate, which devices pin.
    pub fn fingerprint_bytes(&self) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(self.certificate.as_ref()).into()
    }

    /// Lowercase hexadecimal fingerprint, as operators compare it.
    pub fn fingerprint(&self) -> String {
        ganbaru_handoff::pairing::certificate_fingerprint(self.certificate.as_ref())
    }

    /// The DER certificate.
    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.certificate
    }

    /// A TLS 1.3-only server configuration without client authentication.
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| format!("configure relay TLS: {error}"))?
            .with_no_client_auth()
            .with_single_cert(
                vec![self.certificate.clone()],
                PrivateKeyDer::Pkcs8(self.private_key.clone_key()),
            )
            .map_err(|error| format!("load relay TLS identity: {error}"))?;
        Ok(Arc::new(config))
    }
}

fn read_identity_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let bytes = read_regular_file_capped(path, MAX_IDENTITY_FILE_BYTES, FinalSymlink::Refuse)
        .map_err(|error| format!("read {label}: {error}"))?;
    if bytes.is_empty() {
        return Err(format!("{label} is empty"));
    }
    Ok(bytes)
}
