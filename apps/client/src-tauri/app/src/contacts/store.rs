//! SQLite access for contacts and contact requests.

use super::{ContactRequestView, ContactView};
use chrono::{DateTime, Utc};
use ganbaru_contacts::{PersonPublicKey, TrustKind};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Sqlite};

const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContactState {
    Active,
    Blocked,
}

impl ContactState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Blocked => "blocked",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestDirection {
    Sent,
    Received,
}

impl RequestDirection {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Received => "received",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "sent" => Some(Self::Sent),
            "received" => Some(Self::Received),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestState {
    Pending,
    Accepted,
    Declined,
    Expired,
}

impl RequestState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Expired => "expired",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "accepted" => Some(Self::Accepted),
            "declined" => Some(Self::Declined),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

pub(crate) fn now() -> DateTime<Utc> {
    std::time::SystemTime::now().into()
}

pub(crate) fn format_time(time: DateTime<Utc>) -> String {
    time.format(TIME_FORMAT).to_string()
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Decodes a 64-character lowercase or uppercase hex digest.
pub(crate) fn decode_hex_digest(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut digest = [0u8; 32];
    for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).ok()?;
        digest[index] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(digest)
}

/// A trust decision together with the expiry it implies when granted now.
#[derive(Clone, Debug)]
pub(crate) struct TrustGrant {
    pub kind: TrustKind,
    pub expires_at: Option<String>,
}

impl TrustGrant {
    pub(crate) fn new(kind: TrustKind, granted_at: DateTime<Utc>) -> Self {
        Self {
            kind,
            expires_at: kind.expires_at(granted_at).map(format_time),
        }
    }
}

fn parse_trust(value: &str) -> Result<TrustKind, String> {
    TrustKind::parse(value).ok_or_else(|| format!("unknown trust kind {value:?}"))
}

pub(crate) fn color_from_row(value: i64) -> u8 {
    u8::try_from(value)
        .ok()
        .filter(|color| *color <= 31)
        .unwrap_or(30)
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct ContactRow {
    pub id: String,
    pub public_key: String,
    pub display_name: String,
    pub color: i64,
    pub state: String,
    pub invite_trust: String,
    pub invite_trust_expires_at: Option<String>,
    pub message_trust: String,
    pub message_trust_expires_at: Option<String>,
    pub accepted_at: Option<String>,
    pub blocked_at: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl ContactRow {
    pub(crate) fn view(self) -> Result<ContactView, String> {
        let state = ContactState::parse(&self.state)
            .ok_or_else(|| format!("unknown contact state {:?}", self.state))?;
        Ok(ContactView {
            id: self.id,
            public_key: self.public_key,
            display_name: self.display_name,
            color: color_from_row(self.color),
            state,
            invite_trust: parse_trust(&self.invite_trust)?,
            invite_trust_expires_at: self.invite_trust_expires_at,
            message_trust: parse_trust(&self.message_trust)?,
            message_trust_expires_at: self.message_trust_expires_at,
            accepted_at: self.accepted_at,
            blocked_at: self.blocked_at,
            revision: self.revision,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct RequestRow {
    pub id: String,
    pub direction: String,
    pub public_key: String,
    pub display_name: String,
    pub color: i64,
    pub endpoint_hint: String,
    pub coordinator_fingerprint: String,
    pub invite_trust: String,
    pub message_trust: String,
    pub state: String,
    pub expires_at: String,
    pub last_attempt_at: Option<String>,
    pub last_error_code: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl RequestRow {
    pub(crate) fn view(&self) -> Result<ContactRequestView, String> {
        let direction = RequestDirection::parse(&self.direction)
            .ok_or_else(|| format!("unknown request direction {:?}", self.direction))?;
        let state = RequestState::parse(&self.state)
            .ok_or_else(|| format!("unknown request state {:?}", self.state))?;
        let public_key = PersonPublicKey::from_text(&self.public_key)
            .map_err(|error| format!("stored public key is malformed: {error}"))?;
        Ok(ContactRequestView {
            id: self.id.clone(),
            direction,
            public_key: self.public_key.clone(),
            contact_id: public_key.contact_id(),
            display_name: self.display_name.clone(),
            color: color_from_row(self.color),
            verification_code: public_key.verification_code(),
            invite_trust: parse_trust(&self.invite_trust)?,
            message_trust: parse_trust(&self.message_trust)?,
            state,
            expires_at: self.expires_at.clone(),
            last_attempt_at: self.last_attempt_at.clone(),
            last_error_code: self.last_error_code.clone(),
            revision: self.revision,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        })
    }
}

const CONTACT_COLUMNS: &str = "id, public_key, display_name, color, state, invite_trust, \
    invite_trust_expires_at, message_trust, message_trust_expires_at, accepted_at, blocked_at, \
    revision, created_at, updated_at";

const REQUEST_COLUMNS: &str = "id, direction, public_key, display_name, color, endpoint_hint, \
    coordinator_fingerprint, invite_trust, message_trust, state, expires_at, \
    last_attempt_at, last_error_code, revision, created_at, updated_at";

fn db_error(context: &str, error: sqlx::Error) -> String {
    format!("{context}: {error}")
}

pub(crate) async fn list_contacts<'e, E>(executor: E) -> Result<Vec<ContactRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {CONTACT_COLUMNS} FROM contacts ORDER BY state, lower(display_name), id"
    ))
    .fetch_all(executor)
    .await
    .map_err(|error| db_error("list contacts", error))
}

pub(crate) async fn contact_by_key<'e, E>(
    executor: E,
    public_key: &str,
) -> Result<Option<ContactRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {CONTACT_COLUMNS} FROM contacts WHERE public_key = ?"
    ))
    .bind(public_key)
    .fetch_optional(executor)
    .await
    .map_err(|error| db_error("load contact", error))
}

#[cfg(test)]
pub(crate) async fn contact_by_id<'e, E>(
    executor: E,
    id: &str,
) -> Result<Option<ContactRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {CONTACT_COLUMNS} FROM contacts WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(executor)
    .await
    .map_err(|error| db_error("load contact", error))
}

