//! The anchor's authority over a vault's space: the keys it founds the space with, the charter,
//! and the enroll and remove proposals it sequences directly through the engine.

use super::writer::{SyncKey, SyncKeyStore, ensure_device_key};
use ganbaru_contacts::PersonKeyPair;
use ganbaru_handoff::discovery::DiscoveryKey;
use ganbaru_handoff::protocol::DeviceKind as HandoffDeviceKind;
use ganbaru_sync::{Engine, LocalWriter, SpaceContext};
use ganbaru_sync_contracts::bounds::{DISCOVERY_KEY_BYTES, TLS_FINGERPRINT_BYTES};
use ganbaru_sync_contracts::{
    AdminKeyPair, Charter, DeviceKeyPair, DeviceKind, DevicePublicKey, DeviceRole, Enroll, Entry,
    Proposal, ProposalRejection, RecoverySeed, Remove, SignedCharter, sign_charter, sign_proposal,
};
use sqlx::SqliteConnection;

/// The secrets an anchor holds for a space: its device key, the admin key it signs proposals
/// with, and the recovery seed whose derived admin key the charter names.
pub struct AnchorKeys {
    pub device: DeviceKeyPair,
    pub admin: AdminKeyPair,
    pub recovery: RecoverySeed,
}

/// Reads the anchor keys of a vault, creating each missing one once. Key stores may block, so
/// callers run this off the async runtime.
pub fn ensure_anchor_keys(keys: &dyn SyncKeyStore, vault_id: &str) -> Result<AnchorKeys, String> {
    let device = ensure_device_key(keys, vault_id)?;
    let admin = match keys.read(vault_id, SyncKey::Admin)? {
        Some(pkcs8) => AdminKeyPair::from_pkcs8(&pkcs8)
            .map_err(|error| format!("decode sync admin key: {error}"))?,
        None => {
            let (pkcs8, admin) = AdminKeyPair::generate()
                .map_err(|error| format!("create sync admin key: {error}"))?;
            keys.write(vault_id, SyncKey::Admin, &pkcs8)?;
            admin
        }
    };
    let recovery = match keys.read(vault_id, SyncKey::RecoverySeed)? {
        Some(bytes) => RecoverySeed::from_bytes(
            bytes
                .try_into()
                .map_err(|_| "sync recovery seed is malformed".to_string())?,
        ),
        None => {
            let seed = RecoverySeed::generate()
                .map_err(|error| format!("create sync recovery seed: {error}"))?;
            keys.write(vault_id, SyncKey::RecoverySeed, seed.as_bytes())?;
            seed
        }
    };
    Ok(AnchorKeys {
        device,
        admin,
        recovery,
    })
}

/// What the first anchor founds a space with besides its keys.
pub struct Founding<'a> {
    /// Handoff device id of the anchor.
    pub device_id: &'a str,
    /// Fingerprint of the TLS certificate the anchor presents.
    pub tls_fingerprint: [u8; TLS_FINGERPRINT_BYTES],
    /// Discovery key the vault's linked devices share.
    pub discovery_key: [u8; DISCOVERY_KEY_BYTES],
    /// Vault owner's person key, which signs the charter.
    pub person: &'a PersonKeyPair,
    /// Founding time in Unix milliseconds.
    pub now_ms: u64,
}

/// Returns the installed charter, or founds the space with a charter naming this installation
/// as its anchor and installs it.
pub async fn ensure_charter(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    anchor: &AnchorKeys,
    founding: &Founding<'_>,
) -> Result<SignedCharter, String> {
    if let Some(charter) = engine
        .charter(conn, ctx)
        .await
        .map_err(|error| format!("read sync charter: {error}"))?
    {
        return Ok(charter);
    }
    let recovery_admin_key = anchor
        .recovery
        .recovery_admin_key()
        .map_err(|error| format!("derive the recovery admin key: {error}"))?
        .public_key();
    let charter = sign_charter(
        &Charter {
            space: ctx.space,
            anchor_device_id: founding.device_id.to_string(),
            anchor_device_key: anchor.device.public_key(),
            anchor_tls_fingerprint: founding.tls_fingerprint,
            anchor_admin_key: anchor.admin.public_key(),
            recovery_admin_key,
            discovery_key: founding.discovery_key,
            created_at_ms: millis(founding.now_ms)?,
        },
        founding.person,
    )
    .map_err(|error| format!("sign sync charter: {error}"))?;
    engine
        .install_charter(conn, ctx, &charter)
        .await
        .map_err(|error| format!("install sync charter: {error}"))?;
    Ok(charter)
}

