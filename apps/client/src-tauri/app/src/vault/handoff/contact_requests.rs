//! Bounded worker for unauthenticated contact messages. It runs apart from the coordinator
//! actor, so a flood of contact requests cannot delay handoff or sync work.

use crate::contacts::requests::{self, PeerReply};
use std::sync::Arc;
use tauri::{AppHandle, Runtime};
use tokio::sync::{Semaphore, mpsc, oneshot};

/// Contact messages waiting for the worker. The listener answers busy while the queue is full.
pub(crate) const CONTACT_QUEUE_CAPACITY: usize = 16;
/// Contact messages handled at the same time.
const MAX_CONCURRENT_CONTACT_MESSAGES: usize = 4;

/// An unauthenticated contact message addressed to the person owning this vault.
#[derive(Debug)]
pub(crate) enum ContactOperation {
    /// A new contact request.
    Request {
        recipient_card_nonce: String,
        requester_card: String,
        request_id: String,
        signature: String,
    },
    /// A status poll for a contact request sent earlier.
    Status {
        request_id: String,
        requester_public_key: String,
        issued_at_ms: i64,
        signature: String,
    },
}

/// One queued contact message and where its answer goes.
pub(crate) struct ContactJob {
    pub operation: ContactOperation,
    pub response: oneshot::Sender<Result<PeerReply, String>>,
}

pub(crate) type ContactSender = mpsc::Sender<ContactJob>;

/// Creates the bounded queue between the listener and [`run`].
pub(crate) fn channel() -> (ContactSender, mpsc::Receiver<ContactJob>) {
    mpsc::channel(CONTACT_QUEUE_CAPACITY)
}

/// Handles queued contact messages until every sender is dropped.
pub(crate) async fn run<R: Runtime>(app: AppHandle<R>, mut receiver: mpsc::Receiver<ContactJob>) {
    let slots = Arc::new(Semaphore::new(MAX_CONCURRENT_CONTACT_MESSAGES));
    while let Some(job) = receiver.recv().await {
        // Waiting for a slot here leaves later messages in the bounded queue.
        let Ok(slot) = slots.clone().acquire_owned().await else {
            break;
        };
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let reply = handle(&app, job.operation).await;
            let _ = job.response.send(reply);
            drop(slot);
        });
    }
}

async fn handle<R: Runtime>(
    app: &AppHandle<R>,
    operation: ContactOperation,
) -> Result<PeerReply, String> {
    match operation {
        ContactOperation::Request {
            recipient_card_nonce,
            requester_card,
            request_id,
            signature,
        } => {
            requests::receive_request_for_coordinator(
                app,
                &recipient_card_nonce,
                &requester_card,
                &request_id,
                &signature,
            )
            .await
        }
        ContactOperation::Status {
            request_id,
            requester_public_key,
            issued_at_ms,
            signature,
        } => {
            requests::request_state_for_coordinator(
                app,
                &request_id,
                &requester_public_key,
                issued_at_ms,
                &signature,
            )
            .await
        }
    }
}
