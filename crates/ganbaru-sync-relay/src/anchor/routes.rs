//! What an anchor answers besides sync: the setup conversation, and snapshot copies for devices
//! that bootstrap from it.
//!
//! Setup is reachable without a client certificate. Nothing happens before the peer proves the
//! one-time secret, and a wrong proof costs the peer the connection, not the secret.

use super::activation::{Upload, activate};
use super::host::AnchorHost;
use crate::server::unix_time_ms;
use ganbaru_handoff::pairing::{PinnedCharter, certificate_fingerprint};
use ganbaru_handoff::protocol::anchor::{self, AnchorSpace, SETUP_REFUSED_CODE};
use ganbaru_handoff::protocol::{
    self, BundleMetadata, ControlMessage, encode_sync_admin_key, encode_sync_charter,
    encode_sync_device_key, read_control_limited, relay, validate_identifier, write_control,
};
use ganbaru_sync::{Engine, local};
use ganbaru_sync_replica::anchor_link::local_compatibility;
use ganbaru_sync_replica::authority::ensure_admin_key;
use ganbaru_sync_replica::hub::SyncHub;
use ganbaru_sync_replica::serve::bundles::MemberBundles;
use ganbaru_sync_replica::serve::{Peer, Routes, ServerStream, send_error, timeout_control};
use ganbaru_sync_replica::service::{SyncHost, blocking};
use ganbaru_sync_replica::writer::ensure_device_key;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;

/// What the server calls itself in the device list.
const DEVICE_LABEL: &str = "Ganbaru server";
/// How long one chunk of an uploaded snapshot may take.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
const UPLOAD_BUSY_CODE: &str = "upload_busy";
const UPLOAD_REFUSED_CODE: &str = "upload_refused";

/// Setup state shared by every connection.
pub struct SetupRoute {
    pub host: AnchorHost,
    pub hub: SyncHub,
    pub commits: Arc<Notify>,
    pub version: &'static str,
    /// Held for one upload, which has fixed staging paths.
    pub upload: tokio::sync::Mutex<()>,
}

/// The routes of an anchor listener.
#[derive(Clone)]
pub struct AnchorRoutes {
    setup: Arc<SetupRoute>,
    bundles: MemberBundles,
}

impl AnchorRoutes {
    pub fn new(setup: Arc<SetupRoute>, bundles: MemberBundles) -> Self {
        Self { setup, bundles }
    }
}

impl Routes for AnchorRoutes {
    fn handles(&self, request: &ControlMessage) -> bool {
        matches!(request, ControlMessage::AnchorHello { .. }) || MemberBundles::handles(request)
    }

    async fn serve(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
    ) -> Result<(), String> {
        if let ControlMessage::AnchorHello {
            compatibility,
            client_nonce,
            ..
        } = request
        {
            let enrolled = peer.certificate.is_some();
            return self
                .setup
                .converse(stream, enrolled, &compatibility, &client_nonce)
                .await;
        }
        let (Some(admission), Some(certificate)) = (peer.admission, peer.certificate) else {
            return Err("a snapshot copy needs an enrolled device".to_string());
        };
        let fingerprint = certificate_fingerprint(certificate.as_ref());
        self.bundles
            .serve(stream, peer.manager, admission, &fingerprint, request)
            .await
    }
}

impl SetupRoute {
    /// Answers a setup greeting. An enrolled device that greets is asking which protocol this
    /// server speaks, so it gets the challenge alone, as does a peer that cannot continue.
    async fn converse(
        &self,
        stream: &mut ServerStream,
        enrolled: bool,
        compatibility: &protocol::HandoffCompatibility,
        client_nonce: &str,
    ) -> Result<(), String> {
        let local = local_compatibility(self.version);
        let server_nonce = anchor::anchor_nonce()?;
        timeout_control(write_control(
            stream,
            &ControlMessage::AnchorChallenge {
                protocol_version: protocol::PROTOCOL_VERSION,
                compatibility: local.clone(),
                server_nonce: relay::encode_relay_bytes(&server_nonce),
            },
        ))
        .await?;
        if enrolled || protocol::ensure_compatible(&local, compatibility).is_err() {
            return Ok(());
        }
        let client_nonce = anchor::decode_anchor_nonce(client_nonce)?;
        let frame = read_control_limited(stream, protocol::MAX_UNAUTHENTICATED_CONTROL_BYTES);
        let (proof, vault_id) = match timeout_control(frame).await {
            Ok(ControlMessage::AnchorProof { proof, vault_id }) => (proof, vault_id),
            // The peer read the challenge and stopped, as a device of another protocol does.
            Ok(_) | Err(_) => return Ok(()),
        };
        if !self.proof_holds(&server_nonce, &client_nonce, &proof)? {
            return refuse_setup(stream, "this setup code is wrong, used, or expired").await;
        }
        let vault_id = match self.proven_vault(vault_id)? {
            Ok(vault_id) => vault_id,
            Err(message) => return refuse_setup(stream, message).await,
        };
        let identity = self.identity(&vault_id).await?;
        timeout_control(write_control(stream, &identity)).await?;
        self.after_proof(stream, &vault_id).await
    }

