//! The Chat live execution reads a desktop answers for the linked devices that follow a work
//! assignment it executes, so a phone shows the run while it happens.
//!
//! Each read is admitted against a fresh read of the selected log and this installation's
//! quarantine, whichever listener carried it, and fails closed when admission cannot be read.
//! A read that asks to wait is held until the Chat tables change or [`CHAT_LIVE_WAIT`] passes,
//! and is admitted again after every wake before rows are read.
//!
//! The same routes take the answer a following device gives to an approval the run holds open,
//! admitted the same way and never held.

use super::member::MemberAdmissionSource;
use super::{CONTROL_TIMEOUT, Peer, Routes, ServerStream, send_error, timeout_control};
use crate::peers::admission::{MemberAdmission, MemberRefusal};
use ganbaru_handoff::pairing::certificate_fingerprint;
use ganbaru_handoff::protocol::{ChatRemoteApprovalDecision, ControlMessage, write_control};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, watch};

/// The longest a read that asks to wait is held before it is answered unchanged. It stays below
/// [`CONTROL_TIMEOUT`], the bound the reader waits for an answer within.
pub const CHAT_LIVE_WAIT: Duration = Duration::from_secs(12);
const _: () = assert!(CHAT_LIVE_WAIT.as_secs() + 3 <= CONTROL_TIMEOUT.as_secs());

pub use ganbaru_handoff::protocol::{
    CHAT_APPROVAL_STALE_CODE, CHAT_LIVE_NOT_HERE_CODE, CHAT_LIVE_UNAVAILABLE_CODE,
};

/// The execution read one admitted request asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLiveQuery {
    pub assignment_id: String,
    pub after_change: Option<u64>,
}

/// One bounded page of an execution, with the payload already encoded as JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLivePage {
    pub payload: String,
    pub change_cursor: u64,
    pub has_more: bool,
    pub reset_required: bool,
    pub settled: bool,
}

impl ChatLivePage {
    /// Whether this page adds nothing to what a reader at `after_change` already holds, so a
    /// waiting read keeps waiting.
    fn unchanged_since(&self, after_change: Option<u64>) -> bool {
        after_change == Some(self.change_cursor)
            && !self.has_more
            && !self.reset_required
            && !self.settled
    }
}

/// What the local Chat store answers for a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatLiveAnswer {
    /// This device does not execute the assignment.
    NotHere,
    Page(ChatLivePage),
}

/// Reads one page of the execution a query names from the active vault.
pub type ChatLiveReader = Arc<
    dyn Fn(ChatLiveQuery) -> Pin<Box<dyn Future<Output = Result<ChatLiveAnswer, String>> + Send>>
        + Send
        + Sync,
>;

/// The approval answer one admitted request carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatApprovalQuery {
    pub assignment_id: String,
    pub request_id: String,
    pub decision: ChatRemoteApprovalDecision,
    pub provider_option_id: String,
    pub command_id: String,
}

/// What the local Chat runtime answers for an approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatApprovalAnswer {
    /// The request was resolved with the decision, now or by an earlier try of the same command.
    Resolved,
    /// This device does not execute the assignment.
    NotHere,
    /// The request is no longer open, or the decision is not one it offered.
    Stale,
}

/// Resolves the approval a query names through the local Chat runtime.
pub type ChatApprovalResolver = Arc<
    dyn Fn(
            ChatApprovalQuery,
        ) -> Pin<Box<dyn Future<Output = Result<ChatApprovalAnswer, String>> + Send>>
        + Send
        + Sync,
>;

/// The device and assignment one held wait reads.
type WaitKey = (String, String);

/// The waits held for one device and assignment, so a new wait ends the one it replaces.
type HeldWaits = Arc<Mutex<HashMap<WaitKey, Arc<Notify>>>>;

/// The Chat live execution reads one listener answers.
#[derive(Clone)]
pub struct ChatLiveRoutes {
    admission: MemberAdmissionSource,
    reader: ChatLiveReader,
    resolver: ChatApprovalResolver,
    changes: watch::Receiver<u64>,
    waits: HeldWaits,
    wait: Duration,
}

impl ChatLiveRoutes {
    /// Reads answered from `reader` and approvals resolved through `resolver` for devices
    /// `admission` admits, with waits woken whenever `changes` moves. `admission` must apply the
    /// quarantine, since the coordinator listener's own log admission does not.
    pub fn new(
        admission: MemberAdmissionSource,
        reader: ChatLiveReader,
        resolver: ChatApprovalResolver,
        changes: watch::Receiver<u64>,
    ) -> Self {
        Self {
            admission,
            reader,
            resolver,
            changes,
            waits: HeldWaits::default(),
            wait: CHAT_LIVE_WAIT,
        }
    }

