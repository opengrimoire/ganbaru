//! The anchor's authority over a vault's space: the keys it founds the space with, the charter,
//! the admin-signed proposals it places in its outbox, and the sequencing of proposals through
//! the engine.

use super::outbox::{Outbox, is_final};
use super::writer::{
    SelectedHead, SequencedCheck, SyncKey, SyncKeyStore, WriterRecord, ensure_device_key,
    sequenced_check,
};
use ganbaru_contacts::PersonKeyPair;
use ganbaru_handoff::discovery::DiscoveryKey;
use ganbaru_handoff::protocol::{
    DeviceKind as HandoffDeviceKind, MAX_SYNC_FORWARDED_PROPOSALS, encode_sync_proposal,
};
use ganbaru_sync::{AuthorizationView, Engine, LocalWriter, SpaceContext};
use ganbaru_sync_contracts::bounds::{DISCOVERY_KEY_BYTES, TLS_FINGERPRINT_BYTES};
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, Charter, DeviceKeyPair, DeviceKind, DevicePublicKey, DeviceRole,
    Digest32, Enroll, Entry, Handover, Proposal, ProposalRejection, RecoverySeed, Remove, SetRole,
    SignedCharter, SignedProposal, SpaceId, VersionVector, sign_charter, sign_proposal,
};
use sqlx::SqliteConnection;

/// The secrets an anchor holds for a space: its device key, the admin key it signs proposals
/// with, and the recovery seed whose derived admin key the charter names.
pub struct AnchorKeys {
    pub device: DeviceKeyPair,
    pub admin: AdminKeyPair,
    pub recovery: RecoverySeed,
}

