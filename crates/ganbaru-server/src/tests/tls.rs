//! The relay over TLS: claims, authentication, pushes and pulls, epoch repairs, admin commands,
//! assets, waits, connection budgets, operator commands, and space generations.

use super::{
    DAY_MS, TempDir, access, admin, chunk, claim_with, code_of, open_store, ranges, record,
    space_id, writer_id,
};
use crate::identity::RelayIdentity;
use crate::server::{ConnectionLimits, RelayServer, ServerProbe, unix_time_ms};
use crate::store::{ClaimRequest, RelayStore, RetentionPolicy};
use ganbaru_handoff::pairing::certificate_fingerprint;
use ganbaru_handoff::protocol::relay::{
    RELAY_ADMIN_DOMAIN, RELAY_AUTH_DOMAIN, RELAY_EPOCH_DOMAIN, RELAY_EXPORTER_BYTES,
    RELAY_EXPORTER_LABEL, RELAY_PROTOCOL_VERSION, RELAY_SETUP_TOKEN_LIFETIME, RelayAdminAction,
    RelayAdminCommand, RelayAssetOutcome, RelayBinding, RelayPushOutcome, RelayRefusalCode,
    RelayRequest, RelayResponse, RelaySetupToken, decode_relay_bytes, encode_relay_asset,
    encode_relay_bytes,
};
use ganbaru_handoff::protocol::{MAX_CONTROL_BYTES, MAX_UNAUTHENTICATED_CONTROL_BYTES, SyncSeq};
use ganbaru_handoff::protocol::{read_frame, write_frame};
use ganbaru_sync_contracts::{AdminKeyPair, AdminPublicKey, RelayAccessKeyPair, SpaceId};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

#[derive(Debug)]
struct PinnedVerifier {
    fingerprint: String,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if !intermediates.is_empty()
            || certificate_fingerprint(end_entity.as_ref()) != self.fingerprint
        {
            return Err(rustls::Error::General(
                "relay certificate is not pinned".to_string(),
            ));
        }
        Ok(ServerCertVerified::assertion())
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

struct Relay {
    _dir: TempDir,
    address: SocketAddr,
    fingerprint: String,
    store: RelayStore,
    probe: ServerProbe,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start_relay(limits: ConnectionLimits) -> Relay {
    let dir = TempDir::new();
    let identity = RelayIdentity::load_or_create(dir.path()).unwrap();
    // A second load reads the same identity back.
    assert_eq!(
        RelayIdentity::load_or_create(dir.path())
            .unwrap()
            .fingerprint(),
        identity.fingerprint()
    );
    let store = open_store(&dir, RetentionPolicy::default()).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = RelayServer::new(
        listener,
        identity.server_config().unwrap(),
        store.clone(),
        limits,
    );
    let address = server.local_addr().unwrap();
    let probe = server.probe();
    let task = tokio::spawn(server.run());
    Relay {
        _dir: dir,
        address,
        fingerprint: identity.fingerprint(),
        store,
        probe,
        task,
    }
}

struct Client {
    stream: TlsStream<TcpStream>,
    exporter: [u8; RELAY_EXPORTER_BYTES],
}

impl Client {
    async fn connect(relay: &Relay) -> Self {
        let verifier = Arc::new(PinnedVerifier {
            fingerprint: relay.fingerprint.clone(),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        });
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
        let socket = TcpStream::connect(relay.address).await.unwrap();
        let stream = TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("ganbaru-relay.local").unwrap(), socket)
            .await
            .unwrap();
        let exporter = stream
            .get_ref()
            .1
            .export_keying_material([0u8; RELAY_EXPORTER_BYTES], RELAY_EXPORTER_LABEL, None)
            .unwrap();
        Self { stream, exporter }
    }

    async fn send(&mut self, request: &RelayRequest) -> RelayResponse {
        write_frame(&mut self.stream, request).await.unwrap();
        read_frame(&mut self.stream, MAX_CONTROL_BYTES)
            .await
            .unwrap()
    }

    fn binding(&self, space: SpaceId) -> RelayBinding {
        RelayBinding {
            exporter: self.exporter,
            space,
        }
    }

    async fn hello(&mut self, space: SpaceId) {
        let response = self
            .send(&RelayRequest::Hello {
                protocol_version: RELAY_PROTOCOL_VERSION,
                space: space.to_hex(),
            })
            .await;
        assert_eq!(
            response,
            RelayResponse::Welcome {
                protocol_version: RELAY_PROTOCOL_VERSION
            }
        );
    }

    async fn auth(
        &mut self,
        space: SpaceId,
        epoch: u32,
        key: &RelayAccessKeyPair,
    ) -> RelayResponse {
        let signature = key.sign(RELAY_AUTH_DOMAIN, &self.binding(space).auth_payload(epoch));
        self.send(&RelayRequest::Auth {
            epoch,
            signature: encode_relay_bytes(&signature),
        })
        .await
    }