    /// The same routes with waits bounded by `wait` instead of [`CHAT_LIVE_WAIT`].
    #[cfg(test)]
    fn waiting_at_most(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    /// Whether the device whose certificate has `fingerprint` may read `vault_id` as
    /// `device_id`, against admission as it is now.
    async fn admit(
        &self,
        fingerprint: &str,
        vault_id: &str,
        device_id: &str,
    ) -> Result<(), MemberRefusal> {
        admit((self.admission)().await, fingerprint, vault_id, device_id)
    }

    /// Answers `query` for the admitted read of `request`, holding a waiting read until a change
    /// or the wait bound. Admission is checked again after every wake.
    async fn answer(
        &self,
        request: &AdmittedRead<'_>,
        query: ChatLiveQuery,
        wait: bool,
    ) -> Result<ChatLiveAnswer, Failure> {
        let mut changes = self.changes.clone();
        changes.borrow_and_update();
        let mut answer = self.read(query.clone()).await?;
        let waits = match &answer {
            ChatLiveAnswer::Page(page) => wait && page.unchanged_since(query.after_change),
            ChatLiveAnswer::NotHere => false,
        };
        if !waits {
            return Ok(answer);
        }
        let key = (request.device_id.to_string(), query.assignment_id.clone());
        let superseded = self.hold(key.clone())?;
        let deadline = tokio::time::Instant::now() + self.wait;
        let outcome = async {
            loop {
                tokio::select! {
                    changed = changes.changed() => if changed.is_err() { break },
                    () = superseded.notified() => break,
                    () = tokio::time::sleep_until(deadline) => break,
                }
                self.admit(request.fingerprint, request.vault_id, request.device_id)
                    .await
                    .map_err(Failure::Refused)?;
                changes.borrow_and_update();
                answer = self.read(query.clone()).await?;
                let unchanged = matches!(
                    &answer,
                    ChatLiveAnswer::Page(page) if page.unchanged_since(query.after_change)
                );
                if !unchanged {
                    break;
                }
            }
            Ok(())
        }
        .await;
        self.release(&key, &superseded);
        outcome?;
        // The answer may be up to the wait bound old, so the device is admitted once more
        // before it is sent.
        self.admit(request.fingerprint, request.vault_id, request.device_id)
            .await
            .map_err(Failure::Refused)?;
        Ok(answer)
    }

    async fn read(&self, query: ChatLiveQuery) -> Result<ChatLiveAnswer, Failure> {
        (self.reader)(query).await.map_err(Failure::Read)
    }

    /// Holds a wait for `key`, ending the wait it replaces.
    fn hold(&self, key: WaitKey) -> Result<Arc<Notify>, Failure> {
        let notify = Arc::new(Notify::new());
        let mut waits = self.lock_waits()?;
        if let Some(previous) = waits.insert(key, notify.clone()) {
            // `notify_one` stores a permit, so a wait between two selects still sees it.
            previous.notify_one();
        }
        Ok(notify)
    }

    /// Forgets the wait for `key` unless a newer wait replaced it.
    fn release(&self, key: &WaitKey, notify: &Arc<Notify>) {
        let Ok(mut waits) = self.lock_waits() else {
            return;
        };
        if waits
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, notify))
        {
            waits.remove(key);
        }
    }

    fn lock_waits(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<WaitKey, Arc<Notify>>>, Failure> {
        self.waits
            .lock()
            .map_err(|_| Failure::Read("chat live wait lock is unavailable".to_string()))
    }
}

/// Who an admitted read came from.
struct AdmittedRead<'a> {
    fingerprint: &'a str,
    vault_id: &'a str,
    device_id: &'a str,
}

/// Why a read is not answered with a page.
enum Failure {
    Refused(MemberRefusal),
    Read(String),
}

/// Whether the device whose certificate has `fingerprint` may read the live execution of
/// `vault_id` as `device_id`: only a device the selected log enrolled, did not remove, and does
/// not hold, that this installation did not quarantine, naming itself in this vault. Admission
/// that cannot be read, or a vault where sync does not run, refuses the read.
pub fn admit(
    admission: Result<Option<MemberAdmission>, String>,
    fingerprint: &str,
    vault_id: &str,
    device_id: &str,
) -> Result<(), MemberRefusal> {
    match admission {
        Ok(Some(admission)) => admission.bootstrap(fingerprint, vault_id, device_id),
        Ok(None) => Err(MemberRefusal::Unavailable),
        Err(error) => {
            eprintln!("sync: read the Chat live admission: {error}");
            Err(MemberRefusal::Unavailable)
        }
    }
}

impl Routes for ChatLiveRoutes {
    fn handles(&self, request: &ControlMessage) -> bool {
        matches!(
            request,
            ControlMessage::ChatLiveRead { .. } | ControlMessage::ChatApprovalResolve { .. }
        )
    }

