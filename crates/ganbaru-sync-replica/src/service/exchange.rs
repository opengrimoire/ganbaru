//! The LAN exchange step of a sync pass: operations and records with the coordinator hub,
//! fork recovery around it, the passes while the coordinator is away or removed, exchanges
//! with the other members, and the managed asset uploads and downloads around them.

use super::host::SyncHost;
use super::status::{SyncErrorCode, SyncState, SyncStatusView};
use super::transport::PairedTransport;
use super::{
    ApplyMark, AssetPeer, MAX_FORK_RECOVERIES, Next, OnlineWait, ServiceState, Session, acquire,
    blocking, peers,
};
use crate::assets::{AssetSession, AssetTransfer, MAX_ASSET_PASS_BYTES};
#[cfg(desktop)]
use crate::authority;
use crate::client::{ClientSession, Exchange, PeerIds};
use crate::now_ms;
use crate::quarantine::Quarantine;
use crate::records::RecordLink;
use crate::writer::ActiveWriter;
use ganbaru_handoff::pairing::PairingManager;
use ganbaru_handoff::protocol::SyncRefusalCode;
use ganbaru_sync_contracts::VersionVector;
use std::collections::BTreeSet;

fn asset_session<'s, H: SyncHost>(
    session: &'s Session<'_, H>,
    peer: &'s AssetPeer,
) -> AssetSession<'s, PairedTransport> {
    AssetSession {
        transport: &peer.transport,
        ids: &peer.ids,
        engine: &session.engine,
        pool: &session.pool,
        store: &peer.assets,
        max_bytes: MAX_ASSET_PASS_BYTES,
    }
}

