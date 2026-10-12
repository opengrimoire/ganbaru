//! Contacts: the local person identity, signed contact cards, contacts with trust scopes,
//! blocked contacts, and contact requests exchanged over the LAN handoff transport.
//!
//! The person's private key never enters the vault; it lives in native credential storage on
//! desktop and in an app-private file on Android. Everything else is vault SQLite state.

pub(crate) mod card;
pub(crate) mod identity;
pub(crate) mod requests;
pub(crate) mod store;
#[cfg(test)]
mod tests;

use crate::db::connect_sqlite;
use crate::vault::handoff::protocol::QrMatrix;
use ganbaru_contacts::TrustKind;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use store::{ContactState, RequestDirection, RequestState};
use tauri::{AppHandle, Runtime};

/// Stable Contacts failure codes the frontend branches on.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum ContactsError {
    /// No identity exists yet and this device cannot create one.
    IdentityUnavailable(String),
    /// The identity exists but this device holds no copy of the private key.
    KeyUnavailable(String),
    /// The profile has no display name, so no card can be signed.
    ProfileIncomplete(String),
    /// The card text or QR payload did not decode or verify.
    InvalidCard(String),
    /// The recipient regenerated their card after this one was issued.
    CardRevoked(String),
    /// The recipient could not be reached through the hint in the card.
    RecipientUnreachable(String),
    /// The vault is read-only on this device.
    ReadOnly(String),
    /// The row changed since the frontend last read it.
    RevisionConflict(String),
    Failed(String),
}

impl From<String> for ContactsError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl ContactsError {
    /// The failure message without its code.
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::IdentityUnavailable(message)
            | Self::KeyUnavailable(message)
            | Self::ProfileIncomplete(message)
            | Self::InvalidCard(message)
            | Self::CardRevoked(message)
            | Self::RecipientUnreachable(message)
            | Self::ReadOnly(message)
            | Self::RevisionConflict(message)
            | Self::Failed(message) => message,
        }
    }
}

/// The local person as the frontend sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityView {
    pub public_key: String,
    pub contact_id: String,
    pub card_revision: i64,
    pub private_key_available: bool,
}

/// A freshly signed local card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub text: String,
    pub qr: QrMatrix,
    pub verification_code: String,
    pub display_name: String,
    pub color: u8,
    /// Empty when nobody can reach this person right now.
    pub endpoint_hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCardView {
    pub identity: IdentityView,
    /// Absent while this device holds no private key copy.
    pub card: Option<CardView>,
}