    async fn serve(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
    ) -> Result<(), String> {
        let certificate = peer.certificate.map(|certificate| certificate.as_ref());
        if matches!(request, ControlMessage::ChatApprovalResolve { .. }) {
            serve_approval(self, stream, certificate, request).await
        } else {
            serve_read(self, stream, certificate, request).await
        }
    }
}

/// Resolves one approval on `stream` for the peer that presented `certificate`.
async fn serve_approval<W>(
    routes: &ChatLiveRoutes,
    stream: &mut W,
    certificate: Option<&[u8]>,
    request: ControlMessage,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let ControlMessage::ChatApprovalResolve {
        vault_id,
        device_id,
        assignment_id,
        request_id,
        decision,
        provider_option_id,
        command_id,
        ..
    } = request
    else {
        return Err("not a Chat approval".to_string());
    };
    let Some(certificate) = certificate else {
        return refuse(stream, MemberRefusal::Unknown).await;
    };
    let fingerprint = certificate_fingerprint(certificate);
    if let Err(refusal) = routes.admit(&fingerprint, &vault_id, &device_id).await {
        return refuse(stream, refusal).await;
    }
    let query = ChatApprovalQuery {
        assignment_id,
        request_id,
        decision,
        provider_option_id,
        command_id,
    };
    match (routes.resolver)(query).await {
        Ok(ChatApprovalAnswer::Resolved) => {
            timeout_control(write_control(stream, &ControlMessage::ChatApprovalResolved)).await
        }
        Ok(ChatApprovalAnswer::NotHere) => {
            send_error(
                stream,
                CHAT_LIVE_NOT_HERE_CODE,
                "this device does not run that work",
                false,
            )
            .await
        }
        Ok(ChatApprovalAnswer::Stale) => {
            send_error(
                stream,
                CHAT_APPROVAL_STALE_CODE,
                "that approval is no longer open",
                false,
            )
            .await
        }
        Err(error) => {
            eprintln!("sync: resolve a Chat approval: {error}");
            send_error(
                stream,
                CHAT_LIVE_UNAVAILABLE_CODE,
                "this device could not resolve the approval",
                true,
            )
            .await
        }
    }
}

/// Answers one live read on `stream` from the peer that presented `certificate`.
async fn serve_read<W>(
    routes: &ChatLiveRoutes,
    stream: &mut W,
    certificate: Option<&[u8]>,
    request: ControlMessage,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let ControlMessage::ChatLiveRead {
        vault_id,
        device_id,
        assignment_id,
        after_change,
        wait,
        ..
    } = request
    else {
        return Err("not a Chat live read".to_string());
    };
    let Some(certificate) = certificate else {
        return refuse(stream, MemberRefusal::Unknown).await;
    };
    let fingerprint = certificate_fingerprint(certificate);
    if let Err(refusal) = routes.admit(&fingerprint, &vault_id, &device_id).await {
        return refuse(stream, refusal).await;
    }
    let read = AdmittedRead {
        fingerprint: &fingerprint,
        vault_id: &vault_id,
        device_id: &device_id,
    };
    let query = ChatLiveQuery {
        assignment_id,
        after_change,
    };
    match routes.answer(&read, query, wait).await {
        Ok(ChatLiveAnswer::Page(page)) => {
            let response = ControlMessage::ChatLive {
                payload: page.payload,
                change_cursor: page.change_cursor,
                has_more: page.has_more,
                reset_required: page.reset_required,
                settled: page.settled,
            };
            if let Err(error) = response.validate() {
                eprintln!("sync: a Chat live page is out of bounds: {error}");
                return send_error(
                    stream,
                    CHAT_LIVE_UNAVAILABLE_CODE,
                    "this device could not read the execution",
                    true,
                )
                .await;
            }
            timeout_control(write_control(stream, &response)).await
        }
        Ok(ChatLiveAnswer::NotHere) => {
            send_error(
                stream,
                CHAT_LIVE_NOT_HERE_CODE,
                "this device does not run that work",
                false,
            )
            .await
        }
        Err(Failure::Refused(refusal)) => refuse(stream, refusal).await,
        Err(Failure::Read(error)) => {
            eprintln!("sync: read a Chat live execution: {error}");
            send_error(
                stream,
                CHAT_LIVE_UNAVAILABLE_CODE,
                "this device could not read the execution",
                true,
            )
            .await
        }
    }
}

/// Answers a refused read with its typed code and returns the refusal as the connection's error.
async fn refuse<W>(stream: &mut W, refusal: MemberRefusal) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    send_error(
        stream,
        refusal.code(),
        refusal.describe(),
        refusal.retryable(),
    )
    .await?;
    Err(format!("refused: {}", refusal.code()))
}

#[cfg(test)]
mod tests;
