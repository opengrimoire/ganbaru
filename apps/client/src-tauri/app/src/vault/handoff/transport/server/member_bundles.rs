//! Snapshot copies a member desktop serves on its sync listener: the first copy of a device
//! that links, or a fresh copy for a device that fell behind compaction.
//!
//! Only a device the selected log enrolled and did not remove, and that this installation did
//! not quarantine, gets a copy, and only of itself in this vault. A member serves copies only;
//! ownership transfers stay with the coordinator. One copy is held at a time.

use super::dispatch::serve_download;
use super::send_error;
use crate::vault::handoff::coordinator::{BUNDLE_PREPARATION_LIMIT, BundlePreparationWindows};
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::{
    BundleMetadata, BundlePurpose, ControlMessage, write_control,
};
use crate::vault::handoff::transport::timeout_control;
use ganbaru_sync_replica::peers::admission::{MemberAdmission, MemberRefusal};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

/// Error code for a copy refused because this member already holds one for another device.
const MEMBER_BUSY_CODE: &str = "member_busy";
/// Error code for an ownership request, which only the coordinator serves.
const BOOTSTRAP_ONLY_CODE: &str = "bootstrap_only";
/// How long a prepared copy stays reserved for its device before another device may replace it.
const PREPARED_SNAPSHOT_TTL: Duration = Duration::from_secs(30 * 60);

/// The copy a device asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotRequest {
    pub vault_id: String,
    pub device_id: String,
    /// The ownership generation the device holds, which a copy that changes no ownership keeps.
    pub generation: u64,
}

/// A prepared archive and the metadata the receiver checks it against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedSnapshot {
    pub metadata: BundleMetadata,
    pub archive_path: PathBuf,
}

/// Builds a snapshot copy of the active vault for one request.
pub(crate) type SnapshotPreparer = Arc<
    dyn Fn(
            SnapshotRequest,
        ) -> Pin<Box<dyn Future<Output = Result<PreparedSnapshot, String>> + Send>>
        + Send
        + Sync,
>;

struct HeldSnapshot {
    snapshot: PreparedSnapshot,
    prepared_at: Instant,
}

#[derive(Default)]
struct BundleState {
    held: Option<HeldSnapshot>,
    /// The last copy its receiver acknowledged, so a repeated acknowledgement is answered.
    acknowledged: Option<String>,
    preparations: BundlePreparationWindows,
}

/// The snapshot copies one member listener serves.
#[derive(Clone)]
pub(crate) struct MemberBundles {
    preparer: SnapshotPreparer,
    state: Arc<tokio::sync::Mutex<BundleState>>,
}

type ServerStream = tokio_rustls::server::TlsStream<TcpStream>;

impl MemberBundles {
    pub(crate) fn new(preparer: SnapshotPreparer) -> Self {
        Self {
            preparer,
            state: Arc::default(),
        }
    }

    /// Whether `request` is a snapshot copy request this module answers.
    pub(super) fn handles(request: &ControlMessage) -> bool {
        matches!(
            request,
            ControlMessage::RequestBundle { .. }
                | ControlMessage::DownloadBundle { .. }
                | ControlMessage::ActivationComplete { .. }
                | ControlMessage::CancelTransfer { .. }
        )
    }

    /// Answers one snapshot copy request from the peer whose certificate has `fingerprint`.
    pub(super) async fn serve(
        &self,
        stream: &mut ServerStream,
        manager: &PairingManager,
        admission: &MemberAdmission,
        fingerprint: &str,
        request: ControlMessage,
    ) -> Result<(), String> {
        match request {
            ControlMessage::RequestBundle {
                vault_id,
                device_id,
                generation,
                purpose,
                ..
            } => {
                if let Err(refusal) = admission.bootstrap(fingerprint, &vault_id, &device_id) {
                    return refuse(stream, refusal).await;
                }
                if purpose != BundlePurpose::Refresh {
                    return send_error(
                        stream,
                        BOOTSTRAP_ONLY_CODE,
                        "a member serves vault copies only; ownership goes through the coordinator",
                        false,
                    )
                    .await;
                }
                let request = SnapshotRequest {
                    vault_id,
                    device_id,
                    generation,
                };
                let response = match self.prepare(manager, request).await {
                    Ok(response) => response,
                    Err(error) => ControlMessage::Error {
                        code: "snapshot_unavailable".to_string(),
                        message: error,
                        retryable: true,
                    },
                };
                timeout_control(write_control(stream, &response)).await
            }
            ControlMessage::DownloadBundle { metadata } => {
                if let Err(refusal) =
                    admission.bootstrap(fingerprint, &metadata.vault_id, &metadata.device_id)
                {
                    return refuse(stream, refusal).await;
                }
                if !self.holds(&metadata).await {
                    return send_error(
                        stream,
                        "transfer_metadata",
                        "requested transfer metadata does not match the prepared copy",
                        false,
                    )
                    .await;
                }
                serve_download(stream, manager, metadata).await
            }
            ControlMessage::ActivationComplete {
                vault_id,
                device_id,
                transfer_id,
                ..
            } => {
                if let Err(refusal) = admission.bootstrap(fingerprint, &vault_id, &device_id) {
                    return refuse(stream, refusal).await;
                }
                if !self.finish(manager, &device_id, &transfer_id, true).await {
                    return send_error(
                        stream,
                        "transfer_unknown",
                        "activation does not match a copy this device prepared",
                        false,
                    )
                    .await;
                }
                timeout_control(write_control(
                    stream,
                    &ControlMessage::ActivationAcknowledged { transfer_id },
                ))
                .await
            }
            ControlMessage::CancelTransfer { transfer_id } => {
                let device_id = match admission.certificate(fingerprint) {
                    Ok(device_id) => device_id.to_string(),
                    Err(refusal) => return refuse(stream, refusal).await,
                };
                if !self.finish(manager, &device_id, &transfer_id, false).await {
                    return send_error(
                        stream,
                        "transfer_unknown",
                        "cancellation does not match a copy this device prepared",
                        false,
                    )
                    .await;
                }
                timeout_control(write_control(
                    stream,
                    &ControlMessage::TransferCancelled { transfer_id },
                ))
                .await
            }
            _ => Err("not a snapshot copy request".to_string()),
        }
    }

