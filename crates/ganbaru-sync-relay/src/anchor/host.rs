//! The sync host of an anchor: an enrolled replica of a data directory with no views, no hooks
//! beyond the engine's own, and no outbound dials.

use super::state::{Activation, DataPaths, KeyFiles};
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::pairing::PairingManager;
use ganbaru_handoff::protocol::ControlMessage;
use ganbaru_sync::Engine;
use ganbaru_sync_replica::assets::{AssetStore, ContentAddressedStore, InFlightAssets};
use ganbaru_sync_replica::recovery::RecoveryRestorers;
use ganbaru_sync_replica::service::SyncHost;
use ganbaru_sync_replica::service::status::SyncStatusView;
use ganbaru_sync_replica::service::transport::{DialFuture, SyncDialer};
use ganbaru_sync_replica::writer::SyncKeyStore;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

static NO_RESTORERS: LazyLock<RecoveryRestorers> = LazyLock::new(RecoveryRestorers::new);

const NEVER_DIALS: &str = "an anchor server dials nobody";

/// An anchor never dials: every device comes to it.
struct RefusingDialer;

impl SyncDialer for RefusingDialer {
    fn coordinator<'a>(
        &'a self,
        _manager: &'a PairingManager,
        _message: ControlMessage,
    ) -> DialFuture<'a> {
        Box::pin(async { Err(NEVER_DIALS.to_string()) })
    }

    fn member<'a>(
        &'a self,
        _manager: &'a PairingManager,
        _endpoints: &'a [String],
        _fingerprint: &'a str,
        _message: ControlMessage,
    ) -> DialFuture<'a> {
        Box::pin(async { Err(NEVER_DIALS.to_string()) })
    }
}

struct HostParts {
    paths: DataPaths,
    device_id: String,
    pairing: PairingManager,
    pools: DatabasePoolRegistry,
    in_flight: Arc<InFlightAssets>,
}

/// The data directory as a sync host.
#[derive(Clone)]
pub struct AnchorHost(Arc<HostParts>);

impl AnchorHost {
    pub fn new(
        paths: DataPaths,
        device_id: String,
        pairing: PairingManager,
        pools: DatabasePoolRegistry,
    ) -> Self {
        Self(Arc::new(HostParts {
            paths,
            device_id,
            pairing,
            pools,
            in_flight: Arc::default(),
        }))
    }

    pub fn paths(&self) -> &DataPaths {
        &self.0.paths
    }

    pub fn pools(&self) -> &DatabasePoolRegistry {
        &self.0.pools
    }

    pub fn own_device_id(&self) -> &str {
        &self.0.device_id
    }

    pub fn keys(&self) -> KeyFiles {
        KeyFiles::new(self.0.paths.keys())
    }

    pub fn activation(&self) -> Result<Option<Activation>, String> {
        Activation::load(&self.0.paths)
    }

    fn active(&self) -> Result<Activation, String> {
        self.activation()?
            .ok_or_else(|| "this server runs no vault yet".to_string())
    }
}

impl SyncHost for AnchorHost {
    type WritablePath = PathBuf;

    fn config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.paths.config())
    }

    fn pairing(&self) -> PairingManager {
        self.0.pairing.clone()
    }

    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String> {
        Ok(Arc::new(self.keys()))
    }

    fn engine(&self) -> Result<Engine, String> {
        Ok(Engine::vault())
    }

    fn dialer(&self) -> Arc<dyn SyncDialer> {
        Arc::new(RefusingDialer)
    }

    fn asset_store(&self, _vault_id: &str) -> Result<Arc<dyn AssetStore>, String> {
        Ok(Arc::new(
            ContentAddressedStore::new(self.0.paths.vault())
                .with_in_flight(self.0.in_flight.clone()),
        ))
    }

    fn restorers(&self) -> &'static RecoveryRestorers {
        &NO_RESTORERS
    }

    /// The person key never reaches a server.
    async fn person_key(
        &self,
        _public_key: &PersonPublicKey,
    ) -> Result<Option<PersonKeyPair>, String> {
        Ok(None)
    }

    async fn create_identity(&self, _pool: &SqlitePool) -> Result<bool, String> {
        Ok(false)
    }

    /// A snapshot comes from a converted vault, and an anchor writes no domain rows of its own.
    async fn assert_converted(&self, _pool: &SqlitePool) -> Result<(), String> {
        Ok(())
    }

    /// Structure repair is derived state, which a device rebuilds from a stale snapshot.
    async fn repair_notes(&self, _pool: &SqlitePool) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn request_person_key(&self) {}

    fn emit_status(&self, _view: &SyncStatusView) {}

    fn emit_applied(&self, _tables: Vec<String>) {}

    fn is_server(&self) -> bool {
        true
    }

    fn wants_anchor(&self) -> Result<bool, String> {
        Ok(self.activation()?.is_some_and(|active| active.take_anchor))
    }

    fn anchor_taken(&self) -> Result<(), String> {
        match self.activation()? {
            Some(active) if active.take_anchor => Activation {
                take_anchor: false,
                ..active
            }
            .save(&self.0.paths),
            _ => Ok(()),
        }
    }

    fn device_id(&self) -> Result<String, String> {
        Ok(self.0.device_id.clone())
    }

    fn active_vault_id(&self) -> Result<String, String> {
        Ok(self.active()?.vault_id)
    }

    fn active_vault_path(&self) -> Result<PathBuf, String> {
        Ok(self.0.paths.vault())
    }

    fn active_database_path(&self) -> Result<PathBuf, String> {
        Ok(self.0.paths.database())
    }

    async fn connect_for_sync(&self) -> Result<SqlitePool, String> {
        self.0.pools.connect_path(self.0.paths.database()).await
    }

    fn replicated_asset_path(&self) -> Result<PathBuf, String> {
        Ok(self.0.paths.vault())
    }

    /// Sync runs once a vault was activated, whatever pairing state says: an anchor links no
    /// device on its own.
    fn sync_enabled(&self) -> Result<bool, String> {
        Ok(self.activation()?.is_some())
    }
}
