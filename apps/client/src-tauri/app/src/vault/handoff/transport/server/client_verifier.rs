//! Client certificate verification pinned to the exact certificates of known devices.

use crate::vault::handoff::pairing::certificate_fingerprint;
use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use std::collections::HashSet;

/// Accepts a client certificate only when it is byte for byte one of the pinned device
/// certificates, with no intermediates. Device certificates are self-signed and identified by
/// fingerprint, so path building, subjects, and validity dates never decide trust. On the
/// coordinator a client may still connect without a certificate to enroll; it then stays on the
/// unauthenticated budget. A member listener requires a certificate.
#[derive(Debug)]
pub(super) struct PinnedClientVerifier {
    fingerprints: HashSet<String>,
    mandatory: bool,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedClientVerifier {
    /// The coordinator's verifier: pinned to linked device certificates, client auth optional.
    #[cfg(desktop)]
    pub(super) fn new(certificates: &[CertificateDer<'_>]) -> Self {
        Self::pinned(
            certificates
                .iter()
                .map(|certificate| certificate_fingerprint(certificate.as_ref())),
            false,
        )
    }

    /// A verifier pinned to lowercase hex SHA-256 `fingerprints`, requiring a client
    /// certificate when `mandatory`.
    pub(super) fn pinned(fingerprints: impl IntoIterator<Item = String>, mandatory: bool) -> Self {
        Self {
            fingerprints: fingerprints.into_iter().collect(),
            mandatory,
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        }
    }
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        self.mandatory
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        if !intermediates.is_empty()
            || !self
                .fingerprints
                .contains(&certificate_fingerprint(end_entity.as_ref()))
        {
            return Err(rustls::Error::General(
                "device certificate is not admitted by this listener".to_string(),
            ));
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, certificate, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, certificate, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::handoff::pairing::{PairingManager, random_token};

    fn device_certificate(label: &str) -> (std::path::PathBuf, CertificateDer<'static>) {
        let root = std::env::temp_dir().join(format!(
            "ganbaru-client-verifier-{label}-{}",
            random_token("test").expect("random test directory")
        ));
        std::fs::create_dir_all(&root).expect("create test directory");
        let manager = PairingManager::default();
        manager
            .initialize(root.clone(), format!("device-{label}"))
            .expect("initialize device");
        let (_, identity) = manager.identity().expect("device identity");
        (root, identity.certificate)
    }

    #[test]
    fn only_an_exact_pinned_certificate_without_intermediates_is_accepted() {
        let (linked_root, linked) = device_certificate("linked");
        let (other_root, other) = device_certificate("other");
        let verifier = PinnedClientVerifier::new(std::slice::from_ref(&linked));

        let accepted = verifier.verify_client_cert(&linked, &[], UnixTime::now());
        let unknown = verifier.verify_client_cert(&other, &[], UnixTime::now());
        let chained =
            verifier.verify_client_cert(&linked, std::slice::from_ref(&other), UnixTime::now());
        let _ = std::fs::remove_dir_all(linked_root);
        let _ = std::fs::remove_dir_all(other_root);

        assert!(accepted.is_ok());
        assert!(unknown.is_err());
        assert!(chained.is_err());
        assert!(verifier.root_hint_subjects().is_empty());
        assert!(!verifier.client_auth_mandatory());
    }
}