    fn proof_holds(
        &self,
        server_nonce: &[u8; anchor::ANCHOR_NONCE_BYTES],
        client_nonce: &[u8; anchor::ANCHOR_NONCE_BYTES],
        proof: &str,
    ) -> Result<bool, String> {
        let Ok(proof) = anchor::decode_anchor_proof(proof) else {
            return Ok(false);
        };
        let Some(secret) =
            super::state::live_setup_secret(self.host.paths(), unix_time_ms())?
        else {
            return Ok(false);
        };
        let fingerprint = self.host.pairing().certificate().map(|certificate| {
            let digest: [u8; 32] = Sha256::digest(certificate.as_ref()).into();
            digest
        })?;
        Ok(anchor::verify_anchor_setup_proof(
            &secret,
            server_nonce,
            client_nonce,
            &fingerprint,
            &proof,
        ))
    }

    /// The vault a proven peer talks about, or why it cannot be served.
    fn proven_vault(
        &self,
        asked: Option<String>,
    ) -> Result<Result<String, &'static str>, String> {
        let active = self.host.activation()?;
        Ok(match (asked, active) {
            (Some(asked), Some(active)) if asked != active.vault_id => {
                Err("this server already runs another vault")
            }
            (Some(asked), _) => match validate_identifier("vault id", &asked) {
                Ok(()) => Ok(asked),
                Err(_) => Err("this vault id is invalid"),
            },
            (None, Some(active)) => Ok(active.vault_id),
            (None, None) => Err("this server runs no vault yet"),
        })
    }

    /// What the server enrolls with, creating its keys for `vault_id` on first use.
    async fn identity(&self, vault_id: &str) -> Result<ControlMessage, String> {
        let keys = self.host.keys();
        let owned_vault_id = vault_id.to_string();
        let (device_key, admin_key) = blocking(move || {
            let device = ensure_device_key(&keys, &owned_vault_id)?;
            let admin = ensure_admin_key(&keys, &owned_vault_id)?;
            Ok((device.public_key(), admin.public_key()))
        })
        .await?;
        Ok(ControlMessage::AnchorIdentity {
            device_id: self.host.own_device_id().to_string(),
            device_label: DEVICE_LABEL.to_string(),
            device_key: encode_sync_device_key(&device_key),
            admin_key: encode_sync_admin_key(&admin_key),
            space: self.active_space().await?,
        })
    }

    /// The space this server runs, `None` before activation.
    async fn active_space(&self) -> Result<Option<AnchorSpace>, String> {
        let Some(active) = self.host.activation()? else {
            return Ok(None);
        };
        let pool = self.host.connect_for_sync().await?;
        let mut conn = pool
            .acquire()
            .await
            .map_err(|error| format!("open the vault: {error}"))?;
        let ctx = local::space_context(&mut conn, &active.vault_id)
            .await
            .map_err(|error| format!("read sync space: {error}"))?
            .ok_or_else(|| "this server has no sync space".to_string())?;
        let charter = Engine::vault()
            .charter(&mut conn, &ctx)
            .await
            .map_err(|error| format!("read sync charter: {error}"))?
            .ok_or_else(|| "this server has no charter".to_string())?;
        Ok(Some(AnchorSpace {
            vault_id: active.vault_id,
            charter: encode_sync_charter(&charter),
            person_key: ctx.person_key.to_text(),
        }))
    }

    /// Serves the request that follows a proof, if any: a device often closes after it learned
    /// the server's keys.
    async fn after_proof(&self, stream: &mut ServerStream, vault_id: &str) -> Result<(), String> {
        let frame = read_control_limited(stream, protocol::MAX_CONTROL_BYTES);
        match timeout_control(frame).await {
            Ok(ControlMessage::AnchorUpload {
                metadata,
                charter,
                person_key,
                take_anchor,
            }) => {
                let charter = PinnedCharter {
                    charter,
                    person_key,
                };
                self.upload(stream, vault_id, metadata, charter, take_anchor)
                    .await
            }
            Ok(ControlMessage::AnchorRecover { proposal }) => {
                self.recover(stream, vault_id, proposal).await
            }
            Ok(_) => {
                send_error(
                    stream,
                    SETUP_REFUSED_CODE,
                    "this request does not follow a setup proof",
                    false,
                )
                .await
            }
            Err(_) => Ok(()),
        }
    }

    async fn upload(
        &self,
        stream: &mut ServerStream,
        vault_id: &str,
        metadata: BundleMetadata,
        charter: PinnedCharter,
        take_anchor: bool,
    ) -> Result<(), String> {
        let Ok(_upload) = self.upload.try_lock() else {
            return send_error(
                stream,
                UPLOAD_BUSY_CODE,
                "another upload is running on this server",
                true,
            )
            .await;
        };
        let local = local_compatibility(self.version);
        let admissible = metadata
            .validate()
            .and_then(|()| protocol::ensure_compatible(&local, &metadata.compatibility))
            .and_then(|()| {
                if metadata.vault_id != vault_id {
                    return Err("the snapshot belongs to another vault".to_string());
                }
                if self.host.activation()?.is_some() {
                    return Err("this server already runs a vault".to_string());
                }
                charter.verify(vault_id).map(drop)
            });
        if let Err(message) = admissible {
            return send_error(stream, UPLOAD_REFUSED_CODE, &message, false).await;
        }
        let paths = self.host.paths().clone();
        clear_incoming(&paths)?;
        timeout_control(write_control(
            stream,
            &ControlMessage::ResumeAt { offset: 0 },
        ))
        .await?;
        let digest = receive_archive(stream, &paths.upload_archive(), metadata.archive_bytes).await;
        let digest = match digest {
            Ok(digest) => digest,
            Err(message) => {
                clear_incoming(&paths)?;
                return Err(message);
            }
        };
        let closed = timeout_control(read_control_limited(
            stream,
            protocol::MAX_CONTROL_BYTES,
        ))
        .await;
        let complete = matches!(
            &closed,
            Ok(ControlMessage::BundleComplete { transfer_id })
                if *transfer_id == metadata.transfer_id
        );
        if !complete || digest != metadata.archive_sha256 {
            clear_incoming(&paths)?;
            return send_error(
                stream,
                UPLOAD_REFUSED_CODE,
                "the snapshot did not arrive intact",
                false,
            )
            .await;
        }
        let activated = activate(
            &self.host,
            Upload {
                vault_id,
                charter: &charter,
                take_anchor,
            },
        )
        .await;
        clear_incoming(&paths)?;
        if let Err(message) = activated {
            return send_error(stream, UPLOAD_REFUSED_CODE, &message, false).await;
        }
        super::state::consume_setup_secret(&paths)?;
        self.commits.notify_one();
        println!("Activated vault {vault_id}.");
        timeout_control(write_control(
            stream,
            &ControlMessage::AnchorActivated {
                vault_id: vault_id.to_string(),
            },
        ))
        .await
    }

    /// Hands a proposal the recovery admin key signed to the sequencer. The sequencer verifies it
    /// as it verifies any proposal, so the secret only lets a device reach it.
    async fn recover(
        &self,
        stream: &mut ServerStream,
        vault_id: &str,
        proposal: String,
    ) -> Result<(), String> {
        let answer = self
            .hub
            .respond(ControlMessage::SyncPropose {
                protocol_version: protocol::PROTOCOL_VERSION,
                vault_id: vault_id.to_string(),
                device_id: self.host.own_device_id().to_string(),
                proposal,
            })
            .await;
        if matches!(
            &answer,
            ControlMessage::SyncProposed {
                revision: Some(_),
                ..
            }
        ) {
            super::state::consume_setup_secret(self.host.paths())?;
        }
        timeout_control(write_control(stream, &answer)).await
    }
}