    async fn claim(
        &mut self,
        space: SpaceId,
        token: &RelaySetupToken,
        key: &RelayAccessKeyPair,
        owner: &AdminKeyPair,
    ) -> RelayResponse {
        let access_key = key.public_key();
        let admins = [owner.public_key()];
        let payload = self.binding(space).admin_payload(
            1,
            &RelayAdminCommand::Claim {
                epoch: 1,
                access_key: &access_key,
                admins: &admins,
            },
        );
        self.send(&RelayRequest::Claim {
            token: token.clone(),
            epoch: 1,
            access_key: encode_relay_bytes(access_key.as_bytes()),
            admins: vec![encode_relay_bytes(owner.public_key().as_bytes())],
            counter: 1,
            signer: encode_relay_bytes(owner.public_key().as_bytes()),
            signature: encode_relay_bytes(&owner.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
        .await
    }

    async fn admin(
        &mut self,
        space: SpaceId,
        signer: &AdminKeyPair,
        counter: u64,
        key: &AdminPublicKey,
        add: bool,
    ) -> RelayResponse {
        let command = if add {
            RelayAdminCommand::AddKey(key)
        } else {
            RelayAdminCommand::RemoveKey(key)
        };
        let payload = self.binding(space).admin_payload(counter, &command);
        let encoded = encode_relay_bytes(key.as_bytes());
        self.send(&RelayRequest::Admin {
            action: if add {
                RelayAdminAction::AddKey { key: encoded }
            } else {
                RelayAdminAction::RemoveKey { key: encoded }
            },
            counter,
            signer: encode_relay_bytes(signer.public_key().as_bytes()),
            signature: encode_relay_bytes(&signer.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
        .await
    }
}

fn refusal(response: &RelayResponse) -> Option<RelayRefusalCode> {
    match response {
        RelayResponse::Refused { code, .. } => Some(*code),
        _ => None,
    }
}

async fn issue_token(relay: &Relay) -> RelaySetupToken {
    let token = RelaySetupToken::generate().unwrap();
    let lifetime = i64::try_from(RELAY_SETUP_TOKEN_LIFETIME.as_millis()).unwrap();
    relay
        .store
        .add_setup_token(&token, unix_time_ms() + lifetime)
        .await
        .unwrap();
    token
}

#[tokio::test]
async fn devices_claim_authenticate_push_pull_and_rotate_over_tls() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let owner = admin();
    let first = access(&space, 1);
    let token = issue_token(&relay).await;

    let mut client = Client::connect(&relay).await;
    client.hello(space).await;
    assert_eq!(
        refusal(&client.send(&RelayRequest::State).await),
        Some(RelayRefusalCode::Unauthenticated)
    );
    assert_eq!(
        client.claim(space, &token, &first, &owner).await,
        RelayResponse::Claimed { epoch: 1 }
    );
    // The same token on another connection is spent.
    let mut reuse = Client::connect(&relay).await;
    reuse.hello(space_id(2)).await;
    assert_eq!(
        refusal(
            &reuse
                .claim(space_id(2), &token, &access(&space_id(2), 1), &owner)
                .await
        ),
        Some(RelayRefusalCode::Token)
    );

    // A signature made for another connection does not authenticate this one.
    let mut other = Client::connect(&relay).await;
    other.hello(space).await;
    let foreign = first.sign(RELAY_AUTH_DOMAIN, &client.binding(space).auth_payload(1));
    assert_eq!(
        refusal(
            &other
                .send(&RelayRequest::Auth {
                    epoch: 1,
                    signature: encode_relay_bytes(&foreign),
                })
                .await
        ),
        Some(RelayRefusalCode::Authentication)
    );

    assert_eq!(
        client.auth(space, 1, &first).await,
        RelayResponse::Authenticated {
            epoch: 1,
            current_epoch: 1,
            stale: false
        }
    );
    let writer = writer_id(8);
    let bytes = record(space, writer, 1, 1, 5, 40);
    assert_eq!(
        client
            .send(&RelayRequest::Push {
                records: vec![encode_relay_bytes(&bytes)],
            })
            .await,
        RelayResponse::Pushed {
            outcomes: vec![RelayPushOutcome::Stored]
        }
    );
    let wrong_space = record(space_id(9), writer, 2, 1, 5, 40);
    assert_eq!(
        refusal(
            &client
                .send(&RelayRequest::Push {
                    records: vec![encode_relay_bytes(&wrong_space)],
                })
                .await
        ),
        Some(RelayRefusalCode::BadRecord)
    );

    // Rotation: only the holder of the next key registers it.
    let second_key = access(&space, 2);
    let imposter = access(&space, 2);
    let entry = encode_relay_bytes(b"key epoch entry");
    let forged = client
        .send(&RelayRequest::RegisterEpoch {
            epoch: 2,
            access_key: encode_relay_bytes(second_key.public_key().as_bytes()),
            entry: entry.clone(),
            signature: encode_relay_bytes(
                &imposter.sign(RELAY_EPOCH_DOMAIN, &client.binding(space).epoch_payload(2)),
            ),
        })
        .await;
    assert_eq!(refusal(&forged), Some(RelayRefusalCode::Authentication));
    let mut client = Client::connect(&relay).await;
    client.hello(space).await;
    client.auth(space, 1, &first).await;
    let skipped = access(&space, 3);
    assert_eq!(
        refusal(
            &client
                .send(&RelayRequest::RegisterEpoch {
                    epoch: 3,
                    access_key: encode_relay_bytes(skipped.public_key().as_bytes()),
                    entry: entry.clone(),
                    signature: encode_relay_bytes(
                        &skipped.sign(RELAY_EPOCH_DOMAIN, &client.binding(space).epoch_payload(3)),
                    ),
                })
                .await
        ),
        Some(RelayRefusalCode::UnknownEpoch)
    );
    assert_eq!(
        client
            .send(&RelayRequest::RegisterEpoch {
                epoch: 2,
                access_key: encode_relay_bytes(second_key.public_key().as_bytes()),
                entry: entry.clone(),
                signature: encode_relay_bytes(
                    &second_key.sign(RELAY_EPOCH_DOMAIN, &client.binding(space).epoch_payload(2)),
                ),
            })
            .await,
        RelayResponse::EpochRegistered { epoch: 2 }
    );
    // The epoch 1 session is now stale: it cannot push.
    let late = record(space, writer, 2, 1, 5, 40);
    assert_eq!(
        refusal(
            &client
                .send(&RelayRequest::Push {
                    records: vec![encode_relay_bytes(&late)],
                })
                .await
        ),
        Some(RelayRefusalCode::StaleEpoch)
    );

    // The new epoch pushes a record the removed device must not read.
    let mut current = Client::connect(&relay).await;
    current.hello(space).await;
    assert_eq!(
        current.auth(space, 2, &second_key).await,
        RelayResponse::Authenticated {
            epoch: 2,
            current_epoch: 2,
            stale: false
        }
    );
    let after = record(space, writer, 2, 2, 6, 40);
    current
        .send(&RelayRequest::Push {
            records: vec![encode_relay_bytes(&after)],
        })
        .await;

    let mut removed = Client::connect(&relay).await;
    removed.hello(space).await;
    assert_eq!(
        removed.auth(space, 1, &first).await,
        RelayResponse::Authenticated {
            epoch: 1,
            current_epoch: 2,
            stale: true
        }
    );
    let RelayResponse::Records { records, .. } =
        removed.send(&RelayRequest::Pull { have: Vec::new() }).await
    else {
        panic!("expected records");
    };
    assert_eq!(records, vec![encode_relay_bytes(&bytes)]);
    let RelayResponse::Epochs {
        entries,
        current_epoch,
        ..
    } = removed.send(&RelayRequest::Epochs { after: 1 }).await
    else {
        panic!("expected epochs");
    };
    assert_eq!(current_epoch, 2);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        decode_relay_bytes("entry", &entries[0].entry, 64).unwrap(),
        b"key epoch entry"
    );
    let removed_push = record(space, writer, 3, 1, 5, 40);
    assert_eq!(
        refusal(
            &removed
                .send(&RelayRequest::Push {
                    records: vec![encode_relay_bytes(&removed_push)],
                })
                .await
        ),
        Some(RelayRefusalCode::StaleEpoch)
    );

    let RelayResponse::Records { records, .. } = current
        .send(&RelayRequest::Pull {
            have: vec![SyncSeq {
                writer: writer.to_hex(),
                seq: 1,
            }],
        })
        .await
    else {
        panic!("expected records");
    };
    assert_eq!(records, vec![encode_relay_bytes(&after)]);

    // Unknown epochs and unknown spaces are refused before any signature check.
    let mut probe = Client::connect(&relay).await;
    probe.hello(space).await;
    assert_eq!(
        refusal(&probe.auth(space, 3, &skipped).await),
        Some(RelayRefusalCode::UnknownEpoch)
    );
    let mut probe = Client::connect(&relay).await;
    probe.hello(space_id(7)).await;
    assert_eq!(
        refusal(&probe.auth(space_id(7), 1, &first).await),
        Some(RelayRefusalCode::UnknownSpace)
    );
}

#[tokio::test]
async fn stale_epoch_credentials_below_the_claim_epoch_are_refused() {
    let dir = TempDir::new();
    let space = space_id(1);
    let store = open_store(&dir, RetentionPolicy::default()).await;
    let token = RelaySetupToken::generate().unwrap();
    store.add_setup_token(&token, DAY_MS).await.unwrap();
    let owner = admin();
    let admins = [owner.public_key()];
    store
        .claim(
            &ClaimRequest {
                space,
                token: &token,
                epoch: 4,
                access_key: access(&space, 4).public_key(),
                admins: &admins,
                counter: 0,
            },
            1,
        )
        .await
        .unwrap();
    assert_eq!(
        code_of(store.credential(&space, 3).await),
        RelayRefusalCode::StaleEpoch
    );
    assert_eq!(
        code_of(store.credential(&space, 5).await),
        RelayRefusalCode::UnknownEpoch
    );
    assert_eq!(store.credential(&space, 4).await.unwrap().current_epoch, 4);
}

impl Client {
    async fn register(
        &mut self,
        space: SpaceId,
        epoch: u32,
        key: &RelayAccessKeyPair,
    ) -> RelayResponse {
        let signature = key.sign(
            RELAY_EPOCH_DOMAIN,
            &self.binding(space).epoch_payload(epoch),
        );
        self.send(&RelayRequest::RegisterEpoch {
            epoch,
            access_key: encode_relay_bytes(key.public_key().as_bytes()),
            entry: encode_relay_bytes(format!("entry {epoch}").as_bytes()),
            signature: encode_relay_bytes(&signature),
        })
        .await
    }

    async fn repair(
        &mut self,
        space: SpaceId,
        signer: &AdminKeyPair,
        counter: u64,
        epoch: u32,
        key: &RelayAccessKeyPair,
    ) -> RelayResponse {
        let access_key = key.public_key();
        let payload = self.binding(space).admin_payload(
            counter,
            &RelayAdminCommand::RelayEpochRepair {
                epoch,
                access_key: &access_key,
            },
        );
        self.send(&RelayRequest::Admin {
            action: RelayAdminAction::RelayEpochRepair {
                epoch,
                access_key: encode_relay_bytes(access_key.as_bytes()),
            },
            counter,
            signer: encode_relay_bytes(signer.public_key().as_bytes()),
            signature: encode_relay_bytes(&signer.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
        .await
    }

    async fn push(&mut self, bytes: &[u8]) -> RelayResponse {
        self.send(&RelayRequest::Push {
            records: vec![encode_relay_bytes(bytes)],
        })
        .await
    }
}

#[tokio::test]
async fn an_admin_repairs_a_hijacked_epoch_registration_over_tls() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let owner = admin();
    let first = access(&space, 1);
    let token = issue_token(&relay).await;
    let stored = RelayResponse::Pushed {
        outcomes: vec![RelayPushOutcome::Stored],
    };

    let mut honest = Client::connect(&relay).await;
    honest.hello(space).await;
    assert_eq!(
        honest.claim(space, &token, &first, &owner).await,
        RelayResponse::Claimed { epoch: 1 }
    );
    honest.auth(space, 1, &first).await;
    let writer = writer_id(1);
    assert_eq!(
        honest.push(&record(space, writer, 1, 1, 1, 32)).await,
        stored
    );

    // An attacker holding epoch 1 registers keys of its own for epochs 2 and 3 and writes there.
    let mut attacker = Client::connect(&relay).await;
    attacker.hello(space).await;
    attacker.auth(space, 1, &first).await;
    let fake_second = access(&space, 2);
    let fake_third = access(&space, 3);
    assert_eq!(
        attacker.register(space, 2, &fake_second).await,
        RelayResponse::EpochRegistered { epoch: 2 }
    );
    let mut attacker = Client::connect(&relay).await;
    attacker.hello(space).await;
    attacker.auth(space, 2, &fake_second).await;
    assert_eq!(
        attacker.push(&record(space, writer, 2, 2, 9, 32)).await,
        stored
    );
    attacker.register(space, 3, &fake_third).await;
    let mut squatter = Client::connect(&relay).await;
    squatter.hello(space).await;
    squatter.auth(space, 3, &fake_third).await;
    assert_eq!(
        squatter
            .push(&record(space, writer_id(2), 1, 3, 9, 32))
            .await,
        stored
    );

    // The honest member's real epoch 2 key is locked out.
    let second = access(&space, 2);
    assert_eq!(
        refusal(&honest.register(space, 2, &second).await),
        Some(RelayRefusalCode::StaleEpoch)
    );
    let mut locked = Client::connect(&relay).await;
    locked.hello(space).await;
    assert_eq!(
        refusal(&locked.auth(space, 2, &second).await),
        Some(RelayRefusalCode::Authentication)
    );

    // An admin repairs without epoch authentication, learning the counter from a refusal.
    let mut repair = Client::connect(&relay).await;
    repair.hello(space).await;
    let stale = repair.repair(space, &owner, 1, 2, &second).await;
    let RelayResponse::Refused {
        code: RelayRefusalCode::AdminCounter,
        next_counter: Some(next),
        ..
    } = stale
    else {
        panic!("expected a stale counter refusal, got {stale:?}");
    };
    assert_eq!(next, 2);
    assert_eq!(
        repair.repair(space, &owner, next, 2, &second).await,
        RelayResponse::AdminApplied { counter: next }
    );
    assert_eq!(
        refusal(&repair.repair(space, &owner, next, 2, &second).await),
        Some(RelayRefusalCode::AdminCounter)
    );

    // Sessions opened with the replaced keys must authenticate again.
    assert_eq!(
        refusal(
            &squatter
                .push(&record(space, writer_id(2), 2, 3, 9, 32))
                .await
        ),
        Some(RelayRefusalCode::Authentication)
    );

    let mut honest = Client::connect(&relay).await;
    honest.hello(space).await;
    assert_eq!(
        honest.auth(space, 2, &second).await,
        RelayResponse::Authenticated {
            epoch: 2,
            current_epoch: 2,
            stale: false
        }
    );
    assert_eq!(
        honest.push(&record(space, writer, 2, 2, 2, 32)).await,
        stored
    );

    // The attacker's epoch 1 credential is stale, and its own keys are refused.
    let mut attacker = Client::connect(&relay).await;
    attacker.hello(space).await;
    attacker.auth(space, 1, &first).await;
    assert_eq!(
        refusal(&attacker.push(&record(space, writer, 3, 1, 9, 32)).await),
        Some(RelayRefusalCode::StaleEpoch)
    );
    for (epoch, key, code) in [
        (2, &fake_second, RelayRefusalCode::Authentication),
        (3, &fake_third, RelayRefusalCode::UnknownEpoch),
    ] {
        let mut attacker = Client::connect(&relay).await;
        attacker.hello(space).await;
        assert_eq!(refusal(&attacker.auth(space, epoch, key).await), Some(code));
    }

    // Nothing above epoch 2 remains.
    let state = relay.store.state(&space).await.unwrap();
    assert_eq!(state.current_epoch, 2);
    assert_eq!(ranges(&state), vec![(writer, 1, 3)]);
    assert_eq!(
        code_of(relay.store.credential(&space, 3).await),
        RelayRefusalCode::UnknownEpoch
    );
    assert!(
        relay
            .store
            .epochs(&space, 0)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
}

#[tokio::test]
async fn signed_admin_commands_change_the_admin_set_and_refuse_replays_over_tls() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let owner = admin();
    let second = admin();
    let token = issue_token(&relay).await;
    let mut client = Client::connect(&relay).await;
    client.hello(space).await;
    client
        .claim(space, &token, &access(&space, 1), &owner)
        .await;
    // The claim used counter 1, so the next command needs a higher one.
    assert_eq!(
        refusal(
            &client
                .admin(space, &owner, 1, &second.public_key(), true)
                .await
        ),
        Some(RelayRefusalCode::AdminCounter)
    );
    assert_eq!(
        client
            .admin(space, &owner, 2, &second.public_key(), true)
            .await,
        RelayResponse::AdminApplied { counter: 2 }
    );
    // A command signed by a key outside the set is refused.
    let outsider = admin();
    assert_eq!(
        refusal(
            &client
                .admin(space, &outsider, 3, &outsider.public_key(), true)
                .await
        ),
        Some(RelayRefusalCode::AdminSignature)
    );
    // A signature from another connection does not apply here.
    let mut other = Client::connect(&relay).await;
    other.hello(space).await;
    let payload = client
        .binding(space)
        .admin_payload(3, &RelayAdminCommand::RemoveKey(&owner.public_key()));
    let replayed = other
        .send(&RelayRequest::Admin {
            action: RelayAdminAction::RemoveKey {
                key: encode_relay_bytes(owner.public_key().as_bytes()),
            },
            counter: 3,
            signer: encode_relay_bytes(second.public_key().as_bytes()),
            signature: encode_relay_bytes(&second.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
        .await;
    assert_eq!(refusal(&replayed), Some(RelayRefusalCode::AdminSignature));
    assert_eq!(
        client
            .admin(space, &second, 3, &owner.public_key(), false)
            .await,
        RelayResponse::AdminApplied { counter: 3 }
    );
    assert_eq!(
        relay.store.admins(&space).await.unwrap(),
        vec![second.public_key()]
    );
}

#[tokio::test]
async fn assets_round_trip_over_tls() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let key = access(&space, 1);
    let token = issue_token(&relay).await;
    let mut client = Client::connect(&relay).await;
    client.hello(space).await;
    client.claim(space, &token, &key, &admin()).await;
    client.auth(space, 1, &key).await;
    let bytes = chunk(space, [5; 32], 1, 0, 1, 4);
    assert_eq!(
        client
            .send(&RelayRequest::AssetPut {
                chunk: encode_relay_bytes(&bytes),
            })
            .await,
        RelayResponse::AssetStored {
            outcome: RelayAssetOutcome::Stored
        }
    );
    assert_eq!(
        client
            .send(&RelayRequest::AssetQuery {
                assets: vec![encode_relay_asset(&[5; 32]), encode_relay_asset(&[6; 32])],
            })
            .await,
        RelayResponse::AssetPresence {
            assets: vec![encode_relay_asset(&[5; 32])]
        }
    );
    assert_eq!(
        client
            .send(&RelayRequest::AssetGet {
                asset: encode_relay_asset(&[5; 32]),
                index: 0,
            })
            .await,
        RelayResponse::AssetChunk {
            chunk: encode_relay_bytes(&bytes)
        }
    );
}

#[tokio::test]
async fn a_wait_wakes_when_a_record_arrives() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let key = access(&space, 1);
    let token = issue_token(&relay).await;
    let mut waiter = Client::connect(&relay).await;
    waiter.hello(space).await;
    waiter.claim(space, &token, &key, &admin()).await;
    waiter.auth(space, 1, &key).await;
    let mut pusher = Client::connect(&relay).await;
    pusher.hello(space).await;
    pusher.auth(space, 1, &key).await;
    write_frame(&mut waiter.stream, &RelayRequest::Wait { have: Vec::new() })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let bytes = record(space, writer_id(2), 1, 1, 0, 40);
    pusher
        .send(&RelayRequest::Push {
            records: vec![encode_relay_bytes(&bytes)],
        })
        .await;
    let response: RelayResponse = tokio::time::timeout(
        Duration::from_secs(5),
        read_frame(&mut waiter.stream, MAX_CONTROL_BYTES),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response, RelayResponse::Waited { ready: true });
}

#[tokio::test]
async fn a_stale_session_gets_no_state() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let first = access(&space, 1);
    let token = issue_token(&relay).await;
    let mut rotating = Client::connect(&relay).await;
    rotating.hello(space).await;
    rotating.claim(space, &token, &first, &admin()).await;
    rotating.auth(space, 1, &first).await;
    let writer = writer_id(4);
    let sealed = record(space, writer, 1, 1, 5, 40);
    assert_eq!(
        rotating.push(&sealed).await,
        RelayResponse::Pushed {
            outcomes: vec![RelayPushOutcome::Stored]
        }
    );
    let mut stale = Client::connect(&relay).await;
    stale.hello(space).await;
    stale.auth(space, 1, &first).await;
    assert!(matches!(
        stale.send(&RelayRequest::State).await,
        RelayResponse::State {
            current_epoch: 1,
            ..
        }
    ));
    assert_eq!(
        rotating.register(space, 2, &access(&space, 2)).await,
        RelayResponse::EpochRegistered { epoch: 2 }
    );

    // A rotation makes the open session stale: it no longer sees writer ranges or admin keys.
    assert_eq!(
        refusal(&stale.send(&RelayRequest::State).await),
        Some(RelayRefusalCode::StaleEpoch)
    );
    // A fresh session at the old epoch is stale from the start.
    let mut removed = Client::connect(&relay).await;
    removed.hello(space).await;
    assert_eq!(
        removed.auth(space, 1, &first).await,
        RelayResponse::Authenticated {
            epoch: 1,
            current_epoch: 2,
            stale: true
        }
    );
    assert_eq!(
        refusal(&removed.send(&RelayRequest::State).await),
        Some(RelayRefusalCode::StaleEpoch)
    );

    // A lagging member still catches up through Pull, Epochs, and Wait.
    let RelayResponse::Records { records, .. } =
        removed.send(&RelayRequest::Pull { have: Vec::new() }).await
    else {
        panic!("expected records");
    };
    assert_eq!(records, vec![encode_relay_bytes(&sealed)]);
    let RelayResponse::Epochs {
        current_epoch,
        entries,
        ..
    } = removed.send(&RelayRequest::Epochs { after: 1 }).await
    else {
        panic!("expected epochs");
    };
    assert_eq!((current_epoch, entries.len()), (2, 1));
    assert_eq!(
        removed.send(&RelayRequest::Wait { have: Vec::new() }).await,
        RelayResponse::Waited { ready: true }
    );
    // The session stays usable after the refusal.
    assert_eq!(
        refusal(&removed.send(&RelayRequest::State).await),
        Some(RelayRefusalCode::StaleEpoch)
    );
}

#[tokio::test]
async fn oversized_frames_before_authentication_close_the_connection() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let mut client = Client::connect(&relay).await;
    let length = u32::try_from(MAX_UNAUTHENTICATED_CONTROL_BYTES + 1).unwrap();
    client
        .stream
        .write_all(&length.to_be_bytes())
        .await
        .unwrap();
    client.stream.flush().await.unwrap();
    let response: RelayResponse = read_frame(&mut client.stream, MAX_CONTROL_BYTES)
        .await
        .unwrap();
    assert_eq!(refusal(&response), Some(RelayRefusalCode::BadRequest));
    let mut rest = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), client.stream.read_to_end(&mut rest))
        .await
        .unwrap();
    assert!(read.is_err() || rest.is_empty());
}

#[tokio::test]
async fn pending_connection_budgets_close_extra_connections_and_free_slots_on_close() {
    let relay = start_relay(ConnectionLimits {
        pending: 4,
        pending_per_source: 2,
        authenticated: 4,
        authenticated_per_source: 4,
        ..ConnectionLimits::default()
    })
    .await;
    let first = Client::connect(&relay).await;
    let _second = Client::connect(&relay).await;
    // A third connection from the same source is closed before the handshake completes.
    let socket = TcpStream::connect(relay.address).await.unwrap();
    let verifier = Arc::new(PinnedVerifier {
        fingerprint: relay.fingerprint.clone(),
        algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
    });
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .dangerous()
    .with_custom_certificate_verifier(verifier)
    .with_no_client_auth();
    let refused = tokio::time::timeout(
        Duration::from_secs(5),
        TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("ganbaru-relay.local").unwrap(), socket),
    )
    .await
    .unwrap();
    assert!(refused.is_err());
    drop(first);
    // The slot frees once the server notices the close.
    let mut connected = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let socket = TcpStream::connect(relay.address).await.unwrap();
        let verifier = Arc::new(PinnedVerifier {
            fingerprint: relay.fingerprint.clone(),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        });
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
        if TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("ganbaru-relay.local").unwrap(), socket)
            .await
            .is_ok()
        {
            connected = true;
            break;
        }
    }
    assert!(connected);
}

#[tokio::test]
async fn authentication_moves_a_connection_to_the_authenticated_budget() {
    let relay = start_relay(ConnectionLimits {
        pending: 4,
        pending_per_source: 1,
        authenticated: 1,
        authenticated_per_source: 1,
        ..ConnectionLimits::default()
    })
    .await;
    let space = space_id(1);
    let key = access(&space, 1);
    relay
        .store
        .add_setup_token(
            &RelaySetupToken::from_bytes([1; 32]),
            unix_time_ms() + DAY_MS,
        )
        .await
        .unwrap();
    let mut first = Client::connect(&relay).await;
    first.hello(space).await;
    first
        .claim(space, &RelaySetupToken::from_bytes([1; 32]), &key, &admin())
        .await;
    first.auth(space, 1, &key).await;
    // The pending slot is free again, so a second connection gets through the handshake, but
    // the authenticated budget is full.
    let mut second = Client::connect(&relay).await;
    second.hello(space).await;
    assert_eq!(
        refusal(&second.auth(space, 1, &key).await),
        Some(RelayRefusalCode::Busy)
    );
}

#[tokio::test]
async fn authenticated_requests_wait_for_frame_memory_then_get_busy() {
    let relay = start_relay(ConnectionLimits {
        // Room for one request reservation, not two.
        frame_memory_bytes: MAX_CONTROL_BYTES + MAX_CONTROL_BYTES / 2,
        frame_memory_wait: Duration::from_millis(200),
        ..ConnectionLimits::default()
    })
    .await;
    let space = space_id(1);
    let key = access(&space, 1);
    let token = issue_token(&relay).await;
    let mut holder = Client::connect(&relay).await;
    holder.hello(space).await;
    holder.claim(space, &token, &key, &admin()).await;
    assert_eq!(refusal(&holder.auth(space, 1, &key).await), None);
    let mut other = Client::connect(&relay).await;
    other.hello(space).await;
    assert_eq!(refusal(&other.auth(space, 1, &key).await), None);
    assert_eq!(refusal(&other.send(&RelayRequest::State).await), None);

    // A length prefix without its body holds a reservation while the body is awaited.
    holder.stream.write_all(&64u32.to_be_bytes()).await.unwrap();
    holder.stream.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        refusal(&other.send(&RelayRequest::State).await),
        Some(RelayRefusalCode::Busy)
    );