/// Reads the anchor keys of a vault, creating each missing one once. Only founding a charter
/// calls this: a recovery seed the user saved and the store forgot is created anew, for the
/// next charter's kit. Key stores may block, so callers run this off the async runtime.
pub fn ensure_anchor_keys(keys: &dyn SyncKeyStore, vault_id: &str) -> Result<AnchorKeys, String> {
    let device = ensure_device_key(keys, vault_id)?;
    let admin = ensure_admin_key(keys, vault_id)?;
    let recovery = match pending_recovery_seed(keys, vault_id)? {
        Some(seed) => seed,
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

/// Reads the admin key of a vault, creating it once. A desktop that links to a coordinator
/// sends its public half at pairing, so an admin can later promote it without another link.
/// Key stores may block, so callers run this off the async runtime.
pub fn ensure_admin_key(keys: &dyn SyncKeyStore, vault_id: &str) -> Result<AdminKeyPair, String> {
    match read_admin_key(keys, vault_id)? {
        Some(admin) => Ok(admin),
        None => {
            let (pkcs8, admin) = AdminKeyPair::generate()
                .map_err(|error| format!("create sync admin key: {error}"))?;
            keys.write(vault_id, SyncKey::Admin, &pkcs8)?;
            Ok(admin)
        }
    }
}

/// Reads the admin key of a vault without creating one, `None` when this installation has none.
/// Key stores may block, so callers run this off the async runtime.
pub fn read_admin_key(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
) -> Result<Option<AdminKeyPair>, String> {
    keys.read(vault_id, SyncKey::Admin)?
        .map(|pkcs8| {
            AdminKeyPair::from_pkcs8(&pkcs8)
                .map_err(|error| format!("decode sync admin key: {error}"))
        })
        .transpose()
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
    charter: Digest32,
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
        added += usize::from(outbox.add(propose(space, charter, admin, entry, now_ms)?)?);
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
    charter: Digest32,
    admin: &AdminKeyPair,
    entry: Entry,
    now_ms: u64,
) -> Result<SignedProposal, String> {
    sign_proposal(
        &Proposal {
            space,
            charter,
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
    sequence_covering(
        engine,
        conn,
        ctx,
        writer,
        proposal,
        &VersionVector::new(),
        now_ms,
    )
    .await
}

/// [`sequence`] on a linked anchor: a removal's cutoffs cover at least `covered`, the stored
/// vector the coordinator last reported, since the anchor may store less of a device it
/// quarantined than the coordinator and members already applied.
pub async fn sequence_covering(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    proposal: &SignedProposal,
    covered: &VersionVector,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    let discovery_key = match proposal.proposal.entry {
        Entry::Remove(_) => *DiscoveryKey::generate()?.as_bytes(),
        _ => current_discovery_key(engine, conn, ctx).await?,
    };
    engine
        .sequence_proposal_covering(conn, ctx, proposal, discovery_key, covered, writer, now_ms)
        .await
        .map_err(|error| format!("sequence sync proposal: {error}"))
}

/// Sequences every waiting proposal of the anchor's outbox in order, records final rejections,
/// and drops the proposals the selected chain now holds. Removal cutoffs cover at least
/// `covered`, as in [`sequence_covering`]. Returns how many proposals were sequenced or
/// replayed.
pub async fn sequence_outbox(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    outbox: &mut Outbox,
    covered: &VersionVector,
    now_ms: u64,
) -> Result<usize, String> {
    let mut sequenced = 0;
    for proposal in outbox.pending() {
        let outcome =
            sequence_covering(engine, conn, ctx, writer, &proposal, covered, now_ms).await?;
        sequenced += usize::from(outcome.is_ok());
        outbox.settle(&proposal.hash(), outcome)?;
    }
    outbox.prune(engine, conn, ctx).await?;
    Ok(sequenced)
}

/// Bytes of encoded proposals one forward answer carries at most, well inside a control frame.
/// A single proposal larger than this still travels alone.
const FORWARD_BUDGET_BYTES: usize = 768 * 1024;

/// On a hub that is not the anchor, answers a proposal a linked device sent: the revision when
/// the selected chain holds it, the final rejection the anchor gave it, or `NotAnchor` after
/// keeping it in the outbox for the anchor to collect. A full outbox keeps nothing, and the
/// sender keeps the proposal in its own outbox either way.
pub async fn hold_for_anchor(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    outbox: &mut Outbox,
    proposal: SignedProposal,
) -> Result<Result<u64, ProposalRejection>, String> {
    let hash = proposal.hash();
    if let Some(revision) = engine
        .proposal_revision(conn, ctx, &hash)
        .await
        .map_err(|error| format!("read sync proposal revision: {error}"))?
    {
        return Ok(Ok(revision));
    }
    if let Some(rejection) = outbox.get(&hash).and_then(|entry| entry.rejection) {
        return Ok(Err(rejection));
    }
    if outbox.has_room() {
        outbox.add(proposal)?;
    }
    Ok(Err(ProposalRejection::NotAnchor))
}

/// Answers a forward request of `requester`. Only the device the selected log names as anchor
/// is answered: its final rejections are recorded, and the waiting proposals it should
/// sequence next are returned, oldest first and bounded by count and bytes, after proposals
/// the selected chain holds are dropped. Any other requester receives nothing and settles
/// nothing.
pub async fn collect_forwards(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    outbox: &mut Outbox,
    requester: &str,
    settled: &[(Digest32, ProposalRejection)],
) -> Result<Vec<SignedProposal>, String> {
    let anchors = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        .is_some_and(|view| view.anchor_device_id == requester);
    if !anchors {
        return Ok(Vec::new());
    }
    for (hash, rejection) in settled {
        outbox.settle(hash, Err(*rejection))?;
    }
    outbox.prune(engine, conn, ctx).await?;
    let mut bytes = 0;
    let mut forwarded = Vec::new();
    for proposal in outbox.pending() {
        let size = encode_sync_proposal(&proposal).len();
        if forwarded.len() == MAX_SYNC_FORWARDED_PROPOSALS
            || (!forwarded.is_empty() && bytes + size > FORWARD_BUDGET_BYTES)
        {
            break;
        }
        bytes += size;
        forwarded.push(proposal);
    }
    Ok(forwarded)
}

/// Sequences the proposals a hub forwarded through the anchor's writer and returns the final
/// rejections, which the next forward reports so the hub stops holding them. Accepted
/// proposals need no report: the hub finds them in the chain once it applies the entries.
/// Removal cutoffs cover at least `covered`, the hub's stored vector, as in
/// [`sequence_covering`].
pub async fn sequence_forwarded(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    proposals: &[SignedProposal],
    covered: &VersionVector,
    now_ms: u64,
) -> Result<Vec<(Digest32, ProposalRejection)>, String> {
    let mut rejected = Vec::new();
    for proposal in proposals {
        if let Err(rejection) =
            sequence_covering(engine, conn, ctx, writer, proposal, covered, now_ms).await?
            && is_final(rejection)
        {
            rejected.push((proposal.hash(), rejection));
        }
    }
    Ok(rejected)
}

/// The entry that makes an enrolled desktop an admin with `admin_key`, which lets it sign
/// proposals and take the anchor.
pub fn promote_entry(device_id: &str, admin_key: AdminPublicKey) -> Entry {
    Entry::SetRole(SetRole::Device {
        device_id: device_id.to_string(),
        role: DeviceRole::Admin(admin_key),
    })
}

/// The entry that names a new recovery admin key, as regenerating the recovery kit does.
pub fn recovery_entry(recovery: &RecoverySeed) -> Result<Entry, String> {
    Ok(Entry::SetRole(SetRole::Recovery {
        recovery_admin_key: recovery
            .recovery_admin_key()
            .map_err(|error| format!("derive the recovery admin key: {error}"))?
            .public_key(),
    }))
}

/// Replaces the vault's recovery kit and returns the new seed. The `set role` proposal naming
/// the new recovery admin key enters the outbox first and the seed is stored after, so a full
/// outbox changes nothing and a failed store withdraws the proposal: the log never names a key
/// whose seed is gone. The key store keeps the new seed until the user saves the kit. A kit
/// that still waits is never overwritten while it may still sign: one the log names must be
/// saved first, and one whose proposal still waits must be sequenced or rejected first. Key
/// stores may block, so callers run this off the async runtime.
pub fn regenerate_recovery_kit(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
    outbox: &mut Outbox,
    view: &AuthorizationView,
    space: SpaceId,
    admin: &AdminKeyPair,
    now_ms: u64,
) -> Result<RecoverySeed, String> {
    if let Some(waiting) = pending_recovery_seed(keys, vault_id)? {
        let key = recovery_public_key(&waiting)?;
        if key == view.recovery_admin_key {
            return Err("save the waiting recovery kit before replacing it".to_string());
        }
        let proposed = outbox.entries().iter().any(|entry| {
            entry.rejection.is_none()
                && matches!(
                    &entry.proposal.proposal.entry,
                    Entry::SetRole(SetRole::Recovery { recovery_admin_key })
                        if *recovery_admin_key == key
                )
        });
        if proposed {
            return Err("the waiting recovery kit is not sequenced yet".to_string());
        }
    }
    let seed =
        RecoverySeed::generate().map_err(|error| format!("create sync recovery seed: {error}"))?;
    let proposal = propose(space, view.charter, admin, recovery_entry(&seed)?, now_ms)?;
    let hash = proposal.hash();
    outbox.add(proposal)?;
    if let Err(error) = keys.write(vault_id, SyncKey::RecoverySeed, seed.as_bytes()) {
        outbox.withdraw(&hash)?;
        return Err(error);
    }
    Ok(seed)
}

fn recovery_public_key(seed: &RecoverySeed) -> Result<AdminPublicKey, String> {
    Ok(seed
        .recovery_admin_key()
        .map_err(|error| format!("derive the recovery admin key: {error}"))?
        .public_key())
}

/// Whether the selected log names the recovery admin key `seed` derives, so its kit signs.
pub fn recovery_kit_active(view: &AuthorizationView, seed: &RecoverySeed) -> Result<bool, String> {
    Ok(recovery_public_key(seed)? == view.recovery_admin_key)
}

/// The recovery seed whose kit waits for the user to save it, if the key store still holds
/// one. Key stores may block, so callers run this off the async runtime.
pub fn pending_recovery_seed(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
) -> Result<Option<RecoverySeed>, String> {
    keys.read(vault_id, SyncKey::RecoverySeed)?
        .map(|bytes| {
            bytes
                .try_into()
                .map(RecoverySeed::from_bytes)
                .map_err(|_| "sync recovery seed is malformed".to_string())
        })
        .transpose()
}

/// Deletes the recovery seed once the user saved its kit, so no device keeps it. A kit the
/// selected log does not name yet is kept, because forgetting it could leave no kit that signs.
/// Key stores may block, so callers run this off the async runtime.
pub fn forget_recovery_seed(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
    view: &AuthorizationView,
) -> Result<(), String> {
    if let Some(seed) = pending_recovery_seed(keys, vault_id)?
        && !recovery_kit_active(view, &seed)?
    {
        return Err(
            "the anchor has not sequenced this recovery kit yet; keep it until it is active"
                .to_string(),
        );
    }
    keys.remove(vault_id, SyncKey::RecoverySeed)
}

/// The recovery admin key `kit_code` derives. Refuses a code that does not parse or a kit
/// whose key the selected log does not name. Callers keep the key only as long as they sign
/// with it.
pub fn recovery_admin(view: &AuthorizationView, kit_code: &str) -> Result<AdminKeyPair, String> {
    let seed = RecoverySeed::parse_kit_code(kit_code)
        .map_err(|error| format!("read the recovery kit: {error}"))?;
    let recovery = seed
        .recovery_admin_key()
        .map_err(|error| format!("derive the recovery admin key: {error}"))?;
    if recovery.public_key() != view.recovery_admin_key {
        return Err("this recovery kit does not belong to the vault's current log".to_string());
    }
    Ok(recovery)
}

/// Signs, with the recovery admin key `kit_code` derives, the proposal that makes the enrolled
/// desktop `device_id` an admin with `admin_key`, so it can take the anchor. The derived key
/// lives only inside this call. Refuses a code that does not parse or a kit whose key the
/// selected log does not name.
pub fn recovery_promotion(
    view: &AuthorizationView,
    space: SpaceId,
    kit_code: &str,
    device_id: &str,
    admin_key: AdminPublicKey,
    now_ms: u64,
) -> Result<SignedProposal, String> {
    let recovery = recovery_admin(view, kit_code)?;
    propose(
        space,
        view.charter,
        &recovery,
        promote_entry(device_id, admin_key),
        now_ms,
    )
}

/// The enrolled devices other than `own_device_id` and the anchor that are not removed and
/// for which `linked` is false. An admin hub proposes their removal, because the coordinator
/// links every device of the space.
pub fn unlinked_devices(
    view: &AuthorizationView,
    own_device_id: &str,
    linked: impl Fn(&str) -> bool,
) -> Vec<String> {
    view.devices
        .iter()
        .filter(|device| {
            device.removed_revision.is_none()
                && device.device_id != view.anchor_device_id
                && device.device_id != own_device_id
                && !linked(&device.device_id)
        })
        .map(|device| device.device_id.clone())
        .collect()
}

/// The devices the selected log removed that `linked` still reports as linked. The coordinator
/// unlinks them, so pairing never serves a device the log removed, whichever admin proposed the
/// removal.
pub fn removed_linked_devices(
    view: &AuthorizationView,
    linked: impl Fn(&str) -> bool,
) -> Vec<String> {
    view.devices
        .iter()
        .filter(|device| device.removed_revision.is_some() && linked(&device.device_id))
        .map(|device| device.device_id.clone())
        .collect()
}

/// Whether the selected log names `admin_key` as the role of the enrolled, unremoved device
/// `device_id`. Only such a device may take the anchor.
pub fn is_admin(view: &AuthorizationView, device_id: &str, admin_key: &AdminPublicKey) -> bool {
    view.devices.iter().any(|device| {
        device.device_id == device_id
            && device.removed_revision.is_none()
            && matches!(&device.role, DeviceRole::Admin(key) if key == admin_key)
    })
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
    let proposal = propose(
        ctx.space,
        charter_hash(engine, conn, ctx).await?,
        admin,
        enroll_entry(enrollment),
        now_ms,
    )?;
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
    let proposal = propose(
        ctx.space,
        charter_hash(engine, conn, ctx).await?,
        admin,
        remove_entry(device_id),
        now_ms,
    )?;
    accepted(sequence(engine, conn, ctx, writer, &proposal, now_ms).await?)
}

fn accepted(outcome: Result<u64, ProposalRejection>) -> Result<u64, String> {
    outcome.map_err(|rejection| format!("sync proposal rejected: {rejection}"))
}

/// Hash of the installed charter, which every proposal for the space names.
pub async fn charter_hash(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
) -> Result<Digest32, String> {
    engine
        .charter(conn, ctx)
        .await
        .map_err(|error| format!("read sync charter: {error}"))?
        .map(|charter| charter.hash())
        .ok_or_else(|| "the sync space has no charter".to_string())
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

/// Compares the anchor high-water `record` names with the selected head of the space. A space
/// without a charter, or a record without a high-water, is current.
pub async fn restore_check(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    record: &WriterRecord,
) -> Result<SequencedCheck, String> {
    let Some(view) = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
    else {
        return Ok(SequencedCheck::Current);
    };
    Ok(sequenced_check(
        record.sequenced.as_ref(),
        SelectedHead {
            charter: view.charter,
            epoch: view.epoch,
            revision: view.revision,
        },
    ))
}

/// Signs with `admin` and seals a handover from the selected head to the device of the local
/// writer, opening the next epoch. `old_anchor_vector` is the old anchor's stored
/// vector when this device takes over from another anchor; a restored anchor handing over to
/// itself passes `None`, since its own log is what it lost. Returns the revision, or why the
/// engine rejected the handover. A log holding only its charter has no entry to continue from.
pub async fn seal_handover_here(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    writer: &mut LocalWriter<'_>,
    old_anchor_vector: Option<VersionVector>,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    let view = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        .ok_or_else(|| "the sync space has no charter".to_string())?;
    if view.revision == 0 {
        return Err(
            "the log holds only its charter, so no epoch can continue it; link the devices again"
                .to_string(),
        );
    }
    let epoch = view
        .epoch
        .checked_add(1)
        .ok_or_else(|| "the sync epoch is exhausted".to_string())?;
    let proposal = propose(
        ctx.space,
        view.charter,
        admin,
        Entry::Handover(Handover {
            new_anchor_device_id: writer.certificate.certificate.device_id.clone(),
            epoch,
            continues_from_revision: view.revision,
            continues_from_hash: view.entry_hash,
        }),
        now_ms,
    )?;
    engine
        .seal_handover(conn, ctx, &proposal, old_anchor_vector, writer, now_ms)
        .await
        .map_err(|error| format!("seal sync handover: {error}"))
}

/// Forces the anchor to this desktop after the anchor is lost: signs with `signer` and seals a
/// handover without an old anchor vector, so operations only the lost anchor stored can become
/// provisional again. When `promote` is set, which is when the recovery key signs for a desktop
/// that is not an admin yet, the new anchor then sequences the promotion of its own device to
/// admin with that key, signed by the same `signer`. Returns the revision of the last entry, or
/// why the engine rejected one.
pub async fn force_handover_here(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    signer: &AdminKeyPair,
    promote: Option<AdminPublicKey>,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    let sealed = seal_handover_here(engine, conn, ctx, signer, writer, None, now_ms).await?;
    let (Ok(revision), Some(admin_key)) = (sealed, promote) else {
        return Ok(sealed);
    };
    let view = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        .ok_or_else(|| "the sync space has no charter".to_string())?;
    let device_id = writer.certificate.certificate.device_id.clone();
    let promotion = propose(
        ctx.space,
        view.charter,
        signer,
        promote_entry(&device_id, admin_key),
        now_ms,
    )?;
    match sequence(engine, conn, ctx, writer, &promotion, now_ms).await? {
        Ok(promoted) => Ok(Ok(promoted.max(revision))),
        Err(rejection) => Ok(Err(rejection)),
    }
}

fn millis(now_ms: u64) -> Result<i64, String> {
    i64::try_from(now_ms).map_err(|_| "clock is out of range".to_string())
}

/// `vector` without the writers `excluded` names. A desktop taking the anchor merges the
/// coordinator's stored vector into the old anchor vector without the writers of devices it
/// quarantined: it never stores those writers, so naming them would make every removal wait as
/// `catching_up`, including the removal of the quarantined device.
pub fn without_writers(vector: &VersionVector, excluded: &VersionVector) -> VersionVector {
    let mut kept = VersionVector::new();
    for (writer, seq) in vector.iter() {
        if excluded.get(&writer) == 0 {
            kept.advance(writer, seq);
        }
    }
    kept
}
