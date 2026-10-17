//! Contact requests: receiving them on the coordinator, sending and polling them from any
//! writable device, and the local accept, decline, cancel, and block transitions.

use super::card::sign_local_card;
use super::identity::LocalIdentity;
use super::store::{
    self, ContactState, NewContact, NewRequest, RequestDirection, RequestRow, RequestState,
    TrustGrant,
};
use super::{ContactsError, contacts_pool};
#[cfg(desktop)]
use crate::vault::handoff::coordinator::CoordinatorResponse;
use crate::vault::handoff::pairing::random_token;
use crate::vault::handoff::protocol::{
    ContactRequestOutcome, ControlMessage, PROTOCOL_VERSION, unix_time_ms,
};
use crate::vault::handoff::transport::unauthenticated_exchange;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use ganbaru_contacts::{
    PersonPublicKey, TrustKind, decode_card_text, encode_nonce, request_signature_payload,
    status_signature_payload,
};
#[cfg(any(desktop, test))]
use ganbaru_contacts::{SIGNATURE_BYTES, SignedCard, decode_nonce, nonces_match, verify};
use sqlx::SqlitePool;
use tauri::{AppHandle, Runtime};

/// How long a request stays pending on both sides.
pub(crate) const REQUEST_TTL_DAYS: i64 = 7;
/// Upper bound on pending received requests before the oldest are expired.
#[cfg(any(desktop, test))]
pub(crate) const MAX_PENDING_RECEIVED: i64 = 64;
const INITIAL_POLL_DELAY: std::time::Duration = std::time::Duration::from_secs(10);
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const BACKOFF_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub(crate) const CARD_REVOKED_CODE: &str = "card_revoked";
pub(crate) const INVALID_REQUEST_CODE: &str = "invalid_request";
pub(crate) const RECIPIENT_UNAVAILABLE_CODE: &str = "recipient_unavailable";

/// What the coordinator answers to an unauthenticated Contacts message.
#[cfg(any(desktop, test))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PeerReply {
    Received {
        request_id: String,
    },
    State {
        state: ContactRequestOutcome,
        recipient_card: Option<String>,
    },
    Rejected {
        code: &'static str,
        message: &'static str,
        retryable: bool,
    },
}

#[cfg(any(desktop, test))]
impl PeerReply {
    fn invalid() -> Self {
        Self::Rejected {
            code: INVALID_REQUEST_CODE,
            message: "the contact request did not verify",
            retryable: false,
        }
    }

    fn revoked() -> Self {
        Self::Rejected {
            code: CARD_REVOKED_CODE,
            message: "the recipient regenerated their contact card",
            retryable: false,
        }
    }

    #[cfg(desktop)]
    fn unavailable() -> Self {
        Self::Rejected {
            code: RECIPIENT_UNAVAILABLE_CODE,
            message: "the recipient cannot receive contact requests right now",
            retryable: true,
        }
    }

    #[cfg(desktop)]
    fn into_coordinator_response(self) -> CoordinatorResponse {
        match self {
            Self::Received { request_id } => {
                CoordinatorResponse::ContactRequestReceived { request_id }
            }
            Self::State {
                state,
                recipient_card,
            } => CoordinatorResponse::ContactRequestState {
                state,
                recipient_card,
            },
            Self::Rejected {
                code,
                message,
                retryable,
            } => CoordinatorResponse::Rejected {
                code: code.to_string(),
                message: message.to_string(),
                retryable,
            },
        }
    }
}

fn expires_at(now: DateTime<Utc>) -> DateTime<Utc> {
    now + Duration::days(REQUEST_TTL_DAYS)
}

#[cfg(any(desktop, test))]
fn decode_signature(text: &str) -> Option<[u8; SIGNATURE_BYTES]> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    bytes.try_into().ok()
}

#[cfg(any(desktop, test))]
fn fingerprint_hex(card: &SignedCard) -> String {
    card.card
        .coordinator_fingerprint
        .map(|fingerprint| store::encode_hex(&fingerprint))
        .unwrap_or_default()
}