    // Once the holder disconnects, its memory returns to the budget.
    drop(holder);
    let mut served = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut retry = Client::connect(&relay).await;
        retry.hello(space).await;
        retry.auth(space, 1, &key).await;
        if refusal(&retry.send(&RelayRequest::State).await).is_none() {
            served = true;
            break;
        }
    }
    assert!(served);
}

#[tokio::test]
async fn operators_list_and_remove_spaces_and_revoke_setup_tokens() {
    let dir = TempDir::new();
    // Commands refuse a directory without a relay database instead of creating one.
    assert!(crate::list_spaces(dir.path()).await.is_err());
    let store = open_store(&dir, RetentionPolicy::default()).await;
    let first = RelaySetupToken::from_bytes([1; 32]);
    let second = RelaySetupToken::from_bytes([2; 32]);
    let third = RelaySetupToken::from_bytes([3; 32]);
    for token in [&first, &second, &third] {
        store
            .add_setup_token(token, unix_time_ms() + DAY_MS)
            .await
            .unwrap();
    }
    assert!(store.revoke_setup_token(&first).await.unwrap());
    assert!(!store.revoke_setup_token(&first).await.unwrap());
    let space = space_id(4);
    let key = access(&space, 1);
    let owner = admin();
    let claimed = claim_with(&store, space, &first, &key, &owner).await;
    assert_eq!(code_of(claimed), RelayRefusalCode::Token);
    claim_with(&store, space, &second, &key, &owner)
        .await
        .unwrap();
    assert_eq!(store.revoke_all_setup_tokens().await.unwrap(), 1);
    let listed = store.spaces().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].space, space);
    assert_eq!(listed[0].current_epoch, 1);
    store.close().await;
    assert!(
        crate::list_spaces(dir.path())
            .await
            .unwrap()
            .contains(&space.to_hex())
    );

    // A serving relay holds the data directory lock, so removal waits for it to stop.
    let lock = crate::lock_data_dir(dir.path()).unwrap();
    let remove = crate::config::RemoveSpaceConfig {
        data_dir: dir.path().to_path_buf(),
        space,
    };
    assert!(crate::remove_space(remove.clone()).await.is_err());
    drop(lock);
    crate::remove_space(remove.clone()).await.unwrap();
    assert!(crate::remove_space(remove).await.is_err());
    let store = open_store(&dir, RetentionPolicy::default()).await;
    assert!(store.spaces().await.unwrap().is_empty());
    assert_eq!(
        code_of(store.state(&space).await),
        RelayRefusalCode::UnknownSpace
    );
}

