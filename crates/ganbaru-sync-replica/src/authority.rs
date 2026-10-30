//! The anchor's authority over a vault's space: the keys it founds the space with, the charter,
//! the admin-signed proposals it places in its outbox, and the sequencing of proposals through
//! the engine.

use std::collections::BTreeSet;

use super::keys::KitKeys;
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
use ganbaru_sync::{
    AuthorizationView, ContestEntry, ContestRejection, Engine, HandoverPoint, LocalWriter,
    SpaceContext, SpaceKeyHolder,
};
use ganbaru_sync_contracts::bounds::{DISCOVERY_KEY_BYTES, TLS_FINGERPRINT_BYTES};
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, Charter, DeviceKey, DeviceKeyPair, DeviceKind, DevicePublicKey,
    DeviceRole, Digest32, EncryptionKeyPair, EncryptionPublicKey, Enroll, Entry, Handover,
    Proposal, ProposalRejection, RecoveryKey, RecoveryPublicKeys, RecoverySeed, Remove, SetRole,
    SignedCharter, SignedProposal, SpaceId, VersionVector, sign_charter, sign_device_proposal,
    sign_proposal,
};
use sqlx::SqliteConnection;

/// The keys an anchor founds a space with: its device key, the admin key it signs proposals
/// with, and the public keys of the recovery kit, whose admin key the charter names and whose
/// encryption key the first recovery key entry names.
pub struct AnchorKeys {
    pub device: DeviceKeyPair,
    pub admin: AdminKeyPair,
    pub recovery: RecoveryPublicKeys,
}

/// Reads the anchor keys of a vault, creating each missing one once. Only founding a charter
/// calls this. `stored_kit` is the kit the vault database keeps, as for a kit created before the
/// vault linked, and founds the space as it is. Without one, a kit that waits unsaved in the key
/// store founds it, and otherwise a new kit is created and waits for the user to save it. The
/// caller stores the returned kit's public keys in the vault database. Key stores may block, so
/// callers run this off the async runtime.
pub fn ensure_anchor_keys(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
    stored_kit: Option<RecoveryPublicKeys>,
) -> Result<AnchorKeys, String> {
    let device = ensure_device_key(keys, vault_id)?;
    let admin = ensure_admin_key(keys, vault_id)?;
    let recovery = match stored_kit {
        Some(kit) => kit,
        None => match pending_recovery_seed(keys, vault_id)? {
            Some(seed) => recovery_public_keys(&seed)?,
            None => recovery_public_keys(&create_recovery_seed(keys, vault_id)?)?,
        },
    };
    Ok(AnchorKeys {
        device,
        admin,
        recovery,
    })
}

/// Creates a recovery seed and keeps it in the key store until the user saves its kit,
/// replacing any seed that waits there. Key stores may block, so callers run this off the
/// async runtime.
pub fn create_recovery_seed(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
) -> Result<RecoverySeed, String> {
    let seed =
        RecoverySeed::generate().map_err(|error| format!("create sync recovery seed: {error}"))?;
    keys.write(vault_id, SyncKey::RecoverySeed, seed.as_bytes())?;
    Ok(seed)
}

/// The public keys of the kit `seed` makes.
pub fn recovery_public_keys(seed: &RecoverySeed) -> Result<RecoveryPublicKeys, String> {
    seed.public_keys()
        .map_err(|error| format!("derive the recovery keys: {error}"))
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
    let recovery_admin_key = anchor.recovery.admin;
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
        &BTreeSet::new(),
        now_ms,
    )
    .await
}

