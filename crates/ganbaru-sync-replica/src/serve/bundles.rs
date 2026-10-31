//! Snapshot copies a member desktop or anchor server serves on its sync listener: the first copy
//! of a device that links, or a fresh copy for a device that fell behind compaction.
//!
//! Only a device the selected log enrolled and did not remove, and that this installation did
//! not quarantine, gets a copy, and only of itself in this vault. One copy is held at a time.

use super::{Peer, Routes, ServerStream, send_error, timeout_control};
use crate::peers::admission::{MemberAdmission, MemberRefusal};
use ganbaru_handoff::pairing::{PairingManager, certificate_fingerprint};
use ganbaru_handoff::protocol::{
    BundleMetadata, ControlMessage, TRANSFER_CHUNK_BYTES, read_control, write_control,
};
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How long one chunk of a copy may take to read or send.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(20);
/// Span of the sliding window that bounds how often one linked device makes a vault snapshot.
const BUNDLE_PREPARATION_WINDOW: Duration = Duration::from_secs(60 * 60);
/// Most vault snapshots one linked device can start within [`BUNDLE_PREPARATION_WINDOW`]. Each
/// one pauses writers and builds a full archive. First copies and fresh copies stay far below it;
/// a device that keeps requesting and cancelling bundles does not.
const MAX_BUNDLE_PREPARATIONS_PER_PEER: usize = 30;
/// Error returned when one device requested too many snapshots; the frontend maps it to
/// localized guidance.
pub const BUNDLE_PREPARATION_LIMIT: &str =
    "this device requested the vault too often; wait before requesting another copy";

/// Drops events that left the sliding window ending at `now`.
fn forget_expired(events: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while events
        .front()
        .is_some_and(|event| now.saturating_duration_since(*event) >= window)
    {
        events.pop_front();
    }
}

/// Recent vault snapshots started for each linked device, kept in memory for the per-device limit.
#[derive(Debug, Default)]
pub struct BundlePreparationWindows {
    peers: BTreeMap<String, VecDeque<Instant>>,
}

impl BundlePreparationWindows {
    /// Records a snapshot for the device, or refuses it while that device's window is full.
    pub fn admit(&mut self, device_id: &str, now: Instant) -> Result<(), String> {
        self.peers.retain(|_, events| {
            forget_expired(events, now, BUNDLE_PREPARATION_WINDOW);
            !events.is_empty()
        });
        let events = self.peers.entry(device_id.to_string()).or_default();
        if events.len() >= MAX_BUNDLE_PREPARATIONS_PER_PEER {
            return Err(BUNDLE_PREPARATION_LIMIT.to_string());
        }
        events.push_back(now);
        Ok(())
    }
}

/// Streams a registered copy to a receiver from the offset it resumes at.
pub async fn serve_download(
    stream: &mut ServerStream,
    manager: &PairingManager,
    requested: BundleMetadata,
) -> Result<(), String> {
    let registered = manager.outgoing_bundle(&requested.transfer_id)?;
    if registered.metadata != requested {
        return send_error(
            stream,
            "transfer_metadata",
            "requested transfer metadata does not match the registered bundle",
            false,
        )
        .await;
    }
    timeout_control(write_control(
        stream,
        &ControlMessage::BundleMetadata {
            metadata: registered.metadata.clone(),
        },
    ))
    .await?;
    let response = timeout_control(read_control(stream)).await?;
    let offset = match response {
        ControlMessage::ResumeAt { offset } if offset <= registered.metadata.archive_bytes => {
            offset
        }
        _ => {
            return send_error(
                stream,
                "resume_offset",
                "transfer resume offset is invalid",
                false,
            )
            .await;
        }
    };
    stream_file(
        stream,
        &registered.path,
        offset,
        registered.metadata.archive_bytes,
    )
    .await?;
    match timeout_control(read_control(stream)).await? {
        ControlMessage::BundleComplete { transfer_id }
            if transfer_id == registered.metadata.transfer_id =>
        {
            Ok(())
        }
        _ => Err("receiver did not acknowledge the completed transfer".to_string()),
    }
}