/// A person to store as a contact row.
pub(crate) struct NewContact<'a> {
    pub public_key: &'a str,
    pub display_name: &'a str,
    pub color: u8,
}

/// Creates or reactivates an active contact with the given trust grants.
pub(crate) async fn upsert_active_contact<'e, E>(
    executor: E,
    contact: NewContact<'_>,
    invite: &TrustGrant,
    message: &TrustGrant,
    now: DateTime<Utc>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let time = format_time(now);
    let id = crate::vault::handoff::pairing::random_token("contact")?;
    sqlx::query(
        "INSERT INTO contacts (
            id, public_key, display_name, color, state, invite_trust, invite_trust_expires_at,
            message_trust, message_trust_expires_at, accepted_at, blocked_at, created_at, updated_at
         ) VALUES (?, ?, ?, ?, 'active', ?, ?, ?, ?, ?, NULL, ?, ?)
         ON CONFLICT(public_key) DO UPDATE SET
            display_name = excluded.display_name,
            color = excluded.color,
            state = 'active',
            invite_trust = excluded.invite_trust,
            invite_trust_expires_at = excluded.invite_trust_expires_at,
            message_trust = excluded.message_trust,
            message_trust_expires_at = excluded.message_trust_expires_at,
            accepted_at = excluded.accepted_at,
            blocked_at = NULL,
            updated_at = excluded.updated_at",
    )
    .bind(id)
    .bind(contact.public_key)
    .bind(contact.display_name)
    .bind(i64::from(contact.color))
    .bind(invite.kind.as_str())
    .bind(&invite.expires_at)
    .bind(message.kind.as_str())
    .bind(&message.expires_at)
    .bind(&time)
    .bind(&time)
    .bind(&time)
    .execute(executor)
    .await
    .map_err(|error| db_error("store contact", error))?;
    Ok(())
}

/// Creates or converts a contact row into a blocked one with no trust.
pub(crate) async fn block_contact<'e, E>(
    executor: E,
    contact: NewContact<'_>,
    now: DateTime<Utc>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let time = format_time(now);
    let id = crate::vault::handoff::pairing::random_token("contact")?;
    sqlx::query(
        "INSERT INTO contacts (
            id, public_key, display_name, color, state, invite_trust, invite_trust_expires_at,
            message_trust, message_trust_expires_at, accepted_at, blocked_at, created_at, updated_at
         ) VALUES (?, ?, ?, ?, 'blocked', 'not_allowed', NULL, 'not_allowed', NULL, NULL, ?, ?, ?)
         ON CONFLICT(public_key) DO UPDATE SET
            state = 'blocked',
            invite_trust = 'not_allowed',
            invite_trust_expires_at = NULL,
            message_trust = 'not_allowed',
            message_trust_expires_at = NULL,
            blocked_at = excluded.blocked_at,
            updated_at = excluded.updated_at",
    )
    .bind(id)
    .bind(contact.public_key)
    .bind(contact.display_name)
    .bind(i64::from(contact.color))
    .bind(&time)
    .bind(&time)
    .bind(&time)
    .execute(executor)
    .await
    .map_err(|error| db_error("block contact", error))?;
    Ok(())
}