/// Stores a request addressed to this person's current card. Blocked requesters and
/// requests that cannot be stored get the same acknowledgement as stored ones.
#[cfg(any(desktop, test))]
pub(crate) async fn receive_request(
    pool: &SqlitePool,
    identity: &LocalIdentity,
    now: DateTime<Utc>,
    recipient_card_nonce: &str,
    requester_card: &str,
    request_id: &str,
    signature: &str,
) -> Result<PeerReply, String> {
    let Some(nonce) = decode_nonce(recipient_card_nonce) else {
        return Ok(PeerReply::invalid());
    };
    let Ok(signed) = decode_card_text(requester_card) else {
        return Ok(PeerReply::invalid());
    };
    let Some(signature) = decode_signature(signature) else {
        return Ok(PeerReply::invalid());
    };
    let payload = request_signature_payload(&nonce, request_id);
    if verify(&signed.card.public_key, &payload, &signature).is_err() {
        return Ok(PeerReply::invalid());
    }
    if !nonces_match(&nonce, &identity.card_nonce) {
        return Ok(PeerReply::revoked());
    }
    if signed.card.public_key == identity.public_key {
        return Ok(PeerReply::invalid());
    }
    let acknowledged = PeerReply::Received {
        request_id: request_id.to_string(),
    };
    let public_key = signed.card.public_key.to_text();
    if let Some(contact) = store::contact_by_key(pool, &public_key).await?
        && ContactState::parse(&contact.state) == Some(ContactState::Blocked)
    {
        return Ok(acknowledged);
    }
    if let Some(existing) = store::request_by_id(pool, request_id).await? {
        if existing.public_key != public_key {
            return Ok(PeerReply::invalid());
        }
        if RequestState::parse(&existing.state) != Some(RequestState::Pending) {
            return Ok(acknowledged);
        }
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin contact request transaction: {error}"))?;
    store::delete_pending(&mut *tx, RequestDirection::Received, &public_key).await?;
    if store::pending_received_count(&mut *tx).await? >= MAX_PENDING_RECEIVED {
        store::expire_oldest_pending_received(&mut *tx, MAX_PENDING_RECEIVED - 1, now).await?;
    }
    let digest = store::encode_hex(&signed.digest);
    let fingerprint = fingerprint_hex(&signed);
    store::insert_request(
        &mut *tx,
        NewRequest {
            id: request_id,
            direction: RequestDirection::Received,
            public_key: &public_key,
            display_name: &signed.card.display_name,
            color: signed.card.color,
            card: requester_card,
            card_digest: &digest,
            endpoint_hint: &signed.card.endpoint_hint,
            coordinator_fingerprint: &fingerprint,
            invite_trust: TrustKind::NotAllowed,
            message_trust: TrustKind::NotAllowed,
            expires_at: expires_at(now),
        },
        now,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit contact request: {error}"))?;
    Ok(acknowledged)
}

/// What a requester learns when polling. Unknown and blocked requests look pending.
#[cfg(any(desktop, test))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StatusOutcome {
    Invalid,
    Pending,
    Declined,
    Accepted,
}

#[cfg(any(desktop, test))]
pub(crate) async fn request_state(
    pool: &SqlitePool,
    request_id: &str,
    requester_public_key: &str,
    issued_at_ms: i64,
    signature: &str,
) -> Result<StatusOutcome, String> {
    let Ok(public_key) = PersonPublicKey::from_text(requester_public_key) else {
        return Ok(StatusOutcome::Invalid);
    };
    let Some(signature) = decode_signature(signature) else {
        return Ok(StatusOutcome::Invalid);
    };
    let payload = status_signature_payload(request_id, issued_at_ms);
    if verify(&public_key, &payload, &signature).is_err() {
        return Ok(StatusOutcome::Invalid);
    }
    let Some(row) = store::request_by_id(pool, request_id).await? else {
        return Ok(StatusOutcome::Pending);
    };
    if row.public_key != requester_public_key
        || RequestDirection::parse(&row.direction) != Some(RequestDirection::Received)
    {
        return Ok(StatusOutcome::Pending);
    }
    if let Some(contact) = store::contact_by_key(pool, &row.public_key).await?
        && ContactState::parse(&contact.state) == Some(ContactState::Blocked)
    {
        return Ok(StatusOutcome::Pending);
    }
    Ok(match RequestState::parse(&row.state) {
        Some(RequestState::Accepted) => StatusOutcome::Accepted,
        Some(RequestState::Declined) => StatusOutcome::Declined,
        _ => StatusOutcome::Pending,
    })
}

#[cfg(desktop)]
async fn coordinator_identity<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<Option<LocalIdentity>, String> {
    if !crate::vault::ownership::active_status(app)?.can_write {
        return Ok(None);
    }
    super::identity::load_identity(app, pool).await
}

#[cfg(desktop)]
pub(crate) async fn receive_request_for_coordinator<R: Runtime>(
    app: &AppHandle<R>,
    recipient_card_nonce: String,
    requester_card: String,
    request_id: String,
    signature: String,
) -> Result<CoordinatorResponse, String> {
    let pool = contacts_pool(app).await?;
    let Some(identity) = coordinator_identity(app, &pool).await? else {
        return Ok(PeerReply::unavailable().into_coordinator_response());
    };
    let reply = receive_request(
        &pool,
        &identity,
        store::now(),
        &recipient_card_nonce,
        &requester_card,
        &request_id,
        &signature,
    )
    .await?;
    Ok(reply.into_coordinator_response())
}

#[cfg(desktop)]
pub(crate) async fn request_state_for_coordinator<R: Runtime>(
    app: &AppHandle<R>,
    request_id: String,
    requester_public_key: String,
    issued_at_ms: i64,
    signature: String,
) -> Result<CoordinatorResponse, String> {
    let pool = contacts_pool(app).await?;
    let Some(identity) = coordinator_identity(app, &pool).await? else {
        return Ok(PeerReply::unavailable().into_coordinator_response());
    };
    let outcome = request_state(
        &pool,
        &request_id,
        &requester_public_key,
        issued_at_ms,
        &signature,
    )
    .await?;
    let reply = match outcome {
        StatusOutcome::Invalid => PeerReply::invalid(),
        StatusOutcome::Pending => PeerReply::State {
            state: ContactRequestOutcome::Pending,
            recipient_card: None,
        },
        StatusOutcome::Declined => PeerReply::State {
            state: ContactRequestOutcome::Declined,
            recipient_card: None,
        },
        StatusOutcome::Accepted => match sign_local_card(app, &identity) {
            Ok(card) => PeerReply::State {
                state: ContactRequestOutcome::Accepted,
                recipient_card: Some(card.text()),
            },
            Err(_) => PeerReply::State {
                state: ContactRequestOutcome::Pending,
                recipient_card: None,
            },
        },
    };
    Ok(reply.into_coordinator_response())
}

/// Delivers a request to the person whose card text was pasted or scanned, then records it.
pub(crate) async fn send_request<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    identity: &LocalIdentity,
    card_text: &str,
    invite_trust: TrustKind,
    message_trust: TrustKind,
) -> Result<(), ContactsError> {
    let recipient = decode_card_text(card_text)
        .map_err(|error| ContactsError::InvalidCard(error.to_string()))?;
    if recipient.card.public_key == identity.public_key {
        return Err(ContactsError::InvalidCard(
            "this is your own contact card".to_string(),
        ));
    }
    let public_key = recipient.card.public_key.to_text();
    if let Some(contact) = store::contact_by_key(pool, &public_key).await? {
        let message = match ContactState::parse(&contact.state) {
            Some(ContactState::Blocked) => "this person is blocked",
            _ => "this person is already a contact",
        };
        return Err(ContactsError::Failed(message.to_string()));
    }
    let Some(fingerprint) = recipient.card.coordinator_fingerprint else {
        return Err(ContactsError::RecipientUnreachable(
            "the card carries no delivery address".to_string(),
        ));
    };
    if recipient.card.endpoint_hint.is_empty() {
        return Err(ContactsError::RecipientUnreachable(
            "the card carries no delivery address".to_string(),
        ));
    }
    let key = identity.signing_key()?;
    let own_card = sign_local_card(app, identity)?;
    let request_id = random_token("contact")?;
    let signature = key.sign(&request_signature_payload(
        &recipient.card.nonce,
        &request_id,
    ));
    let message = ControlMessage::ContactRequest {
        protocol_version: PROTOCOL_VERSION,
        recipient_card_nonce: encode_nonce(&recipient.card.nonce),
        requester_card: own_card.text(),
        request_id: request_id.clone(),
        signature: URL_SAFE_NO_PAD.encode(signature),
    };
    let fingerprint_hex = store::encode_hex(&fingerprint);
    let response =
        unauthenticated_exchange(&recipient.card.endpoint_hint, &fingerprint_hex, message)
            .await
            .map_err(ContactsError::RecipientUnreachable)?;
    match response {
        ControlMessage::ContactRequestReceived { request_id: echoed } if echoed == request_id => {}
        ControlMessage::Error { code, message, .. } => return Err(peer_error(&code, message)),
        _ => {
            return Err(ContactsError::Failed(
                "the recipient answered with an unexpected message".to_string(),
            ));
        }
    }
    let now = store::now();
    let digest = store::encode_hex(&recipient.digest);
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin contact request transaction: {error}"))?;
    store::delete_pending(&mut *tx, RequestDirection::Sent, &public_key).await?;
    store::insert_request(
        &mut *tx,
        NewRequest {
            id: &request_id,
            direction: RequestDirection::Sent,
            public_key: &public_key,
            display_name: &recipient.card.display_name,
            color: recipient.card.color,
            card: card_text,
            card_digest: &digest,
            endpoint_hint: &recipient.card.endpoint_hint,
            coordinator_fingerprint: &fingerprint_hex,
            invite_trust,
            message_trust,
            expires_at: expires_at(now),
        },
        now,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit contact request: {error}"))?;
    Ok(())
}

fn peer_error(code: &str, message: String) -> ContactsError {
    match code {
        CARD_REVOKED_CODE => ContactsError::CardRevoked(message),
        RECIPIENT_UNAVAILABLE_CODE => ContactsError::RecipientUnreachable(message),
        _ => ContactsError::Failed(message),
    }
}

fn revision_conflict() -> ContactsError {
    ContactsError::RevisionConflict("the request changed since it was last read".to_string())
}

async fn pending_row_in_direction(
    pool: &SqlitePool,
    id: &str,
    direction: RequestDirection,
) -> Result<RequestRow, ContactsError> {
    let row = store::request_by_id(pool, id)
        .await?
        .ok_or_else(revision_conflict)?;
    if RequestDirection::parse(&row.direction) != Some(direction)
        || RequestState::parse(&row.state) != Some(RequestState::Pending)
    {
        return Err(revision_conflict());
    }
    Ok(row)
}

/// Accepts a received request: the row becomes accepted and the person an active contact.
pub(crate) async fn accept_request(
    pool: &SqlitePool,
    id: &str,
    expected_revision: i64,
    invite_trust: TrustKind,
    message_trust: TrustKind,
    now: DateTime<Utc>,
) -> Result<(), ContactsError> {
    let row = pending_row_in_direction(pool, id, RequestDirection::Received).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin accept transaction: {error}"))?;
    let updated = store::set_request_state(
        &mut *tx,
        id,
        Some(expected_revision),
        RequestState::Accepted,
        None,
        now,
    )
    .await?;
    if !updated {
        return Err(revision_conflict());
    }
    store::upsert_active_contact(
        &mut *tx,
        NewContact {
            public_key: &row.public_key,
            display_name: &row.display_name,
            color: store::color_from_row(row.color),
        },
        &TrustGrant::new(invite_trust, now),
        &TrustGrant::new(message_trust, now),
        now,
    )
    .await?;
    store::close_pending_from(
        &mut *tx,
        RequestDirection::Sent,
        &row.public_key,
        RequestState::Expired,
        now,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit accept: {error}"))?;
    Ok(())
}

/// Declines a received request or cancels a sent one.
pub(crate) async fn close_request(
    pool: &SqlitePool,
    id: &str,
    expected_revision: i64,
    direction: RequestDirection,
    state: RequestState,
) -> Result<(), ContactsError> {
    pending_row_in_direction(pool, id, direction).await?;
    let updated =
        store::set_request_state(pool, id, Some(expected_revision), state, None, store::now())
            .await?;
    if updated {
        Ok(())
    } else {
        Err(revision_conflict())
    }
}

/// Blocks a person by public key, closing every pending request with them.
pub(crate) async fn block_person(
    pool: &SqlitePool,
    public_key: &str,
    now: DateTime<Utc>,
) -> Result<(), ContactsError> {
    PersonPublicKey::from_text(public_key)
        .map_err(|error| ContactsError::Failed(format!("public key is invalid: {error}")))?;
    let (display_name, color) =
        if let Some(contact) = store::contact_by_key(pool, public_key).await? {
            (contact.display_name, store::color_from_row(contact.color))
        } else if let Some(request) = store::latest_request_from(pool, public_key).await? {
            (request.display_name, store::color_from_row(request.color))
        } else {
            return Err(ContactsError::Failed("this person is unknown".to_string()));
        };
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin block transaction: {error}"))?;
    store::block_contact(
        &mut *tx,
        NewContact {
            public_key,
            display_name: &display_name,
            color,
        },
        now,
    )
    .await?;
    store::close_pending_from(
        &mut *tx,
        RequestDirection::Received,
        public_key,
        RequestState::Declined,
        now,
    )
    .await?;
    store::close_pending_from(
        &mut *tx,
        RequestDirection::Sent,
        public_key,
        RequestState::Expired,
        now,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit block: {error}"))?;
    Ok(())
}

/// Asks each recipient of a pending sent request for its state. Returns whether any request
/// is still pending afterwards.
pub(crate) async fn poll_sent_requests(
    pool: &SqlitePool,
    identity: &LocalIdentity,
) -> Result<bool, String> {
    let key = identity
        .signing_key()
        .map_err(|_| "this device holds no person key".to_string())?;
    let now = store::now();
    store::expire_stale_requests(pool, now).await?;
    let mut still_pending = false;
    for row in store::pending_requests(pool, RequestDirection::Sent).await? {
        if row.endpoint_hint.is_empty() || row.coordinator_fingerprint.is_empty() {
            store::touch_request_attempt(pool, &row.id, now, Some(RECIPIENT_UNAVAILABLE_CODE))
                .await?;
            still_pending = true;
            continue;
        }
        let issued_at_ms = unix_time_ms();
        let signature = key.sign(&status_signature_payload(&row.id, issued_at_ms));
        let message = ControlMessage::ContactRequestStatus {
            protocol_version: PROTOCOL_VERSION,
            request_id: row.id.clone(),
            requester_public_key: identity.public_key.to_text(),
            issued_at_ms,
            signature: URL_SAFE_NO_PAD.encode(signature),
        };
        let response =
            unauthenticated_exchange(&row.endpoint_hint, &row.coordinator_fingerprint, message)
                .await;
        let resolved = apply_status_response(pool, &row, response, now).await?;
        still_pending |= !resolved;
    }
    Ok(still_pending)
}

/// Records one status answer for a sent request. Returns true when the row left `pending`.
async fn apply_status_response(
    pool: &SqlitePool,
    row: &RequestRow,
    response: Result<ControlMessage, String>,
    now: DateTime<Utc>,
) -> Result<bool, String> {
    match response {
        Ok(ControlMessage::ContactRequestState {
            state: ContactRequestOutcome::Accepted,
            recipient_card: Some(card_text),
        }) => {
            let Ok(card) = decode_card_text(&card_text) else {
                store::touch_request_attempt(pool, &row.id, now, Some(INVALID_REQUEST_CODE))
                    .await?;
                return Ok(false);
            };
            if card.card.public_key.to_text() != row.public_key {
                store::touch_request_attempt(pool, &row.id, now, Some(INVALID_REQUEST_CODE))
                    .await?;
                return Ok(false);
            }
            let invite = TrustKind::parse(&row.invite_trust).unwrap_or(TrustKind::NotAllowed);
            let message = TrustKind::parse(&row.message_trust).unwrap_or(TrustKind::NotAllowed);
            let mut tx = pool
                .begin()
                .await
                .map_err(|error| format!("begin accepted transaction: {error}"))?;
            store::set_request_state(&mut *tx, &row.id, None, RequestState::Accepted, None, now)
                .await?;
            store::upsert_active_contact(
                &mut *tx,
                NewContact {
                    public_key: &row.public_key,
                    display_name: &card.card.display_name,
                    color: card.card.color,
                },
                &TrustGrant::new(invite, now),
                &TrustGrant::new(message, now),
                now,
            )
            .await?;
            tx.commit()
                .await
                .map_err(|error| format!("commit accepted request: {error}"))?;
            Ok(true)
        }
        Ok(ControlMessage::ContactRequestState {
            state: ContactRequestOutcome::Declined,
            ..
        }) => {
            store::set_request_state(pool, &row.id, None, RequestState::Declined, None, now)
                .await?;
            Ok(true)
        }
        Ok(ControlMessage::ContactRequestState { .. }) => {
            store::touch_request_attempt(pool, &row.id, now, None).await?;
            Ok(false)
        }
        Ok(ControlMessage::Error { code, .. }) if code == CARD_REVOKED_CODE => {
            store::set_request_state(
                pool,
                &row.id,
                None,
                RequestState::Expired,
                Some(CARD_REVOKED_CODE),
                now,
            )
            .await?;
            Ok(true)
        }
        Ok(ControlMessage::Error { code, .. }) => {
            store::touch_request_attempt(pool, &row.id, now, Some(&code)).await?;
            Ok(false)
        }
        Ok(_) => {
            store::touch_request_attempt(pool, &row.id, now, Some(INVALID_REQUEST_CODE)).await?;
            Ok(false)
        }
        Err(_) => {
            store::touch_request_attempt(pool, &row.id, now, Some(RECIPIENT_UNAVAILABLE_CODE))
                .await?;
            Ok(false)
        }
    }
}

/// Runs the background loop that fetches a missing person key on linked devices and polls
/// pending sent requests while this device can write.
pub(crate) fn start_request_polling<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_POLL_DELAY).await;
        loop {
            let interval = match poll_once(&app).await {
                Ok(()) => POLL_INTERVAL,
                Err(()) => BACKOFF_INTERVAL,
            };
            tokio::time::sleep(interval).await;
        }
    });
}

async fn poll_once<R: Runtime>(app: &AppHandle<R>) -> Result<(), ()> {
    super::identity::try_release(app).await;
    let Ok(status) = crate::vault::ownership::active_status(app) else {
        return Ok(());
    };
    if !status.can_write {
        return Ok(());
    }
    let Ok(pool) = contacts_pool(app).await else {
        return Ok(());
    };
    let Ok(Some(identity)) = super::identity::load_identity(app, &pool).await else {
        return Ok(());
    };
    if identity.key.is_none() {
        return Ok(());
    }
    let has_pending = store::pending_requests(&pool, RequestDirection::Sent)
        .await
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    if !has_pending {
        return Ok(());
    }
    poll_sent_requests(&pool, &identity)
        .await
        .map(|_| ())
        .map_err(|_| ())
}
