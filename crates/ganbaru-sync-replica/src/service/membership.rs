//! Space membership of a session: founding the space and pinning its charter, quarantine and
//! removal of the coordinator, discovery keys, device enrollment and removal on the
//! coordinator, proposal sequencing on the anchor, and which device the selected log names as
//! the anchor.

use super::host::SyncHost;
#[cfg(desktop)]
use super::{ProposalRequest, linked_enrollment, tls_fingerprint};
use super::{Session, acquire, blocking, discovery_keys, peers};
#[cfg(desktop)]
use crate::authority::{self, Founding};
use crate::now_ms;
#[cfg(desktop)]
use crate::outbox::Outbox;
use crate::peers::discovery_keys::DiscoveryKeys;
use crate::quarantine::Quarantine;
#[cfg(desktop)]
use crate::reset;
use crate::writer::ActiveWriter;
use ganbaru_handoff::pairing::{PairingManager, PinnedCharter};
#[cfg(desktop)]
use ganbaru_sync_contracts::VersionVector;
#[cfg(desktop)]
use ganbaru_sync_contracts::{ProposalRejection, SignedCharter};
#[cfg(desktop)]
use std::collections::BTreeSet;

impl<H: SyncHost> Session<'_, H> {
    /// On a desktop coordinator, founds the vault's sync space once the vault first links: it
    /// creates the anchor keys and installs a charter naming this installation as the anchor,
    /// signed by the person key. A space that has a charter is left as it is. Phones never found
    /// a space. Returns the charter with its person key for the pairing reply.
    pub(super) async fn found_space(
        &self,
        manager: &PairingManager,
    ) -> Result<Option<PinnedCharter>, String> {
        #[cfg(desktop)]
        {
            let device_id = manager.device_id()?;
            let installed = {
                let mut conn = acquire(&self.pool).await?;
                self.init_space(&mut conn).await?;
                self.engine
                    .charter(&mut conn, &self.ctx)
                    .await
                    .map_err(|error| format!("read sync charter: {error}"))?
            };
            if let Some(charter) = installed {
                return Ok(Some(self.pinned_charter(&charter)));
            }
            let record_path = self.files.record_path();
            let record = blocking(move || crate::writer::read_record(&record_path)).await?;
            reset::refuse_refounding(record.as_ref())?;
            let Some(person) = self.host.person_key(&self.ctx.person_key).await? else {
                return Err(
                    "the person key that founds the sync space is missing on this device"
                        .to_string(),
                );
            };
            // Writers without a charter come from a build before the authorization log, and
            // only Reset sync state may found a space over them.
            {
                let mut conn = acquire(&self.pool).await?;
                self.engine
                    .refuse_legacy_state(&mut conn, &self.ctx)
                    .await
                    .map_err(|error| format!("open sync space: {error}"))?;
            }
            let tls_fingerprint = tls_fingerprint(manager.certificate()?.as_ref());
            let discovery_key = ganbaru_handoff::discovery::DiscoveryKey::generate()?;
            let founding = Founding {
                device_id: &device_id,
                tls_fingerprint,
                discovery_key: *discovery_key.as_bytes(),
                person: &person,
                now_ms: now_ms()?,
            };
            let charter = authority::found_with_recovery_kit(
                &self.engine,
                &self.pool,
                &self.ctx,
                self.keys.clone(),
                &self.files.vault_id,
                &founding,
            )
            .await?;
            Ok(Some(self.pinned_charter(&charter)))
        }
        #[cfg(mobile)]
        {
            let _ = manager;
            Ok(None)
        }
    }

    /// The pairing form of `charter`. The coordinator presents it whether or not it anchors the
    /// space: a linked device learns the current anchor from the log the charter starts.
    #[cfg(desktop)]
    fn pinned_charter(&self, charter: &SignedCharter) -> PinnedCharter {
        PinnedCharter {
            charter: ganbaru_handoff::protocol::encode_sync_charter(charter),
            person_key: self.ctx.person_key.to_text(),
        }
    }

    /// Whether this device quarantined the coordinator it pinned, which it then never dials.
    pub(super) async fn coordinator_quarantined(
        &self,
        manager: &PairingManager,
    ) -> Result<bool, String> {
        let Some(pin) = manager.coordinator_pin()? else {
            return Ok(false);
        };
        let path = self.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        Ok(quarantine.contains(&pin.device_id))
    }

    /// The devices this installation quarantined, whose device keys the anchor refuses to
    /// sequence.
    #[cfg(desktop)]
    pub(super) async fn quarantined(&self) -> Result<BTreeSet<String>, String> {
        let path = self.files.quarantine_path();
        Ok(blocking(move || Quarantine::load(&path))
            .await?
            .devices()
            .clone())
    }

    /// Whether the selected log, derived from the stored record envelopes alone, removed the
    /// coordinator this device pinned. Such a coordinator gets record exchange only, so it
    /// learns its removal but no longer takes this device's operations or assets.
    pub(super) async fn coordinator_removed(
        &self,
        manager: &PairingManager,
    ) -> Result<bool, String> {
        let Some(pin) = manager.coordinator_pin()? else {
            return Ok(false);
        };
        let mut conn = acquire(&self.pool).await?;
        let view = self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?;
        Ok(view.is_some_and(|view| crate::hub::removes_device(&view, &pin.device_id)))
    }

    /// On a client, installs the charter pinned at pairing when the space has none yet, and
    /// refuses a space whose charter differs from it, because the pin names the log this device
    /// agreed to follow.
    pub(super) async fn install_pinned_charter(
        &self,
        manager: &PairingManager,
    ) -> Result<(), String> {
        let Some(pinned) = manager.coordinator_pin()?.and_then(|pin| pin.charter) else {
            return Ok(());
        };
        let (charter, _) =
            ganbaru_handoff::protocol::decode_sync_charter(&pinned.charter, &pinned.person_key)?;
        let mut conn = acquire(&self.pool).await?;
        self.init_space(&mut conn).await?;
        match self
            .engine
            .charter(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync charter: {error}"))?
        {
            Some(installed) if installed == charter => Ok(()),
            Some(_) => Err(
                "the sync space has a different charter than the one pinned at pairing".to_string(),
            ),
            None => self
                .engine
                .install_charter(&mut conn, &self.ctx, &charter)
                .await
                .map_err(|error| format!("install the pinned sync charter: {error}")),
        }
    }

    /// Follows the vault's discovery keys from the authorization log into the device-local file
    /// and publishes them for the advertiser and coordinator discovery of this process.
    pub(super) async fn follow_discovery_keys(&self) -> Result<(), String> {
        let view = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
        };
        let path = self.files.discovery_keys_path();
        let vault_id = self.files.vault_id.clone();
        let now = now_ms()?;
        let keys =
            blocking(move || DiscoveryKeys::follow(&path, &vault_id, view.as_ref(), now)).await?;
        discovery_keys::publish(keys);
        Ok(())
    }

    /// On the coordinator, unlinks every linked device the selected log removed, whichever admin
    /// proposed the removal and whether or not this desktop anchors, so pairing stops serving
    /// it the person key, vault bundles, and every other coordinator service. A device with a
    /// handoff in progress is reported and unlinked on a later pass; the listener refuses it
    /// from the log meanwhile.
    #[cfg(desktop)]
    pub(super) async fn unlink_removed_devices(
        &self,
        manager: &PairingManager,
        vault_id: &str,
    ) -> Result<(), String> {
        let mut conn = acquire(&self.pool).await?;
        let Some(view) = self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
        else {
            return Ok(());
        };
        drop(conn);
        let peers = manager.linked_peers_for(vault_id)?;
        let removed = authority::removed_linked_devices(&view, |device_id| {
            peers.iter().any(|peer| peer.device_id == device_id)
        });
        for device_id in removed {
            if let Err(error) = manager.unlink_device(&device_id) {
                eprintln!("sync: unlink removed device {device_id}: {error}");
            }
        }
        Ok(())
    }

    /// On an admin coordinator, enrolls linked devices the space does not know yet and removes
    /// enrolled devices that are no longer linked, so a crash between linking and sealing heals
    /// on the next pass. Each change goes through the outbox as a proposal signed by this
    /// desktop's admin key; the coordinator sequences it while it anchors and holds it for the
    /// anchor's forward request after a handover. A device the space removed stays
    /// removed: linking it again under the same device id enrolls nothing. A device that cannot
    /// be enrolled is reported and skipped, so it never stops the pass.
    pub(super) async fn reconcile_devices(
        &self,
        manager: &PairingManager,
        vault_id: &str,
        writer: &mut Option<ActiveWriter>,
    ) -> Result<(), String> {
        #[cfg(desktop)]
        {
            let mut conn = acquire(&self.pool).await?;
            let Some(view) = self
                .engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
            else {
                return Ok(());
            };
            let Some(active) = writer.as_mut() else {
                return Ok(());
            };
            let own_device_id = manager.device_id()?;
            let admin = self.admin_key().await?;
            if !authority::is_admin(&view, &own_device_id, &admin.public_key()) {
                return Ok(());
            }
            let anchors = view.anchor_device_id == own_device_id;
            let peers = manager.linked_peers_for(vault_id)?;
            let enroll: Vec<_> = peers
                .iter()
                .filter(|peer| {
                    !view
                        .devices
                        .iter()
                        .any(|device| device.device_id == peer.device_id)
                })
                .collect();
            // Only devices this coordinator unlinked are removed. Devices that joined through a
            // server never link here, so an absent link is not a removal.
            let revoked = manager.revoked_device_ids(vault_id)?;
            let remove = authority::unlinked_devices(&view, &own_device_id, |device_id| {
                revoked.contains(device_id)
            });
            let mut changes = authority::Reconciliation {
                remove,
                ..Default::default()
            };
            for peer in enroll {
                match linked_enrollment(peer) {
                    Ok(enrollment) => changes.enroll.push(enrollment),
                    Err(error) => {
                        eprintln!("sync: enroll linked device {}: {error}", peer.device_id);
                    }
                }
            }
            let path = self.files.outbox_path();
            let mut outbox = blocking(move || Outbox::load(&path)).await?;
            if !changes.enroll.is_empty() || !changes.remove.is_empty() {
                authority::queue_reconciliation(
                    &mut outbox,
                    self.ctx.space,
                    view.charter,
                    &admin,
                    changes,
                    now_ms()?,
                )?;
            }
            // A coordinator that handed the anchor over keeps the proposals for the anchor's
            // forward request.
            if !anchors || outbox.entries().iter().all(|entry| !entry.pending()) {
                return Ok(());
            }
            let quarantined = self.quarantined().await?;
            authority::sequence_outbox(
                &self.engine,
                &mut conn,
                &self.ctx,
                &mut active.local(),
                &mut outbox,
                &VersionVector::new(),
                &quarantined,
                now_ms()?,
            )
            .await?;
            drop(conn);
            // The record must name what sequencing sealed, or a restored database behind those
            // entries would still match the writer.
            self.record_commit(active).await?;
            Ok(())
        }
        #[cfg(mobile)]
        {
            let _ = (manager, vault_id, writer);
            Ok(())
        }
    }

    /// On the anchor, sequences the proposals linked devices sent through the hub with the
    /// anchor writer and answers each. A coordinator that handed the anchor over holds each in
    /// its outbox for the anchor's forward request. Without a writer, each is answered as not
    /// reaching the anchor; the sender keeps it in its outbox either way. Removal cutoffs cover
    /// at least `covered`, the coordinator's last reported stored vector on a linked anchor.
    #[cfg(desktop)]
    pub(super) async fn sequence_proposals(
        &self,
        manager: &PairingManager,
        writer: &mut Option<ActiveWriter>,
        requests: Vec<ProposalRequest>,
        covered: &VersionVector,
    ) {
        if requests.is_empty() {
            return;
        }
        let anchor = self.anchors_here(manager).await;
        let quarantined = match anchor {
            Ok(true) => self.quarantined().await,
            _ => Ok(BTreeSet::new()),
        };
        let mut outbox = None;
        if matches!(anchor, Ok(false)) {
            let path = self.files.outbox_path();
            outbox = Some(blocking(move || Outbox::load(&path)).await);
        }
        for request in requests {
            let outcome = match (&anchor, writer.as_mut()) {
                (Err(error), _) => Err(error.clone()),
                (Ok(false), _) => match (outbox.as_mut(), acquire(&self.pool).await) {
                    (Some(Ok(outbox)), Ok(mut conn)) => {
                        authority::hold_for_anchor(
                            &self.engine,
                            &mut conn,
                            &self.ctx,
                            outbox,
                            request.proposal.clone(),
                        )
                        .await
                    }
                    (Some(Err(error)), _) => Err(error.clone()),
                    (_, Err(error)) => Err(error),
                    (None, _) => Ok(Err(ProposalRejection::NotAnchor)),
                },
                (Ok(true), Some(active)) => match acquire(&self.pool).await {
                    Ok(mut conn) => match now_ms() {
                        Ok(now) => match &quarantined {
                            Ok(quarantined) => {
                                authority::sequence_covering(
                                    &self.engine,
                                    &mut conn,
                                    &self.ctx,
                                    &mut active.local(),
                                    &request.proposal,
                                    covered,
                                    quarantined,
                                    now,
                                )
                                .await
                            }
                            Err(error) => Err(error.clone()),
                        },
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                },
                _ => Ok(Err(ProposalRejection::NotAnchor)),
            };
            let _ = request.reply.send(outcome);
        }
        if let (Ok(true), Some(active)) = (&anchor, writer.as_mut())
            && let Err(error) = self.record_commit(active).await
        {
            eprintln!("sync: record the sequenced entries: {error}");
        }
    }

    /// Whether the selected log names this installation's device as the anchor.
    #[cfg(desktop)]
    pub(super) async fn anchors_here(&self, manager: &PairingManager) -> Result<bool, String> {
        let device_id = manager.device_id()?;
        let mut conn = acquire(&self.pool).await?;
        Ok(self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .is_some_and(|view| view.anchor_device_id == device_id))
    }

    /// The device the selected log names as the anchor, if the space has a log.
    pub(super) async fn anchor_device_id(&self) -> Result<Option<String>, String> {
        let mut conn = acquire(&self.pool).await?;
        Ok(self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .map(|view| view.anchor_device_id))
    }

    /// Whether the selected log names a server as the anchor.
    pub(super) async fn anchored_by_server(&self) -> Result<bool, String> {
        let mut conn = acquire(&self.pool).await?;
        Ok(self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .as_ref()
            .is_some_and(peers::anchored_by_server))
    }
}
