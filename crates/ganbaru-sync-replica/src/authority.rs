//! The anchor's authority over a vault's space: the keys it founds the space with, the charter,
//! the admin-signed proposals it places in its outbox, and the sequencing of proposals through
//! the engine.

use super::outbox::Outbox;
use super::writer::{SyncKey, SyncKeyStore, ensure_device_key};
use ganbaru_contacts::PersonKeyPair;
use ganbaru_handoff::discovery::DiscoveryKey;
use ganbaru_handoff::protocol::DeviceKind as HandoffDeviceKind;
use ganbaru_sync::{Engine, LocalWriter, SpaceContext};
use ganbaru_sync_contracts::bounds::{DISCOVERY_KEY_BYTES, TLS_FINGERPRINT_BYTES};
use ganbaru_sync_contracts::{
    AdminKeyPair, Charter, DeviceKeyPair, DeviceKind, DevicePublicKey, DeviceRole, Enroll, Entry,
    Proposal, ProposalRejection, RecoverySeed, Remove, SignedCharter, SignedProposal, SpaceId,
    sign_charter, sign_proposal,
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

/// The entry that enrolls a linked device as a member.
pub fn enroll_entry(enrollment: Enrollment) -> Entry {
    Entry::Enroll(Enroll {
        device_id: enrollment.device_id,
        device_key: enrollment.device_key,
        tls_fingerprint: enrollment.tls_fingerprint,
        label: enrollment.label,
        device_kind: enrollment.device_kind,
        role: DeviceRole::Member,
    })
}

/// The entry that removes a device.
pub fn remove_entry(device_id: &str) -> Entry {
    Entry::Remove(Remove {
        device_id: device_id.to_string(),
    })
}

/// The membership changes a reconciliation of linked devices with the space wants.
#[derive(Default)]
pub struct Reconciliation {
    /// Linked devices the space does not know.
    pub enroll: Vec<Enrollment>,
    /// Enrolled devices that are no longer linked.
    pub remove: Vec<String>,
}

/// Places a proposal signed by `admin` in the outbox for each change of `changes` that no
/// proposal exists for yet, waiting or finally rejected, matched by change kind and device id.
/// A finally rejected change is not proposed again while its rejection stays in the outbox, so
/// a removed device that links again stays removed. An outbox full of waiting proposals leaves
/// the remaining changes for a later pass instead of failing it. Returns how many proposals it
/// added.
pub fn queue_reconciliation(
    outbox: &mut Outbox,
    space: SpaceId,
    admin: &AdminKeyPair,
    changes: Reconciliation,
    now_ms: u64,
) -> Result<usize, String> {
    let entries = changes.enroll.into_iter().map(enroll_entry).chain(
        changes
            .remove
            .iter()
            .map(|device_id| remove_entry(device_id)),
    );
    let mut added = 0;
    for entry in entries {
        if outbox.find(|queued| same_change(queued, &entry)).is_some() {
            continue;
        }
        if !outbox.has_room() {
            break;
        }
        added += usize::from(outbox.add(propose(space, admin, entry, now_ms)?)?);
    }
    Ok(added)
}

/// Whether two entries make the same membership change to the same device.
fn same_change(left: &Entry, right: &Entry) -> bool {
    match (left, right) {
        (Entry::Enroll(left), Entry::Enroll(right)) => left.device_id == right.device_id,
        (Entry::Remove(left), Entry::Remove(right)) => left.device_id == right.device_id,
        _ => false,
    }
}

/// Signs `entry` as a proposal of the admin key for the space.
pub fn propose(
    space: SpaceId,
    admin: &AdminKeyPair,
    entry: Entry,
    now_ms: u64,
) -> Result<SignedProposal, String> {
    sign_proposal(
        &Proposal {
            space,
            author: admin.public_key(),
            created_at_ms: millis(now_ms)?,
            entry,
        },
        admin,
    )
    .map_err(|error| format!("sign sync proposal: {error}"))
}

/// Sequences an admin-signed proposal through the anchor's writer. A removal takes a fresh
/// discovery key; every other entry ignores it. Returns the revision, or why the anchor
/// rejected the proposal; a proposal already in the selected chain returns its revision.
pub async fn sequence(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    proposal: &SignedProposal,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    let discovery_key = match proposal.proposal.entry {
        Entry::Remove(_) => *DiscoveryKey::generate()?.as_bytes(),
        _ => current_discovery_key(engine, conn, ctx).await?,
    };
    engine
        .sequence_proposal(conn, ctx, proposal, discovery_key, writer, now_ms)
        .await
        .map_err(|error| format!("sequence sync proposal: {error}"))
}

/// Sequences every waiting proposal of the anchor's outbox in order, records final rejections,
/// and drops the proposals the selected chain now holds. Returns how many proposals were
/// sequenced or replayed.
pub async fn sequence_outbox(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    outbox: &mut Outbox,
    now_ms: u64,
) -> Result<usize, String> {
    let mut sequenced = 0;
    for proposal in outbox.pending() {
        let outcome = sequence(engine, conn, ctx, writer, &proposal, now_ms).await?;
        sequenced += usize::from(outcome.is_ok());
        outbox.settle(&proposal.hash(), outcome)?;
    }
    outbox.prune(engine, conn, ctx).await?;
    Ok(sequenced)
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
    let proposal = propose(ctx.space, admin, enroll_entry(enrollment), now_ms)?;
    accepted(sequence(engine, conn, ctx, writer, &proposal, now_ms).await?)
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
    let proposal = propose(ctx.space, admin, remove_entry(device_id), now_ms)?;
    accepted(sequence(engine, conn, ctx, writer, &proposal, now_ms).await?)
}

fn accepted(outcome: Result<u64, ProposalRejection>) -> Result<u64, String> {
    outcome.map_err(|rejection| format!("sync proposal rejected: {rejection}"))
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

fn millis(now_ms: u64) -> Result<i64, String> {
    i64::try_from(now_ms).map_err(|_| "clock is out of range".to_string())
}