/// Updates both trust scopes of an active contact. Returns false on a revision mismatch.
pub(crate) async fn update_contact_trust<'e, E>(
    executor: E,
    id: &str,
    expected_revision: i64,
    invite: TrustGrant,
    message: TrustGrant,
    now: DateTime<Utc>,
) -> Result<bool, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let result = sqlx::query(
        "UPDATE contacts
         SET invite_trust = ?, invite_trust_expires_at = ?, message_trust = ?,
             message_trust_expires_at = ?, updated_at = ?
         WHERE id = ? AND revision = ? AND state = 'active'",
    )
    .bind(invite.kind.as_str())
    .bind(&invite.expires_at)
    .bind(message.kind.as_str())
    .bind(&message.expires_at)
    .bind(format_time(now))
    .bind(id)
    .bind(expected_revision)
    .execute(executor)
    .await
    .map_err(|error| db_error("update contact trust", error))?;
    Ok(result.rows_affected() == 1)
}

/// Deletes a contact row in the given state. Returns false on a revision or state mismatch.
pub(crate) async fn delete_contact<'e, E>(
    executor: E,
    id: &str,
    expected_revision: i64,
    state: ContactState,
) -> Result<bool, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let result = sqlx::query("DELETE FROM contacts WHERE id = ? AND revision = ? AND state = ?")
        .bind(id)
        .bind(expected_revision)
        .bind(state.as_str())
        .execute(executor)
        .await
        .map_err(|error| db_error("delete contact", error))?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn list_requests<'e, E>(executor: E) -> Result<Vec<RequestRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM contact_requests ORDER BY created_at DESC, id"
    ))
    .fetch_all(executor)
    .await
    .map_err(|error| db_error("list contact requests", error))
}

pub(crate) async fn request_by_id<'e, E>(
    executor: E,
    id: &str,
) -> Result<Option<RequestRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM contact_requests WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(executor)
    .await
    .map_err(|error| db_error("load contact request", error))
}

pub(crate) async fn pending_request<'e, E>(
    executor: E,
    direction: RequestDirection,
    public_key: &str,
) -> Result<Option<RequestRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM contact_requests
         WHERE direction = ? AND public_key = ? AND state = 'pending'"
    ))
    .bind(direction.as_str())
    .bind(public_key)
    .fetch_optional(executor)
    .await
    .map_err(|error| db_error("load pending contact request", error))
}

pub(crate) async fn pending_requests<'e, E>(
    executor: E,
    direction: RequestDirection,
) -> Result<Vec<RequestRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM contact_requests
         WHERE direction = ? AND state = 'pending' ORDER BY created_at, id"
    ))
    .bind(direction.as_str())
    .fetch_all(executor)
    .await
    .map_err(|error| db_error("list pending contact requests", error))
}

/// The most recent request row from a person in any state.
pub(crate) async fn latest_request_from<'e, E>(
    executor: E,
    public_key: &str,
) -> Result<Option<RequestRow>, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM contact_requests
         WHERE public_key = ? ORDER BY created_at DESC, id LIMIT 1"
    ))
    .bind(public_key)
    .fetch_optional(executor)
    .await
    .map_err(|error| db_error("load latest contact request", error))
}

/// A request row to insert.
pub(crate) struct NewRequest<'a> {
    pub id: &'a str,
    pub direction: RequestDirection,
    pub public_key: &'a str,
    pub display_name: &'a str,
    pub color: u8,
    pub card: &'a str,
    pub card_digest: &'a str,
    pub endpoint_hint: &'a str,
    pub coordinator_fingerprint: &'a str,
    pub invite_trust: TrustKind,
    pub message_trust: TrustKind,
    pub expires_at: DateTime<Utc>,
}

pub(crate) async fn insert_request<'e, E>(
    executor: E,
    request: NewRequest<'_>,
    now: DateTime<Utc>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let time = format_time(now);
    sqlx::query(
        "INSERT INTO contact_requests (
            id, direction, public_key, display_name, color, card, card_digest, endpoint_hint,
            coordinator_fingerprint, invite_trust, message_trust, state, expires_at, created_at,
            updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
    )
    .bind(request.id)
    .bind(request.direction.as_str())
    .bind(request.public_key)
    .bind(request.display_name)
    .bind(i64::from(request.color))
    .bind(request.card)
    .bind(request.card_digest)
    .bind(request.endpoint_hint)
    .bind(request.coordinator_fingerprint)
    .bind(request.invite_trust.as_str())
    .bind(request.message_trust.as_str())
    .bind(format_time(request.expires_at))
    .bind(&time)
    .bind(&time)
    .execute(executor)
    .await
    .map_err(|error| db_error("store contact request", error))?;
    Ok(())
}