/// [`sequence`] on a linked anchor: a removal's cutoffs cover at least `covered`, the stored
/// vector the coordinator last reported, since the anchor may store less of a device it
/// quarantined than the coordinator and members already applied. A device key entry of a device
/// in `quarantined` is refused, so a device this installation distrusts receives no envelope.
#[allow(clippy::too_many_arguments)]
pub async fn sequence_covering(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    proposal: &SignedProposal,
    covered: &VersionVector,
    quarantined: &BTreeSet<String>,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    if let Entry::DeviceKey(entry) = &proposal.proposal.entry
        && quarantined.contains(&entry.device_id)
    {
        return Ok(Err(ProposalRejection::DeviceQuarantined));
    }
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
/// `covered`, and device key entries of devices in `quarantined` are refused, as in
/// [`sequence_covering`]. Returns how many proposals were sequenced or replayed.
#[allow(clippy::too_many_arguments)]
pub async fn sequence_outbox(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    outbox: &mut Outbox,
    covered: &VersionVector,
    quarantined: &BTreeSet<String>,
    now_ms: u64,
) -> Result<usize, String> {
    let mut sequenced = 0;
    for proposal in outbox.pending() {
        let outcome = sequence_covering(
            engine,
            conn,
            ctx,
            writer,
            &proposal,
            covered,
            quarantined,
            now_ms,
        )
        .await?;
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
/// Removal cutoffs cover at least `covered`, the hub's stored vector, and device key entries of
/// devices in `quarantined` are refused, as in [`sequence_covering`].
#[allow(clippy::too_many_arguments)]
pub async fn sequence_forwarded(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    proposals: &[SignedProposal],
    covered: &VersionVector,
    quarantined: &BTreeSet<String>,
    now_ms: u64,
) -> Result<Vec<(Digest32, ProposalRejection)>, String> {
    let mut rejected = Vec::new();
    for proposal in proposals {
        if let Err(rejection) = sequence_covering(
            engine,
            conn,
            ctx,
            writer,
            proposal,
            covered,
            quarantined,
            now_ms,
        )
        .await?
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

/// Replaces the vault's recovery kit and returns the new seed. Two proposals enter the outbox:
/// the recovery key entry naming the new recovery encryption key, then the `set role` naming the
/// new recovery admin key. The recovery key goes first, so the key epoch it opens and every
/// later one seal no envelope to the old kit. The seed is stored after both, so a full outbox
/// changes nothing and a failed store withdraws them: the log never names a key whose seed is
/// gone. The key store keeps the new seed until the user saves the kit. A kit that still waits
/// is never overwritten while it may still sign: one the log names must be saved first, and one
/// whose proposal still waits must be sequenced or rejected first. Key stores may block, so
/// callers run this off the async runtime.
pub fn regenerate_recovery_kit(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
    outbox: &mut Outbox,
    view: &AuthorizationView,
    space: SpaceId,
    admin: &AdminKeyPair,
    now_ms: u64,
) -> Result<RecoverySeed, String> {
    // The anchor rejects a recovery key entry another admin key signs, which would leave the
    // new kit's admin key without its encryption key.
    if !is_admin(view, &view.anchor_device_id, &admin.public_key()) {
        return Err("only the anchor computer replaces the recovery kit".to_string());
    }
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
    let public = recovery_public_keys(&seed)?;
    let mut added = Vec::new();
    for entry in [
        recovery_key_entry(public.encryption),
        recovery_entry(&seed)?,
    ] {
        let proposal = propose(space, view.charter, admin, entry, now_ms)?;
        let hash = proposal.hash();
        if let Err(error) = outbox.add(proposal) {
            withdraw_all(outbox, &added)?;
            return Err(error);
        }
        added.push(hash);
    }
    if let Err(error) = keys.write(vault_id, SyncKey::RecoverySeed, seed.as_bytes()) {
        withdraw_all(outbox, &added)?;
        return Err(error);
    }
    Ok(seed)
}

fn withdraw_all(outbox: &mut Outbox, hashes: &[Digest32]) -> Result<(), String> {
    hashes.iter().try_for_each(|hash| outbox.withdraw(hash))
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

/// Deletes the recovery seed of a vault without a sync space once the user saved its kit. The
/// seed is kept unless `stored`, the kit keys the vault database keeps for charter creation at
/// first link, are the keys it derives, because forgetting it could leave a founding kit nobody
/// holds. Key stores may block, so callers run this off the async runtime.
pub fn forget_unlinked_recovery_seed(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
    stored: Option<RecoveryPublicKeys>,
) -> Result<(), String> {
    if let Some(seed) = pending_recovery_seed(keys, vault_id)?
        && stored != Some(recovery_public_keys(&seed)?)
    {
        return Err("the vault does not keep this recovery kit; create it again".to_string());
    }
    keys.remove(vault_id, SyncKey::RecoverySeed)
}

/// The recovery admin key `kit_code` derives, whether or not any log names it. Refuses a code
/// that does not parse. A forced handover uses it unchecked, because a stolen anchor may have
/// replaced the recovery key on the selected chain, and the engine admits the handover only
/// where the key is of kit lineage. Callers keep the key only as long as they sign with it.
pub fn kit_admin(kit_code: &str) -> Result<AdminKeyPair, String> {
    let seed = RecoverySeed::parse_kit_code(kit_code)
        .map_err(|error| format!("read the recovery kit: {error}"))?;
    seed.recovery_admin_key()
        .map_err(|error| format!("derive the recovery admin key: {error}"))
}

/// The recovery encryption key `kit_code` derives, which opens every key epoch sealed while the
/// log names it. Refuses a code that does not parse. Callers keep the key only while the kit
/// flow runs.
pub fn kit_encryption(kit_code: &str) -> Result<EncryptionKeyPair, String> {
    let seed = RecoverySeed::parse_kit_code(kit_code)
        .map_err(|error| format!("read the recovery kit: {error}"))?;
    seed.recovery_encryption_key()
        .map_err(|error| format!("derive the recovery encryption key: {error}"))
}

/// The recovery admin key `kit_code` derives. Refuses a code that does not parse or a kit
/// whose key the selected log does not name. Callers keep the key only as long as they sign
/// with it.
pub fn recovery_admin(view: &AuthorizationView, kit_code: &str) -> Result<AdminKeyPair, String> {
    let recovery = kit_admin(kit_code)?;
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

/// The enrolled devices other than `own_device_id`, the anchor, and servers that are not removed and
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
                && device.kind != DeviceKind::Server
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
    if !opens_current_epoch(
        &view,
        &writer.certificate.certificate.device_id,
        writer.keys,
    ) {
        return Err(KeylessAnchor::Unpublished.to_string());
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

/// What a forced handover to this desktop signs and seals after it.
pub struct ForcedHandover<'a> {
    /// The admin key or kit recovery key that signs the handover and the entries after it.
    pub signer: &'a AdminKeyPair,
    /// The admin key this desktop is promoted with, set when the recovery key signs for a
    /// desktop that is not an admin yet.
    pub promote: Option<AdminPublicKey>,
    /// The kit point to continue from, one the contest view offers; `None` continues from the
    /// default point.
    pub point: Option<ContestEntry>,
    /// Devices the user marked in the device review of a kit recovery, removed under the kit
    /// key after the handover. Only a handover the kit key signs removes them.
    pub removals: Vec<String>,
    /// The recovery encryption key the kit derives, set on a kit recovery. It opens the
    /// recovery envelope of the current key epoch, so this desktop can publish its own device
    /// key and seal the rotations after the handover before an envelope to its own key exists,
    /// as long as no regeneration replaced it.
    pub recovery: Option<&'a EncryptionKeyPair>,
}

/// Whether a desktop that takes the anchor without the kit opens the current key epoch: the
/// space has no key epoch yet, or the selected log names the encryption key of `keys` for the
/// live device `device_id`, so an envelope of every later epoch reaches it. An anchor that
/// opens none could seal neither its own device key nor any rotation, so every removal would
/// wait forever.
pub fn opens_current_epoch(
    view: &AuthorizationView,
    device_id: &str,
    keys: Option<&dyn SpaceKeyHolder>,
) -> bool {
    if view.key_epoch == 0 {
        return true;
    }
    let Some(keys) = keys else {
        return false;
    };
    view.devices.iter().any(|device| {
        device.device_id == device_id
            && device.removed_revision.is_none()
            && device.encryption_key == Some(keys.encryption_key())
    })
}

/// Why a desktop may not take the anchor: it could open no key epoch past the one it takes over,
/// so it could seal neither its own device key nor any rotation and every removal would wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeylessAnchor {
    /// Without the kit, the log names no encryption key for this desktop.
    Unpublished,
    /// With a kit, the log names neither this desktop's encryption key nor the kit's: a
    /// regeneration replaced the recovery key the printed kit derives.
    StaleKit,
}

impl std::fmt::Display for KeylessAnchor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unpublished => {
                "this desktop's encryption key is not published yet; sync once with the anchor, or use the recovery kit"
            }
            Self::StaleKit => {
                "this recovery kit was replaced by a newer recovery key and cannot open the vault's current keys; use the newer recovery kit code, or sync this desktop with the anchor first"
            }
        })
    }
}

impl std::error::Error for KeylessAnchor {}

/// Whether a forced handover may continue from `point`: this desktop opens its key epoch, see
/// [`opens_current_epoch`], or the kit's `recovery` encryption key is the one the point's latest
/// recovery key entry published, so the kit opens the point's recovery envelope.
fn forced_opens_epoch(
    point: &HandoverPoint,
    device_id: &str,
    keys: Option<&dyn SpaceKeyHolder>,
    recovery: Option<&EncryptionKeyPair>,
) -> Result<(), KeylessAnchor> {
    if opens_current_epoch(&point.view, device_id, keys) {
        return Ok(());
    }
    match recovery {
        Some(kit) if point.view.recovery_encryption_key == Some(kit.public_key()) => Ok(()),
        Some(_) => Err(KeylessAnchor::StaleKit),
        None => Err(KeylessAnchor::Unpublished),
    }
}

/// Forces the anchor to this desktop after the anchor is lost or stolen: signs with the
/// request's signer and seals a handover without an old anchor vector, so operations only the
/// lost anchor stored can become provisional again. A kit-lineage recovery key continues from
/// the chosen kit point, or by default from the live entry of highest rank where this desktop
/// is still live, which may lie below a stolen anchor's removal of it, and then removes the old
/// anchor and every device marked in the device review; any other signer continues from the
/// selected head. With a promotion, the new anchor then sequences the promotion of its own
/// device to admin with that key, signed by the same signer. With the kit's recovery key, the
/// new anchor publishes its own device key right after the handover, which seals any rotation
/// the handover point waits for, and signs every later entry with that key at hand. Before
/// anything is sealed, a desktop that would open no key epoch at the point it continues from is
/// refused with [`KeylessAnchor`]: the point needs no key epoch, names this desktop's encryption
/// key, or, with the kit, names the kit's recovery encryption key, which a regeneration
/// replaces. Returns the revision of the last entry, or why the engine rejected one.
pub async fn force_handover_here(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    forced: &ForcedHandover<'_>,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    let Some(recovery) = forced.recovery else {
        return force_handover_with(engine, conn, ctx, forced, writer, now_ms).await;
    };
    let base = writer
        .keys
        .ok_or_else(|| "this desktop holds no sync encryption key".to_string())?;
    let kit = KitKeys::new(base, recovery);
    let mut with_kit = LocalWriter {
        key: writer.key,
        certificate: writer.certificate,
        reservation: &mut *writer.reservation,
        keys: Some(&kit),
    };
    force_handover_with(engine, conn, ctx, forced, &mut with_kit, now_ms).await
}

async fn force_handover_with(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    forced: &ForcedHandover<'_>,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<u64, ProposalRejection>, String> {
    let signer = forced.signer;
    let device_id = writer.certificate.certificate.device_id.clone();
    let point = match &forced.point {
        Some(entry) => engine
            .kit_handover_point(conn, ctx, &signer.public_key(), &device_id, entry)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .ok_or_else(|| "the chosen entry is not a recovery kit point".to_string())?,
        None => engine
            .handover_point(conn, ctx, &signer.public_key(), &device_id)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .ok_or_else(|| "the sync space has no charter".to_string())?,
    };
    if point.revision == 0 {
        return Err(
            "the log holds only its charter, so no epoch can continue it; link the devices again"
                .to_string(),
        );
    }
    forced_opens_epoch(&point, &device_id, writer.keys, forced.recovery)
        .map_err(|refusal| refusal.to_string())?;
    let charter = engine
        .charter(conn, ctx)
        .await
        .map_err(|error| format!("read sync charter: {error}"))?
        .ok_or_else(|| "the sync space has no charter".to_string())?
        .hash();
    let epoch = point
        .epoch
        .checked_add(1)
        .ok_or_else(|| "the sync epoch is exhausted".to_string())?;
    let handover = propose(
        ctx.space,
        charter,
        signer,
        Entry::Handover(Handover {
            new_anchor_device_id: device_id.clone(),
            epoch,
            continues_from_revision: point.revision,
            continues_from_hash: point.entry_hash,
        }),
        now_ms,
    )?;
    let mut revision = match engine
        .seal_handover(conn, ctx, &handover, None, writer, now_ms)
        .await
        .map_err(|error| format!("seal sync handover: {error}"))?
    {
        Ok(revision) => revision,
        Err(rejection) => return Ok(Err(rejection)),
    };
    if forced.recovery.is_some() {
        match sequence_own_device_key(engine, conn, ctx, writer, now_ms).await? {
            Ok(Some(sealed)) => revision = revision.max(sealed),
            Ok(None) => {}
            Err(rejection) => return Ok(Err(rejection)),
        }
    }
    let mut follow_up = Vec::new();
    if point.by_kit {
        let mut removed = BTreeSet::from([device_id.clone()]);
        let reviewed = forced.removals.iter().map(String::as_str);
        for removal in std::iter::once(point.anchor_device_id.as_str()).chain(reviewed) {
            if removed.insert(removal.to_owned()) {
                follow_up.push(remove_entry(removal));
            }
        }
    }
    if let Some(admin_key) = forced.promote {
        follow_up.push(promote_entry(&device_id, admin_key));
    }
    for entry in follow_up {
        let proposal = propose(ctx.space, charter, signer, entry, now_ms)?;
        match sequence(engine, conn, ctx, writer, &proposal, now_ms).await? {
            Ok(sealed) => revision = revision.max(sealed),
            Err(rejection) => return Ok(Err(rejection)),
        }
    }
    Ok(Ok(revision))
}

/// The proposal that publishes the device encryption key of `keys` for the device `device_id`,
/// signed by that device's key.
pub fn device_key_proposal(
    space: SpaceId,
    charter: Digest32,
    device_id: &str,
    keys: &dyn SpaceKeyHolder,
    now_ms: u64,
) -> Result<SignedProposal, String> {
    let device = keys.device_key();
    sign_device_proposal(
        &Proposal {
            space,
            charter,
            author: AdminPublicKey::from_bytes(*device.public_key().as_bytes()),
            created_at_ms: millis(now_ms)?,
            entry: Entry::DeviceKey(DeviceKey {
                device_id: device_id.to_string(),
                encryption_key: keys.encryption_key(),
            }),
        },
        device,
    )
    .map_err(|error| format!("sign sync device key proposal: {error}"))
}

/// The entry that names the recovery encryption key, which every later key epoch seals an
/// envelope to. The first one the anchor sequences opens key epoch 1.
pub fn recovery_key_entry(encryption_key: EncryptionPublicKey) -> Entry {
    Entry::RecoveryKey(RecoveryKey { encryption_key })
}

/// What publishing the device encryption key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKeyPublication {
    /// The selected log already names the key, or the device is not live in it.
    Current,
    /// The proposal waits in the outbox for the anchor.
    Queued,
    /// The anchor sequenced it at this revision.
    Sequenced(u64),
    /// The anchor refused it.
    Rejected(ProposalRejection),
}

