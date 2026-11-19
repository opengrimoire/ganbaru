//! The relay step of a sync pass: one pass against the relay the space's server configuration
//! names, how large an asset file it moves on the current network, and how its outcome is
//! recorded in the sync status.

use super::host::SyncHost;
use super::status::{RelayDeliveryView, SyncStatusView};
use super::{RELAY_LOOPBACK, ServiceState, Session, acquire, blocking};
use crate::assets::AssetStore;
use crate::authority::read_admin_key;
use crate::now_ms;
use crate::quarantine::Quarantine;
use crate::relay::{
    METERED_MAX_ASSET_BYTES, RelayError, RelayLimits, RelayPass, RelayProblem, RelaySession,
    RelayTarget, UNMETERED_MAX_ASSET_BYTES,
};
use crate::writer::ActiveWriter;
use std::sync::Arc;

/// The largest asset file a relay pass of `host` moves: large files only on a network the host
/// does not report as metered.
fn relay_asset_bound<H: SyncHost>(host: &H) -> u32 {
    if host.relay_metered() {
        METERED_MAX_ASSET_BYTES
    } else {
        UNMETERED_MAX_ASSET_BYTES
    }
}

/// Records a relay pass outcome in `view`. A failure stays a present relay status, which keeps
/// the Android relay worker scheduled while the space names a relay. `previous` is the status
/// of the pass before this one.
fn record_relay_outcome(
    view: &mut SyncStatusView,
    outcome: Result<RelayPass, RelayError>,
    previous: Option<&RelayDeliveryView>,
) {
    let (now, clock) = match now_ms() {
        Ok(now) => (now, None),
        Err(error) => (0, Some(RelayError::from(error))),
    };
    view.relay = Some(match (outcome, clock) {
        (Ok(pass), None) => {
            if let Some(detail) = &pass.detail {
                eprintln!("sync relay: {detail}");
            }
            RelayDeliveryView::of(&pass, now, previous)
        }
        (Err(error), _) | (Ok(_), Some(error)) => {
            eprintln!("sync relay: {}", error.detail);
            RelayDeliveryView::failed(error.problem, now, previous)
        }
    });
}

impl ServiceState {
    /// Runs one relay pass while the space's server configuration names a relay and records
    /// what it moved in `view`. Every relay fault, local or remote, is reported in the status
    /// rather than failing the pass, so LAN exchange continues and the relay status stays
    /// present while a relay is configured. Returns whether the relay holds more to move.
    pub(super) async fn relay_pass<H: SyncHost>(
        &self,
        session: &Session<'_, H>,
        view: &mut SyncStatusView,
    ) -> bool {
        // `self.view` still holds the status the previous pass published.
        let previous = self.view.relay.clone();
        let target = match Self::relay_target(session).await {
            Ok(Some(target)) => target,
            Ok(None) => {
                view.relay = None;
                return false;
            }
            Err(error) => {
                record_relay_outcome(view, Err(error.into()), previous.as_ref());
                return false;
            }
        };
        let outcome = self.run_relay(session, &target).await;
        let more = outcome.as_ref().is_ok_and(|pass| pass.more);
        record_relay_outcome(view, outcome, previous.as_ref());
        more
    }

    /// The relay the selected chain names, `None` when the space has no relay.
    async fn relay_target<H: SyncHost>(
        session: &Session<'_, H>,
    ) -> Result<Option<RelayTarget>, String> {
        let mut conn = acquire(&session.pool).await?;
        let authorization = session
            .engine
            .authorization(&mut conn, &session.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?;
        Ok(authorization.as_ref().and_then(RelayTarget::of))
    }

    /// Prepares this installation's relay session and runs one pass against `target`.
    async fn run_relay<H: SyncHost>(
        &self,
        session: &Session<'_, H>,
        target: &RelayTarget,
    ) -> Result<RelayPass, RelayError> {
        let holder = self
            .writer
            .as_ref()
            .and_then(ActiveWriter::space_keys)
            .ok_or_else(|| {
                RelayError::new(RelayProblem::NoKeys, "this device holds no space keys yet")
            })?;
        let keys = session.keys.clone();
        let vault_id = session.host.active_vault_id()?;
        let admin_vault = vault_id.clone();
        let admin = blocking(move || read_admin_key(keys.as_ref(), &admin_vault)).await?;
        let path = session.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        let assets: Arc<dyn AssetStore> = session.host.asset_store(&vault_id)?;
        let own_writers = self
            .writer
            .as_ref()
            .map(ActiveWriter::own_writers)
            .unwrap_or_default();
        let relay = RelaySession {
            engine: &session.engine,
            pool: &session.pool,
            ctx: &session.ctx,
            holder: holder.as_ref(),
            admin: admin.as_ref(),
            own_writers: &own_writers,
            quarantined: quarantine.devices(),
            assets: Some(&assets),
            limits: RelayLimits {
                max_bytes: crate::client::MAX_EXCHANGE_BYTES,
                max_asset_bytes: relay_asset_bound(session.host),
                loopback: RELAY_LOOPBACK,
            },
        };
        relay.pass(target).await
    }
}