/// A decoded card that has not been acted on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCardView {
    pub display_name: String,
    pub color: u8,
    pub verification_code: String,
    pub public_key: String,
    pub contact_id: String,
    pub has_endpoint: bool,
    pub is_self: bool,
    pub existing_state: Option<ContactState>,
    pub pending_sent: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactView {
    pub id: String,
    pub public_key: String,
    pub display_name: String,
    pub color: u8,
    pub state: ContactState,
    pub invite_trust: TrustKind,
    pub invite_trust_expires_at: Option<String>,
    pub message_trust: TrustKind,
    pub message_trust_expires_at: Option<String>,
    pub accepted_at: Option<String>,
    pub blocked_at: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactRequestView {
    pub id: String,
    pub direction: RequestDirection,
    pub public_key: String,
    pub contact_id: String,
    pub display_name: String,
    pub color: u8,
    pub verification_code: String,
    pub invite_trust: TrustKind,
    pub message_trust: TrustKind,
    pub state: RequestState,
    pub expires_at: String,
    pub last_attempt_at: Option<String>,
    pub last_error_code: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsSnapshot {
    pub identity: Option<IdentityView>,
    pub contacts: Vec<ContactView>,
    pub requests: Vec<ContactRequestView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendContactRequest {
    pub card_text: String,
    pub invite_trust: TrustKind,
    pub message_trust: TrustKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptContactRequest {
    pub id: String,
    pub expected_revision: i64,
    pub invite_trust: TrustKind,
    pub message_trust: TrustKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowRevision {
    pub id: String,
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockPersonRequest {
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTrustRequest {
    pub id: String,
    pub expected_revision: i64,
    pub invite_trust: TrustKind,
    pub message_trust: TrustKind,
}

pub(crate) async fn contacts_pool<R: Runtime>(app: &AppHandle<R>) -> Result<SqlitePool, String> {
    connect_sqlite(
        app.clone(),
        format!("sqlite:{}", crate::vault::APP_SQLITE_FILE),
    )
    .await
}

fn require_writable<R: Runtime>(app: &AppHandle<R>) -> Result<(), ContactsError> {
    let status = crate::vault::ownership::active_status(app)?;
    if status.can_write {
        Ok(())
    } else {
        Err(ContactsError::ReadOnly(
            "the vault is read-only on this device".to_string(),
        ))
    }
}

async fn snapshot<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<ContactsSnapshot, ContactsError> {
    let identity = identity::load_identity(app, pool)
        .await?
        .map(|identity| identity.view());
    let contacts = store::list_contacts(pool)
        .await?
        .into_iter()
        .map(store::ContactRow::view)
        .collect::<Result<Vec<_>, _>>()?;
    let requests = store::list_requests(pool)
        .await?
        .iter()
        .map(store::RequestRow::view)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ContactsSnapshot {
        identity,
        contacts,
        requests,
    })
}

#[tauri::command]
pub async fn contacts_list<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ContactsSnapshot, ContactsError> {
    let pool = contacts_pool(&app).await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_local_card<R: Runtime>(
    app: AppHandle<R>,
) -> Result<LocalCardView, ContactsError> {
    let pool = contacts_pool(&app).await?;
    let identity = identity::ensure_identity(&app, &pool).await?;
    card::local_card_view(&app, &identity)
}

#[tauri::command]
pub async fn contacts_regenerate_card<R: Runtime>(
    app: AppHandle<R>,
) -> Result<LocalCardView, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    let identity = identity::ensure_identity(&app, &pool).await?;
    let identity = card::rotate_card(&pool, identity).await?;
    card::local_card_view(&app, &identity)
}

#[tauri::command]
pub async fn contacts_parse_card<R: Runtime>(
    app: AppHandle<R>,
    text: String,
) -> Result<ParsedCardView, ContactsError> {
    let signed = ganbaru_contacts::decode_card_text(&text)
        .map_err(|error| ContactsError::InvalidCard(error.to_string()))?;
    let pool = contacts_pool(&app).await?;
    let identity = identity::load_identity(&app, &pool).await?;
    let public_key = signed.card.public_key.to_text();
    let existing = store::contact_by_key(&pool, &public_key).await?;
    let pending_sent = store::pending_request(&pool, RequestDirection::Sent, &public_key)
        .await?
        .is_some();
    Ok(ParsedCardView {
        display_name: signed.card.display_name.clone(),
        color: signed.card.color,
        verification_code: signed.verification_code(),
        contact_id: signed.card.public_key.contact_id(),
        has_endpoint: !signed.card.endpoint_hint.is_empty()
            && signed.card.coordinator_fingerprint.is_some(),
        is_self: identity.is_some_and(|identity| identity.public_key == signed.card.public_key),
        existing_state: existing.and_then(|row| ContactState::parse(&row.state)),
        pending_sent,
        public_key,
    })
}

#[tauri::command]
pub async fn contacts_decode_card_qr(
    width: usize,
    height: usize,
    luma: Vec<u8>,
) -> Result<String, ContactsError> {
    card::decode_card_qr(width, height, &luma)
}

#[tauri::command]
pub async fn contacts_send_request<R: Runtime>(
    app: AppHandle<R>,
    request: SendContactRequest,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    let identity = identity::ensure_identity(&app, &pool).await?;
    requests::send_request(
        &app,
        &pool,
        &identity,
        &request.card_text,
        request.invite_trust,
        request.message_trust,
    )
    .await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_accept_request<R: Runtime>(
    app: AppHandle<R>,
    request: AcceptContactRequest,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    requests::accept_request(
        &pool,
        &request.id,
        request.expected_revision,
        request.invite_trust,
        request.message_trust,
        store::now(),
    )
    .await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_decline_request<R: Runtime>(
    app: AppHandle<R>,
    request: RowRevision,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    requests::close_request(
        &pool,
        &request.id,
        request.expected_revision,
        RequestDirection::Received,
        RequestState::Declined,
    )
    .await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_cancel_request<R: Runtime>(
    app: AppHandle<R>,
    request: RowRevision,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    requests::close_request(
        &pool,
        &request.id,
        request.expected_revision,
        RequestDirection::Sent,
        RequestState::Expired,
    )
    .await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_block<R: Runtime>(
    app: AppHandle<R>,
    request: BlockPersonRequest,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    requests::block_person(&pool, &request.public_key, store::now()).await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_unblock<R: Runtime>(
    app: AppHandle<R>,
    request: RowRevision,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    remove_contact_in_state(
        &pool,
        &request.id,
        request.expected_revision,
        ContactState::Blocked,
    )
    .await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_remove<R: Runtime>(
    app: AppHandle<R>,
    request: RowRevision,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    remove_contact_in_state(
        &pool,
        &request.id,
        request.expected_revision,
        ContactState::Active,
    )
    .await?;
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_update_trust<R: Runtime>(
    app: AppHandle<R>,
    request: UpdateTrustRequest,
) -> Result<ContactsSnapshot, ContactsError> {
    require_writable(&app)?;
    let pool = contacts_pool(&app).await?;
    let now = store::now();
    let updated = store::update_contact_trust(
        &pool,
        &request.id,
        request.expected_revision,
        store::TrustGrant::new(request.invite_trust, now),
        store::TrustGrant::new(request.message_trust, now),
        now,
    )
    .await?;
    if !updated {
        return Err(ContactsError::RevisionConflict(
            "the contact changed since it was last read".to_string(),
        ));
    }
    snapshot(&app, &pool).await
}

#[tauri::command]
pub async fn contacts_sync_requests<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ContactsSnapshot, ContactsError> {
    let pool = contacts_pool(&app).await?;
    if let Some(identity) = identity::load_identity(&app, &pool).await?
        && identity.key.is_some()
        && crate::vault::ownership::active_status(&app)?.can_write
    {
        requests::poll_sent_requests(&pool, &identity).await?;
    }
    snapshot(&app, &pool).await
}

/// Writes a PNG rendering of the card to a user-selected path. Returns false when cancelled.
#[cfg(desktop)]
#[tauri::command]
pub async fn contacts_save_card_image(
    app: AppHandle,
    title: String,
    file_name: String,
    png_base64: String,
) -> Result<bool, ContactsError> {
    const MAX_PNG_BYTES: usize = 2 * 1024 * 1024;
    use base64::Engine;
    if png_base64.len() > MAX_PNG_BYTES.div_ceil(3) * 4 {
        return Err(ContactsError::Failed("card image is too large".to_string()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64)
        .map_err(|error| ContactsError::Failed(format!("decode card image: {error}")))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(ContactsError::Failed("card image is not a PNG".to_string()));
    }
    let picked = tauri::async_runtime::spawn_blocking(move || {
        crate::vault::pick_save_path(
            &app,
            &title,
            &file_name,
            "PNG image",
            &["png"],
            crate::vault::existing_downloads_directory(&app),
        )
    })
    .await
    .map_err(|error| ContactsError::Failed(format!("save dialog: {error}")))??;
    let Some(path) = picked else {
        return Ok(false);
    };
    if path.extension().and_then(|value| value.to_str()) != Some("png") {
        return Err(ContactsError::Failed(
            "card image must be saved as a PNG file".to_string(),
        ));
    }
    std::fs::write(&path, bytes)
        .map_err(|error| ContactsError::Failed(format!("write card image: {error}")))?;
    Ok(true)
}

async fn remove_contact_in_state(
    pool: &SqlitePool,
    id: &str,
    expected_revision: i64,
    state: ContactState,
) -> Result<(), ContactsError> {
    let removed = store::delete_contact(pool, id, expected_revision, state).await?;
    if removed {
        Ok(())
    } else {
        Err(ContactsError::RevisionConflict(
            "the contact changed since it was last read".to_string(),
        ))
    }
}