impl Client {
    async fn remove_space(
        &mut self,
        space: SpaceId,
        signer: &AdminKeyPair,
        counter: u64,
    ) -> RelayResponse {
        let payload = self
            .binding(space)
            .admin_payload(counter, &RelayAdminCommand::RemoveSpace);
        self.send(&RelayRequest::Admin {
            action: RelayAdminAction::RemoveSpace,
            counter,
            signer: encode_relay_bytes(signer.public_key().as_bytes()),
            signature: encode_relay_bytes(&signer.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
        .await
    }
}

#[tokio::test]
async fn unknown_space_leaves_no_generation_entry() {
    // Every probe checks the map stays empty, so a few probes over each refusal path suffice.
    const PROBES: u16 = 8;
    // Each probe holds its connection open, so the budget must fit all of them at once.
    let open = usize::from(PROBES) + 1;
    let relay = start_relay(ConnectionLimits {
        pending: open,
        pending_per_source: open,
        ..ConnectionLimits::default()
    })
    .await;
    let signer = admin();
    let mut clients = Vec::new();
    for probe in 0..PROBES {
        let mut bytes = [0xA5; 16];
        bytes[..2].copy_from_slice(&probe.to_le_bytes());
        let space = SpaceId::from_bytes(bytes);
        let mut client = Client::connect(&relay).await;
        client.hello(space).await;
        if probe % 2 == 0 {
            let response = client.auth(space, 1, &access(&space, 1)).await;
            assert_eq!(refusal(&response), Some(RelayRefusalCode::UnknownSpace));
        } else {
            // Both admin paths answer like an unknown signer on a hosted space.
            let response = if probe % 4 == 1 {
                client
                    .admin(space, &signer, 1, &signer.public_key(), true)
                    .await
            } else {
                client.remove_space(space, &signer, 1).await
            };
            assert_eq!(refusal(&response), Some(RelayRefusalCode::AdminSignature));
        }
        assert_eq!(relay.probe.generations(), 0, "probe {probe}");
        clients.push(client);
    }
    assert_eq!(relay.probe.waits(), 0);
}

#[tokio::test]
async fn a_recreated_space_never_repeats_a_generation() {
    let relay = start_relay(ConnectionLimits::default()).await;
    let space = space_id(1);
    let owner = admin();
    let key = access(&space, 1);
    let token = issue_token(&relay).await;
    let mut operator = Client::connect(&relay).await;
    operator.hello(space).await;
    assert_eq!(
        operator.claim(space, &token, &key, &owner).await,
        RelayResponse::Claimed { epoch: 1 }
    );
    // A repair advances the generation before the session below verifies.
    assert_eq!(
        operator.repair(space, &owner, 2, 1, &key).await,
        RelayResponse::AdminApplied { counter: 2 }
    );
    let mut old = Client::connect(&relay).await;
    old.hello(space).await;
    assert!(matches!(
        old.auth(space, 1, &key).await,
        RelayResponse::Authenticated { .. }
    ));
    let mut pusher = Client::connect(&relay).await;
    pusher.hello(space).await;
    pusher.auth(space, 1, &key).await;
    write_frame(&mut old.stream, &RelayRequest::Wait { have: Vec::new() })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    pusher.push(&record(space, writer_id(2), 1, 1, 0, 40)).await;
    let woke: RelayResponse = tokio::time::timeout(
        Duration::from_secs(5),
        read_frame(&mut old.stream, MAX_CONTROL_BYTES),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(woke, RelayResponse::Waited { ready: true });
    assert_eq!(relay.probe.generations(), 1);
    assert_eq!(relay.probe.waits(), 1);

    assert_eq!(
        operator.remove_space(space, &owner, 3).await,
        RelayResponse::AdminApplied { counter: 3 }
    );
    assert_eq!(relay.probe.generations(), 0);
    assert_eq!(relay.probe.waits(), 0);

    let token = issue_token(&relay).await;
    let mut claimer = Client::connect(&relay).await;
    claimer.hello(space).await;
    assert_eq!(
        claimer.claim(space, &token, &key, &owner).await,
        RelayResponse::Claimed { epoch: 1 }
    );
    let mut fresh = Client::connect(&relay).await;
    fresh.hello(space).await;
    assert!(matches!(
        fresh.auth(space, 1, &key).await,
        RelayResponse::Authenticated { .. }
    ));
    assert!(matches!(
        fresh.send(&RelayRequest::State).await,
        RelayResponse::State { .. }
    ));
    // The session from before the removal verified against the removed space.
    assert_eq!(
        refusal(&old.send(&RelayRequest::State).await),
        Some(RelayRefusalCode::Authentication)
    );
    assert_eq!(relay.probe.generations(), 1);
}