/// Publishes the device encryption key of `keys` when the selected log does not name it for
/// the local writer's live device: the anchor sequences it, and any other device places the
/// proposal in its outbox once for the anchor to collect. On the anchor, a rotation the log
/// waits for is sealed right after the key, which the anchor needs to seal it.
pub async fn publish_device_key(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    keys: &dyn SpaceKeyHolder,
    outbox: &mut Outbox,
    now_ms: u64,
) -> Result<DeviceKeyPublication, String> {
    let Some(view) = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
    else {
        return Ok(DeviceKeyPublication::Current);
    };
    let device_id = writer.certificate.certificate.device_id.clone();
    let encryption_key = keys.encryption_key();
    let live = view.devices.iter().find(|device| {
        device.device_id == device_id
            && device.removed_revision.is_none()
            && device.device_key == keys.device_key().public_key()
    });
    let Some(device) = live else {
        return Ok(DeviceKeyPublication::Current);
    };
    if device.encryption_key == Some(encryption_key) {
        return Ok(DeviceKeyPublication::Current);
    }
    let proposal = device_key_proposal(ctx.space, view.charter, &device_id, keys, now_ms)?;
    if view.anchor_device_id != device_id {
        let queued = outbox
            .find(|entry| {
                matches!(entry, Entry::DeviceKey(queued)
                    if queued.device_id == device_id && queued.encryption_key == encryption_key)
            })
            .is_some();
        if !queued && outbox.has_room() {
            outbox.add(proposal)?;
        }
        return Ok(DeviceKeyPublication::Queued);
    }
    Ok(
        match sequence(engine, conn, ctx, writer, &proposal, now_ms).await? {
            Ok(revision) => DeviceKeyPublication::Sequenced(revision),
            Err(rejection) => DeviceKeyPublication::Rejected(rejection),
        },
    )
}