/// Removes pending rows in one direction from a person so a newer request can replace them.
pub(crate) async fn delete_pending<'e, E>(
    executor: E,
    direction: RequestDirection,
    public_key: &str,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "DELETE FROM contact_requests
         WHERE direction = ? AND public_key = ? AND state = 'pending'",
    )
    .bind(direction.as_str())
    .bind(public_key)
    .execute(executor)
    .await
    .map_err(|error| db_error("replace pending contact request", error))?;
    Ok(())
}

/// Moves every pending request in one direction from a person to a final state.
pub(crate) async fn close_pending_from<'e, E>(
    executor: E,
    direction: RequestDirection,
    public_key: &str,
    state: RequestState,
    now: DateTime<Utc>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE contact_requests
         SET state = ?, updated_at = ?
         WHERE direction = ? AND public_key = ? AND state = 'pending'",
    )
    .bind(state.as_str())
    .bind(format_time(now))
    .bind(direction.as_str())
    .bind(public_key)
    .execute(executor)
    .await
    .map_err(|error| db_error("close pending contact requests", error))?;
    Ok(())
}

/// Moves a request to a final state. With an expected revision, returns false on a mismatch.
pub(crate) async fn set_request_state<'e, E>(
    executor: E,
    id: &str,
    expected_revision: Option<i64>,
    state: RequestState,
    error_code: Option<&str>,
    now: DateTime<Utc>,
) -> Result<bool, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let result = sqlx::query(
        "UPDATE contact_requests
         SET state = ?, last_error_code = ?, updated_at = ?
         WHERE id = ? AND state = 'pending' AND (? IS NULL OR revision = ?)",
    )
    .bind(state.as_str())
    .bind(error_code)
    .bind(format_time(now))
    .bind(id)
    .bind(expected_revision)
    .bind(expected_revision)
    .execute(executor)
    .await
    .map_err(|error| db_error("update contact request", error))?;
    Ok(result.rows_affected() == 1)
}

#[cfg(any(desktop, test))]
pub(crate) async fn pending_received_count<'e, E>(executor: E) -> Result<i64, String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_scalar(
        "SELECT count(*) FROM contact_requests
         WHERE direction = 'received' AND state = 'pending'",
    )
    .fetch_one(executor)
    .await
    .map_err(|error| db_error("count pending contact requests", error))
}

/// Expires the oldest pending received requests until at most `keep` remain.
#[cfg(any(desktop, test))]
pub(crate) async fn expire_oldest_pending_received<'e, E>(
    executor: E,
    keep: i64,
    now: DateTime<Utc>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE contact_requests
         SET state = 'expired', updated_at = ?
         WHERE id IN (
            SELECT id FROM contact_requests
            WHERE direction = 'received' AND state = 'pending'
            ORDER BY created_at DESC, id DESC
            LIMIT -1 OFFSET ?
         )",
    )
    .bind(format_time(now))
    .bind(keep)
    .execute(executor)
    .await
    .map_err(|error| db_error("expire oldest contact requests", error))?;
    Ok(())
}

/// Records a delivery or polling attempt on a sent request.
pub(crate) async fn touch_request_attempt<'e, E>(
    executor: E,
    id: &str,
    now: DateTime<Utc>,
    error_code: Option<&str>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE contact_requests
         SET last_attempt_at = ?, last_error_code = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(format_time(now))
    .bind(error_code)
    .bind(format_time(now))
    .bind(id)
    .execute(executor)
    .await
    .map_err(|error| db_error("record contact request attempt", error))?;
    Ok(())
}

/// Expires every pending request whose deadline passed.
pub(crate) async fn expire_stale_requests<'e, E>(
    executor: E,
    now: DateTime<Utc>,
) -> Result<(), String>
where
    E: Executor<'e, Database = Sqlite>,
{
    let time = format_time(now);
    sqlx::query(
        "UPDATE contact_requests
         SET state = 'expired', updated_at = ?
         WHERE state = 'pending' AND expires_at <= ?",
    )
    .bind(&time)
    .bind(&time)
    .execute(executor)
    .await
    .map_err(|error| db_error("expire stale contact requests", error))?;
    Ok(())
}