    /// The prepared copy for `request`: the one already held for that device, or a new one
    /// when no other device holds a copy that is still reserved.
    async fn prepare(
        &self,
        manager: &PairingManager,
        request: SnapshotRequest,
    ) -> Result<ControlMessage, String> {
        let mut state = self.state.lock().await;
        if let Some(held) = state.held.as_ref() {
            let metadata = &held.snapshot.metadata;
            if metadata.vault_id == request.vault_id && metadata.device_id == request.device_id {
                return Ok(prepared(metadata.clone()));
            }
            if held.prepared_at.elapsed() < PREPARED_SNAPSHOT_TTL {
                return Ok(ControlMessage::Error {
                    code: MEMBER_BUSY_CODE.to_string(),
                    message: "this device is serving a copy to another device; retry shortly"
                        .to_string(),
                    retryable: true,
                });
            }
        }
        if let Some(stale) = state.held.take() {
            release(manager, &stale.snapshot);
        }
        if state
            .preparations
            .admit(&request.device_id, Instant::now())
            .is_err()
        {
            return Ok(ControlMessage::Error {
                code: "preparation_limit".to_string(),
                message: BUNDLE_PREPARATION_LIMIT.to_string(),
                retryable: true,
            });
        }
        let snapshot = (self.preparer)(request.clone()).await?;
        if snapshot.metadata.vault_id != request.vault_id
            || snapshot.metadata.device_id != request.device_id
        {
            let _ = std::fs::remove_file(&snapshot.archive_path);
            return Err("the prepared copy does not match the request".to_string());
        }
        if let Err(error) = manager
            .register_outgoing_bundle(snapshot.metadata.clone(), snapshot.archive_path.clone())
        {
            let _ = std::fs::remove_file(&snapshot.archive_path);
            return Err(error);
        }
        let metadata = snapshot.metadata.clone();
        state.held = Some(HeldSnapshot {
            snapshot,
            prepared_at: Instant::now(),
        });
        Ok(prepared(metadata))
    }

    async fn holds(&self, metadata: &BundleMetadata) -> bool {
        self.state
            .lock()
            .await
            .held
            .as_ref()
            .is_some_and(|held| held.snapshot.metadata == *metadata)
    }

    /// Releases the copy `transfer_id` held for `device_id`. A repeated activation of the last
    /// acknowledged copy also counts, so the receiver can retry its acknowledgement.
    async fn finish(
        &self,
        manager: &PairingManager,
        device_id: &str,
        transfer_id: &str,
        activated: bool,
    ) -> bool {
        let mut state = self.state.lock().await;
        let matches = state.held.as_ref().is_some_and(|held| {
            held.snapshot.metadata.transfer_id == transfer_id
                && held.snapshot.metadata.device_id == device_id
        });
        if matches {
            if let Some(held) = state.held.take() {
                release(manager, &held.snapshot);
            }
            if activated {
                state.acknowledged = Some(transfer_id.to_string());
            }
            return true;
        }
        activated && state.acknowledged.as_deref() == Some(transfer_id)
    }
}

fn prepared(metadata: BundleMetadata) -> ControlMessage {
    ControlMessage::BundlePrepared {
        metadata,
        purpose: BundlePurpose::Refresh,
    }
}

fn release(manager: &PairingManager, snapshot: &PreparedSnapshot) {
    if let Err(error) = manager.unregister_outgoing_bundle(&snapshot.metadata.transfer_id) {
        eprintln!("sync member listener: release a prepared copy: {error}");
    }
    if let Err(error) = std::fs::remove_file(&snapshot.archive_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("sync member listener: remove a prepared copy: {error}");
    }
}

/// Answers a refused copy request with its typed code and returns the refusal as the
/// connection's error, so it is logged without keys or certificates.
async fn refuse(stream: &mut ServerStream, refusal: MemberRefusal) -> Result<(), String> {
    send_error(stream, refusal.code(), refusal.describe(), false).await?;
    Err(format!("refused: {}", refusal.code()))
}