/// On the anchor, sequences the device key of the local writer's keys unless the selected log
/// already names it. Returns the revision, `None` when nothing was sequenced, or why the engine
/// refused it.
async fn sequence_own_device_key(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<Option<u64>, ProposalRejection>, String> {
    let keys = writer
        .keys
        .ok_or_else(|| "this desktop holds no sync encryption key".to_string())?;
    let view = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        .ok_or_else(|| "the sync space has no charter".to_string())?;
    let device_id = writer.certificate.certificate.device_id.clone();
    let published = view.devices.iter().any(|device| {
        device.device_id == device_id && device.encryption_key == Some(keys.encryption_key())
    });
    if published {
        return Ok(Ok(None));
    }
    let proposal = device_key_proposal(ctx.space, view.charter, &device_id, keys, now_ms)?;
    Ok(sequence(engine, conn, ctx, writer, &proposal, now_ms)
        .await?
        .map(Some))
}

/// On the anchor of a space without a key epoch, sequences the first recovery key entry,
/// which names the kit's recovery encryption key and opens key epoch 1. `kit` is the kit the
/// vault database keeps; it starts the epochs only when its admin key is the one the selected
/// log names, so a kit the log does not name never receives an envelope. The anchor's own
/// device key must be sequenced first, because the anchor seals the key epoch to it. Returns
/// the revision of the key epoch, `None` when nothing was sequenced, or why the engine refused
/// it.
pub async fn start_key_epochs(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    writer: &mut LocalWriter<'_>,
    kit: &RecoveryPublicKeys,
    now_ms: u64,
) -> Result<Result<Option<u64>, ProposalRejection>, String> {
    let Some(view) = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
    else {
        return Ok(Ok(None));
    };
    let device_id = &writer.certificate.certificate.device_id;
    if !starts_key_epochs(&view, device_id, kit) {
        return Ok(Ok(None));
    }
    let proposal = propose(
        ctx.space,
        view.charter,
        admin,
        recovery_key_entry(kit.encryption),
        now_ms,
    )?;
    Ok(sequence(engine, conn, ctx, writer, &proposal, now_ms)
        .await?
        .map(Some))
}

/// Whether the anchor `device_id` starts the key epochs of the space `view` shows with `kit`:
/// it anchors, no recovery key entry was sequenced, and the log names the kit's admin key.
pub fn starts_key_epochs(
    view: &AuthorizationView,
    device_id: &str,
    kit: &RecoveryPublicKeys,
) -> bool {
    view.anchor_device_id == device_id
        && view.recovery_encryption_key.is_none()
        && view.recovery_admin_key == kit.admin
}

/// On the anchor, seals the key epoch the selected log waits for, as after a handover that
/// continued past a removal. Returns its revision, `None` when nothing waits, or why the anchor
/// cannot seal it yet, such as before it opens the current epoch.
pub async fn complete_rotation(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<Option<u64>, ProposalRejection>, String> {
    engine
        .complete_rotation(conn, ctx, writer, now_ms)
        .await
        .map_err(|error| format!("seal sync key epoch: {error}"))
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

/// Signs with `admin` and seals a yield to the stored handover `handover`, as a contestant of
/// its frozen instance or as the anchor of that instance with an objection to it. The yield
/// enters the record store at once and travels with the writer's other operations. Returns the
/// revision, or why the engine did not seal it.
pub async fn yield_here(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    handover: &Digest32,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<u64, ContestRejection>, String> {
    engine
        .seal_yield(conn, ctx, admin, handover, writer, now_ms)
        .await
        .map_err(|error| format!("seal sync yield: {error}"))
}

/// Signs with `admin` and seals, as the anchor of the instance the stored handover `handover`
/// leaves, a removal of its target with a fresh discovery key, which objects to it. Returns the
/// revision, or why the engine did not seal it.
pub async fn object_here(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    admin: &AdminKeyPair,
    handover: &Digest32,
    writer: &mut LocalWriter<'_>,
    now_ms: u64,
) -> Result<Result<u64, ContestRejection>, String> {
    let discovery_key = *DiscoveryKey::generate()?.as_bytes();
    engine
        .seal_objection(conn, ctx, admin, handover, discovery_key, writer, now_ms)
        .await
        .map_err(|error| format!("seal sync objection: {error}"))
}