impl ServiceState {
    /// Exchanges with the hub, re-sealing after a fork of this installation's chain.
    pub(super) async fn exchange<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        mut wait: OnlineWait,
        view: &mut SyncStatusView,
    ) -> Result<Next, String> {
        let path = session.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        let listening = session.host.serve_members().await;
        self.anchor_link = None;
        // The coordinator's device id names it for record exchange windows and cursors.
        let coordinator = wait
            .transport
            .manager
            .coordinator_pin()?
            .map(|pin| pin.device_id);
        for _ in 0..MAX_FORK_RECOVERIES {
            let own_writers = self
                .writer
                .as_ref()
                .map(ActiveWriter::own_writers)
                .unwrap_or_default();
            let client = ClientSession {
                transport: &wait.transport,
                ids: &wait.ids,
                engine: &session.engine,
                pool: &session.pool,
                ctx: &session.ctx,
                writer: self.writer.as_ref().map(ActiveWriter::id),
                own_writers: &own_writers,
                quarantined: quarantine.devices(),
                pull_skips: Some(&self.pull_skips),
                max_bytes: crate::client::MAX_EXCHANGE_BYTES,
                listening: &listening,
                records: coordinator.as_deref().map(|peer| RecordLink {
                    state: &self.records,
                    peer,
                }),
            };
            match client.exchange().await {
                Err(error) => {
                    return Ok(self
                        .coordinator_away(
                            session,
                            wait,
                            quarantine.devices(),
                            &listening,
                            view,
                            error,
                        )
                        .await);
                }
                Ok(Exchange::Done(done)) => {
                    view.last_exchange_at_ms = Some(now_ms()?);
                    if let Err(error) = peers::record_reported_peers(
                        session.files.peers_path(),
                        &session.engine,
                        &session.pool,
                        &session.ctx,
                        quarantine.devices(),
                        done.members.clone(),
                    )
                    .await
                    {
                        eprintln!("sync: record reported members: {error}");
                    }
                    if done.pulled > 0 {
                        self.assets.hub_changed();
                    }
                    {
                        let mut conn = acquire(&session.pool).await?;
                        session
                            .engine
                            .record_stable(&mut conn, &session.ctx, &done.stable)
                            .await
                            .map_err(|error| format!("record sync stability: {error}"))?;
                        if let Some(pin) = wait.transport.manager.coordinator_pin()? {
                            crate::client::record_anchor_marks(
                                &session.engine,
                                &mut conn,
                                &session.ctx,
                                &wait.ids.device_id,
                                &pin.device_id,
                                &done.hub,
                            )
                            .await
                            .map_err(|error| format!("record anchor marks: {error}"))?;
                        }
                    }
                    if done.confirmed.is_some() {
                        self.confirmed = done.confirmed;
                    }
                    if done.bootstrap_required {
                        // Waiting would return at once, since the hub holds what this device
                        // cannot pull; local commits and the fallback interval still push.
                        view.state = SyncState::Error;
                        view.last_error = done.problem.or_else(|| {
                            Some("the hub no longer holds operations this device lacks".into())
                        });
                        view.last_error_code = Some(SyncErrorCode::BootstrapRequired);
                        return Ok(Next::Idle);
                    }
                    match done.problem {
                        Some(problem) => {
                            view.state = SyncState::Error;
                            view.last_error = Some(problem);
                            view.last_error_code = (done.refusal
                                == Some(SyncRefusalCode::WriterLimit))
                            .then_some(SyncErrorCode::WriterLimit);
                        }
                        None if done.more => return Ok(Next::Continue),
                        None => {}
                    }
                    // Operations of devices this client quarantined count as known, so the long
                    // poll does not return for what it turns away; the coordinator's stored
                    // vector without those writers is what taking the anchor merges.
                    let skipped = self.pull_skips.for_quarantine(quarantine.devices());
                    #[cfg(desktop)]
                    {
                        wait.hub = authority::without_writers(&done.hub, &skipped);
                        self.hub_reported.clone_from(&done.hub);
                    }
                    let members = self
                        .exchange_members(session, &wait, quarantine.devices(), &listening, true)
                        .await;
                    self.anchor_link = members.anchor;
                    if members.pulled > 0 {
                        // Apply what members brought, then exchange it with the coordinator.
                        return Ok(Next::Continue);
                    }
                    wait.known = done.known;
                    wait.known.merge(&skipped);
                    return Ok(Next::Online(Box::new(wait)));
                }
                Ok(Exchange::UnprovenFork(fork)) => {
                    // A claim without signed proof retires nothing: the writer stays, the
                    // coordinator is treated as away, and members still exchange.
                    let error = format!(
                        "the coordinator claims writer {} forked and shows no signed operation \
                         for it",
                        fork.writer.to_hex()
                    );
                    let next = self
                        .coordinator_away(
                            session,
                            wait,
                            quarantine.devices(),
                            &listening,
                            view,
                            error,
                        )
                        .await;
                    view.state = SyncState::Error;
                    view.last_error_code = Some(SyncErrorCode::ForkClaimUnproven);
                    return Ok(next);
                }
                Ok(Exchange::Fork(fork)) => {
                    if !session.recover_fork(&mut self.writer, fork).await? {
                        view.state = SyncState::WaitingForIdentity;
                        return Ok(Next::Idle);
                    }
                    self.applied = ApplyMark::default();
                }
            }
        }
        Err("sync fork recovery did not settle".to_string())
    }

    /// The pass while the coordinator does not answer: members still exchange with each other,
    /// and an anchor that is not the coordinator and answered a complete exchange carries the
    /// long poll, so its changes arrive without waiting for the coordinator. The status stays
    /// offline with `error`.
    pub(super) async fn coordinator_away<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        mut wait: OnlineWait,
        quarantined: &BTreeSet<String>,
        listening: &[String],
        view: &mut SyncStatusView,
        error: String,
    ) -> Next {
        let members = self
            .exchange_members(session, &wait, quarantined, listening, false)
            .await;
        view.state = SyncState::Offline;
        view.last_error = Some(error);
        self.anchor_link = members.anchor;
        let (Some(anchor), Some(known)) = (self.anchor_link.as_ref(), members.anchor_known) else {
            return Next::Retry;
        };
        wait.known = known;
        wait.known
            .merge(&self.pull_skips.for_quarantine(quarantined));
        wait.poll = Some(anchor.clone());
        Next::Online(Box::new(wait))
    }

    /// The pass while the selected log removed the coordinator: record exchange only, so it
    /// learns its removal, and then members exchange as while it is away. Nothing else reaches
    /// it or comes from it: no operations or assets, no stable vector, and no anchor marks.
    pub(super) async fn removed_coordinator_pass<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        manager: &PairingManager,
        ids: PeerIds,
        view: &mut SyncStatusView,
    ) -> Result<Next, String> {
        let path = session.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        let listening = session.host.serve_members().await;
        self.anchor_link = None;
        let wait = OnlineWait {
            transport: PairedTransport {
                manager: manager.clone(),
                dialer: session.host.dialer(),
            },
            ids,
            known: VersionVector::new(),
            #[cfg(desktop)]
            hub: VersionVector::new(),
            poll: None,
        };
        let coordinator = manager.coordinator_pin()?.map(|pin| pin.device_id);
        let own_writers = self
            .writer
            .as_ref()
            .map(ActiveWriter::own_writers)
            .unwrap_or_default();
        let records = ClientSession {
            transport: &wait.transport,
            ids: &wait.ids,
            engine: &session.engine,
            pool: &session.pool,
            ctx: &session.ctx,
            writer: self.writer.as_ref().map(ActiveWriter::id),
            own_writers: &own_writers,
            quarantined: quarantine.devices(),
            pull_skips: Some(&self.pull_skips),
            max_bytes: crate::client::MAX_EXCHANGE_BYTES,
            listening: &listening,
            records: coordinator.as_deref().map(|peer| RecordLink {
                state: &self.records,
                peer,
            }),
        }
        .exchange_records_only()
        .await;
        if let Err(error) = records {
            eprintln!("sync: record exchange with the removed coordinator: {error}");
        }
        let next = self
            .coordinator_away(
                session,
                wait,
                quarantine.devices(),
                &listening,
                view,
                "the selected log removed the coordinator".to_string(),
            )
            .await;
        view.last_error_code = Some(SyncErrorCode::CoordinatorRemoved);
        Ok(next)
    }

    /// Exchanges with the other members the pass plan picks. A failure only delays what members
    /// would bring until a later pass, so it is logged.
    async fn exchange_members<H: SyncHost>(
        &self,
        session: &Session<'_, H>,
        wait: &OnlineWait,
        quarantined: &BTreeSet<String>,
        listening: &[String],
        coordinator_reachable: bool,
    ) -> peers::MemberPassOutcome {
        let own_writers = self
            .writer
            .as_ref()
            .map(ActiveWriter::own_writers)
            .unwrap_or_default();
        let pass = peers::MemberPass {
            host: session.host,
            manager: &wait.transport.manager,
            ids: &wait.ids,
            engine: &session.engine,
            pool: &session.pool,
            ctx: &session.ctx,
            writer: self.writer.as_ref().map(ActiveWriter::id),
            own_writers: &own_writers,
            quarantined,
            pull_skips: &self.pull_skips,
            listening,
            files: &session.files,
            records: &self.records,
        };
        match peers::exchange_with_members(pass, coordinator_reachable).await {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("sync: member exchanges: {error}");
                peers::MemberPassOutcome::default()
            }
        }
    }

    /// Uploads the managed assets this device holds and the hub lacks, ahead of the operations
    /// that reference them. Only a transport failure is an error; a file that could not be read
    /// is a problem in `transfer`.
    pub(super) async fn upload_assets<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        peer: &AssetPeer,
        transfer: &mut AssetTransfer,
    ) -> Result<(), String> {
        asset_session(session, peer)
            .upload(&mut self.assets, transfer)
            .await
    }

    /// Downloads the referenced managed assets this device lacks once operations were applied,
    /// then folds the outcome of both directions into the status and the next wait. Downloads
    /// run only after an exchange that reached the hub.
    pub(super) async fn download_assets<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        peer: &AssetPeer,
        transfer: &mut AssetTransfer,
        next: Next,
        view: &mut SyncStatusView,
    ) -> Next {
        match &next {
            Next::Online(wait) if wait.poll.is_none() => {}
            Next::Continue => {}
            _ => return next,
        }
        let downloaded = asset_session(session, peer)
            .download(&mut self.assets, transfer)
            .await;
        if let Err(error) = downloaded {
            view.state = SyncState::Offline;
            view.last_error = Some(error);
            return Next::Retry;
        }
        if let Some(problem) = transfer.problem.take() {
            view.state = SyncState::Error;
            view.last_error = Some(problem);
        }
        match next {
            Next::Online(_) if transfer.more => Next::Continue,
            next => next,
        }
    }
}