async fn stream_file<W>(
    writer: &mut W,
    path: &Path,
    offset: u64,
    total_bytes: u64,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("open transfer bundle: {error}"))?;
    tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(offset))
        .await
        .map_err(|error| format!("seek transfer bundle: {error}"))?;
    let mut remaining = total_bytes.saturating_sub(offset);
    let mut buffer = vec![0_u8; TRANSFER_CHUNK_BYTES];
    while remaining > 0 {
        let chunk_bytes = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "transfer chunk size overflow".to_string())?;
        tokio::time::timeout(CHUNK_TIMEOUT, file.read_exact(&mut buffer[..chunk_bytes]))
            .await
            .map_err(|_| "reading the transfer bundle timed out".to_string())?
            .map_err(|error| format!("read transfer bundle: {error}"))?;
        tokio::time::timeout(CHUNK_TIMEOUT, writer.write_all(&buffer[..chunk_bytes]))
            .await
            .map_err(|_| "sending the transfer bundle timed out".to_string())?
            .map_err(|error| format!("send transfer bundle: {error}"))?;
        remaining -= chunk_bytes as u64;
    }
    writer
        .flush()
        .await
        .map_err(|error| format!("flush transfer bundle: {error}"))
}

/// Error code for a copy refused because this member already holds one for another device.
const MEMBER_BUSY_CODE: &str = "member_busy";
/// How long a prepared copy stays reserved for its device before another device may replace it.
const PREPARED_SNAPSHOT_TTL: Duration = Duration::from_secs(30 * 60);

/// The copy a device asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRequest {
    pub vault_id: String,
    pub device_id: String,
}

/// A prepared archive and the metadata the receiver checks it against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSnapshot {
    pub metadata: BundleMetadata,
    pub archive_path: PathBuf,
}

/// Builds a snapshot copy of the active vault for one request.
pub type SnapshotPreparer = Arc<
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
pub struct MemberBundles {
    preparer: SnapshotPreparer,
    state: Arc<tokio::sync::Mutex<BundleState>>,
}

impl MemberBundles {
    pub fn new(preparer: SnapshotPreparer) -> Self {
        Self {
            preparer,
            state: Arc::default(),
        }
    }

    /// Whether `request` is a snapshot copy request this module answers.
    pub fn handles(request: &ControlMessage) -> bool {
        matches!(
            request,
            ControlMessage::RequestBundle { .. }
                | ControlMessage::DownloadBundle { .. }
                | ControlMessage::ActivationComplete { .. }
                | ControlMessage::CancelTransfer { .. }
        )
    }

    /// Answers one snapshot copy request from the peer whose certificate has `fingerprint`.
    pub async fn serve(
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
                ..
            } => {
                if let Err(refusal) = admission.bootstrap(fingerprint, &vault_id, &device_id) {
                    return refuse(stream, refusal).await;
                }
                let request = SnapshotRequest {
                    vault_id,
                    device_id,
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

impl Routes for MemberBundles {
    fn handles(&self, request: &ControlMessage) -> bool {
        MemberBundles::handles(request)
    }

    async fn serve(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
    ) -> Result<(), String> {
        let (Some(admission), Some(certificate)) = (peer.admission, peer.certificate) else {
            return Err("a snapshot copy needs an admitted peer".to_string());
        };
        let fingerprint = certificate_fingerprint(certificate.as_ref());
        MemberBundles::serve(self, stream, peer.manager, admission, &fingerprint, request).await
    }
}

fn prepared(metadata: BundleMetadata) -> ControlMessage {
    ControlMessage::BundlePrepared { metadata }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_preparations_are_limited_per_device_and_sliding_window() {
        let start = Instant::now();
        let mut windows = BundlePreparationWindows::default();
        for _ in 0..MAX_BUNDLE_PREPARATIONS_PER_PEER {
            windows.admit("phone-a", start).unwrap();
        }
        assert_eq!(
            windows.admit("phone-a", start).unwrap_err(),
            BUNDLE_PREPARATION_LIMIT
        );
        // Another linked device keeps its own budget.
        windows.admit("phone-b", start).unwrap();
        let almost = start + BUNDLE_PREPARATION_WINDOW - Duration::from_millis(1);
        assert!(windows.admit("phone-a", almost).is_err());
        windows
            .admit("phone-a", start + BUNDLE_PREPARATION_WINDOW)
            .unwrap();
        // Refused requests do not extend the window.
        assert_eq!(
            windows.peers["phone-a"].len(),
            1,
            "only the admitted snapshot after expiry is kept"
        );
    }
}