/// A linked device the anchor enrolls as a member.
pub struct Enrollment {
    pub device_id: String,
    pub device_key: DevicePublicKey,
    pub tls_fingerprint: [u8; TLS_FINGERPRINT_BYTES],
    pub label: String,
    pub device_kind: DeviceKind,
}

/// The authorization form factor of a handoff device kind. An unknown kind enrolls as mobile,
/// which can never hold an admin role.
pub fn device_kind(kind: HandoffDeviceKind) -> DeviceKind {
    match kind {
        HandoffDeviceKind::Computer => DeviceKind::Desktop,
        HandoffDeviceKind::Phone | HandoffDeviceKind::Unknown => DeviceKind::Mobile,
    }
}

/// Enrolls a device as a member through the anchor's writer and returns the revision.
pub async fn enroll(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    writer: &mut LocalWriter<'_>,
    enrollment: Enrollment,
    now_ms: u64,
) -> Result<u64, String> {
    let entry = Entry::Enroll(Enroll {
        device_id: enrollment.device_id,
        device_key: enrollment.device_key,
        tls_fingerprint: enrollment.tls_fingerprint,
        label: enrollment.label,
        device_kind: enrollment.device_kind,
        role: DeviceRole::Member,
    });
    let discovery_key = current_discovery_key(engine, conn, ctx).await?;
    sequence(
        engine,
        conn,
        ctx,
        admin,
        writer,
        entry,
        discovery_key,
        now_ms,
    )
    .await
}

/// Removes a device through the anchor's writer with a fresh discovery key and returns the
/// revision. The removal sets the cutoff of each of the device's writers.
pub async fn remove(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    writer: &mut LocalWriter<'_>,
    device_id: &str,
    now_ms: u64,
) -> Result<u64, String> {
    let entry = Entry::Remove(Remove {
        device_id: device_id.to_string(),
    });
    let discovery_key = *DiscoveryKey::generate()?.as_bytes();
    sequence(
        engine,
        conn,
        ctx,
        admin,
        writer,
        entry,
        discovery_key,
        now_ms,
    )
    .await
}

/// The discovery key of the charter, which a non-removal entry ignores.
async fn current_discovery_key(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
) -> Result<[u8; DISCOVERY_KEY_BYTES], String> {
    engine
        .charter(conn, ctx)
        .await
        .map_err(|error| format!("read sync charter: {error}"))?
        .map(|charter| charter.charter.discovery_key)
        .ok_or_else(|| "the sync space has no charter".to_string())
}

#[allow(clippy::too_many_arguments)]
async fn sequence(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    writer: &mut LocalWriter<'_>,
    entry: Entry,
    discovery_key: [u8; DISCOVERY_KEY_BYTES],
    now_ms: u64,
) -> Result<u64, String> {
    let proposal = sign_proposal(
        &Proposal {
            space: ctx.space,
            author: admin.public_key(),
            created_at_ms: millis(now_ms)?,
            entry,
        },
        admin,
    )
    .map_err(|error| format!("sign sync proposal: {error}"))?;
    engine
        .sequence_proposal(conn, ctx, &proposal, discovery_key, writer, now_ms)
        .await
        .map_err(|error| format!("sequence sync proposal: {error}"))?
        .map_err(|rejection: ProposalRejection| format!("sync proposal rejected: {rejection:?}"))
}

fn millis(now_ms: u64) -> Result<i64, String> {
    i64::try_from(now_ms).map_err(|_| "clock is out of range".to_string())
}
