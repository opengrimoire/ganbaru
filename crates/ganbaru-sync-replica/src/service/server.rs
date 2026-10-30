//! A device's exchange with the always-on anchor server the selected log names.
//!
//! The server dials nobody, so every device reaches it. This runs beside the coordinator and member
//! exchanges: it moves operations, records, and assets, and it is what lets a phone sync while
//! its coordinator desktop is off.

use super::status::{SyncErrorCode, SyncState, SyncStatusView};
use super::{ActiveWriter, RELAY_LOOPBACK, ServiceState, Session, SyncHost, acquire, blocking};
use crate::anchor_link::{ServerTarget, ServerTransport, protocol_gap};
use crate::assets::{AssetSession, AssetStore, AssetTransfer, MAX_ASSET_PASS_BYTES};
use crate::client::{ClientSession, Exchange, PeerIds};
use crate::quarantine::Quarantine;
use crate::records::RecordLink;
use ganbaru_handoff::pairing::PairingManager;
use ganbaru_handoff::protocol::anchor::SERVER_NEEDS_UPDATE_CODE;
use ganbaru_sync_contracts::VersionVector;
use std::sync::Arc;

/// The version this build reports when it asks a server which protocol it speaks.
const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An anchor server that answered this pass.
pub(super) struct ServerLink {
    transport: ServerTransport,
    ids: PeerIds,
    /// The device behind the server's certificate in the selected log.
    device_id: String,
    assets: Arc<dyn AssetStore>,
    transfer: AssetTransfer,
    /// What the server reported storing, which covers removal cutoffs an anchor here seals.
    #[cfg_attr(mobile, allow(dead_code))]
    stored: VersionVector,
    /// An exchange stopped at its byte bound.
    pub(super) more: bool,
    /// The selected log names the server as the anchor.
    pub(super) anchors: bool,
}

impl ServiceState {
    /// Exchanges with the anchor server the selected log names, when there is one and it is not
    /// this device. A server that does not answer costs one dial and leaves the rest of the pass
    /// alone.
    pub(super) async fn server_exchange<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        manager: &PairingManager,
        vault_id: &str,
        view: &mut SyncStatusView,
    ) -> Option<ServerLink> {
        if session.host.is_server() {
            return None;
        }
        match self
            .try_server_exchange(session, manager, vault_id, view)
            .await
        {
            Ok(link) => link,
            Err(error) => {
                self.server_version_checked = false;
                eprintln!("sync: exchange with the anchor server: {error}");
                None
            }
        }
    }

    async fn try_server_exchange<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        manager: &PairingManager,
        vault_id: &str,
        view: &mut SyncStatusView,
    ) -> Result<Option<ServerLink>, String> {
        let authorization = {
            let mut conn = acquire(&session.pool).await?;
            session
                .engine
                .authorization(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
        };
        let Some(authorization) = authorization else {
            return Ok(None);
        };
        let Some(target) = ServerTarget::of(&authorization) else {
            return Ok(None);
        };
        let Some(device_id) = target.device_id(&authorization).map(str::to_string) else {
            return Ok(None);
        };
        let path = session.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        if quarantine.contains(&device_id) {
            return Ok(None);
        }
        let transport = ServerTransport {
            manager: manager.clone(),
            target,
            loopback: RELAY_LOOPBACK,
        };
        if !self.server_version_checked {
            match protocol_gap(&transport, BUILD_VERSION).await {
                Some(SERVER_NEEDS_UPDATE_CODE) => {
                    view.state = SyncState::Error;
                    view.last_error = Some("the anchor server needs an update".to_string());
                    view.last_error_code = Some(SyncErrorCode::ServerNeedsUpdate);
                    return Ok(None);
                }
                Some(_) => return Err("this device needs an update".to_string()),
                None => self.server_version_checked = true,
            }
        }
        let ids = PeerIds {
            vault_id: vault_id.to_string(),
            device_id: manager.device_id()?,
        };
        let assets: Arc<dyn AssetStore> = session.host.asset_store(vault_id)?;
        let mut transfer = AssetTransfer::default();
        AssetSession {
            transport: &transport,
            ids: &ids,
            engine: &session.engine,
            pool: &session.pool,
            store: &assets,
            max_bytes: MAX_ASSET_PASS_BYTES,
        }
        .upload(&mut self.server_assets, &mut transfer)
        .await?;
        let own_writers = self
            .writer
            .as_ref()
            .map(ActiveWriter::own_writers)
            .unwrap_or_default();
        let client = ClientSession {
            transport: &transport,
            ids: &ids,
            engine: &session.engine,
            pool: &session.pool,
            ctx: &session.ctx,
            writer: self.writer.as_ref().map(ActiveWriter::id),
            own_writers: &own_writers,
            quarantined: quarantine.devices(),
            pull_skips: Some(&self.pull_skips),
            max_bytes: crate::client::MAX_EXCHANGE_BYTES,
            listening: &[],
            records: Some(RecordLink {
                state: &self.records,
                peer: &device_id,
            }),
        };
        let done = match client.exchange().await? {
            Exchange::Done(done) => done,
            Exchange::Fork(fork) => {
                return Err(format!(
                    "the server holds writer {} past this device at {}",
                    fork.writer.to_hex(),
                    fork.keep_through
                ));
            }
        };
        if let Some(problem) = &done.problem {
            eprintln!("sync: the anchor server refused part of an exchange: {problem}");
        }
        if done.pulled > 0 {
            self.server_assets.hub_changed();
        }
        let anchors = authorization.anchor_device_id == device_id;
        if anchors {
            super::peers::record_direct_anchor_marks(
                &session.engine,
                &session.pool,
                &session.ctx,
                &authorization,
                &done,
            )
            .await?;
        }
        Ok(Some(ServerLink {
            transport,
            ids,
            device_id,
            assets,
            transfer,
            stored: done.hub.clone(),
            more: done.more || done.pulled > 0,
            anchors,
        }))
    }

    /// Finishes a server exchange once its operations were applied: hands proposals to the
    /// server when it anchors, collects what it holds when this device anchors, and downloads the
    /// assets the applied operations reference. Returns whether another pass should run at once.
    pub(super) async fn settle_server<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        manager: &PairingManager,
        mut link: ServerLink,
    ) -> Result<bool, String> {
        #[cfg_attr(mobile, allow(unused_mut))]
        let mut more = false;
        if link.anchors {
            session
                .replay_outbox(&link.transport, &link.ids, &link.device_id)
                .await;
        } else {
            #[cfg(desktop)]
            if session.anchors_here(manager).await?
                && self.anchor_ready(session, manager, true).await?
            {
                match session
                    .anchor_as_member(
                        &link.transport,
                        &link.ids,
                        &mut self.writer,
                        &mut self.server_settled,
                        &link.stored,
                    )
                    .await
                {
                    Ok(sealed) => more |= sealed,
                    Err(error) => {
                        eprintln!("sync: collect proposals from the anchor server: {error}");
                    }
                }
            }
            #[cfg(mobile)]
            let _ = manager;
        }
        AssetSession {
            transport: &link.transport,
            ids: &link.ids,
            engine: &session.engine,
            pool: &session.pool,
            store: &link.assets,
            max_bytes: MAX_ASSET_PASS_BYTES,
        }
        .download(&mut self.server_assets, &mut link.transfer)
        .await?;
        if let Some(problem) = link.transfer.problem.take() {
            eprintln!("sync: anchor server assets: {problem}");
        }
        Ok(more || link.transfer.more)
    }
}