async fn refuse_setup(stream: &mut ServerStream, message: &str) -> Result<(), String> {
    send_error(stream, SETUP_REFUSED_CODE, message, false).await
}

/// Empties the upload folder.
pub fn clear_incoming(paths: &super::state::DataPaths) -> Result<(), String> {
    let incoming = paths.incoming();
    match std::fs::remove_dir_all(&incoming) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("clear {}: {error}", incoming.display())),
    }
    ganbaru_atomic_files::create_private_directories(&incoming)
        .map_err(|error| format!("create {}: {error}", incoming.display()))
}

/// Reads exactly `bytes` archive bytes into `path` and returns their SHA-256 in hex.
async fn receive_archive(
    stream: &mut ServerStream,
    path: &std::path::Path,
    bytes: u64,
) -> Result<String, String> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|error| format!("create the upload: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; protocol::TRANSFER_CHUNK_BYTES];
    let mut remaining = bytes;
    while remaining > 0 {
        let want = usize::try_from(remaining)
            .unwrap_or(buffer.len())
            .min(buffer.len());
        let read = tokio::time::timeout(CHUNK_TIMEOUT, stream.read(&mut buffer[..want]))
            .await
            .map_err(|_| "the upload stalled".to_string())?
            .map_err(|error| format!("read the upload: {error}"))?;
        if read == 0 {
            return Err("the upload ended early".to_string());
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        file.write_all(chunk)
            .await
            .map_err(|error| format!("write the upload: {error}"))?;
        remaining -= read as u64;
    }
    file.sync_all()
        .await
        .map_err(|error| format!("sync the upload: {error}"))?;
    Ok(format!("{:x}", hasher.finalize()))
}
