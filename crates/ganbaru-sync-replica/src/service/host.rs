//! What a sync pass needs from the process it runs in: device-local state, the active vault,
//! pairing, key stores, the dialer, and where status and change notices go.

use super::dial_report::CoordinatorDialReport;
use super::status::{DegradedReasons, SyncRole, SyncStatusView};
use super::transport::SyncDialer;
use crate::assets::AssetStore;
use crate::recovery::RecoveryRestorers;
use crate::writer::{SyncFiles, SyncKeyStore};
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey};
use ganbaru_handoff::pairing::PairingManager;
use ganbaru_sync::{ChangeNotice, Engine};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Directory under the app config directory that holds writer records and carry bundles.
pub const SYNC_DIRECTORY: &str = "sync";

/// The process a sync pass runs in: where device state lives, the active vault, pairing state,
/// key stores, the dialer, and where status and change notices go.
pub trait SyncHost: Clone + Send + Sync + 'static {
    /// The active vault root held with a managed write permit, for replicated asset files.
    type WritablePath: Deref<Target = Path> + AsRef<Path> + Send + 'static;

    /// The app config directory, which holds device-local state.
    fn config_dir(&self) -> Result<PathBuf, String>;

    /// Pairing state of this device.
    fn pairing(&self) -> PairingManager;

    /// Where writer keys are kept.
    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String>;

    /// The engine a pass seals and applies with, with the registered hooks.
    fn engine(&self) -> Result<Engine, String>;

    /// How requests reach the coordinator and member listeners.
    fn dialer(&self) -> Arc<dyn SyncDialer>;

    /// The managed asset files of the active vault `vault_id`.
    fn asset_store(&self, vault_id: &str) -> Result<Arc<dyn AssetStore>, String>;

    /// The restorers of the domains whose rows can root a recovery offer.
    fn restorers(&self) -> &'static RecoveryRestorers;

    /// The device-local copy of the person key when it matches `public_key`. It reads no vault
    /// database, so it is usable while the vault is quiesced.
    fn person_key(
        &self,
        public_key: &PersonPublicKey,
    ) -> impl Future<Output = Result<Option<PersonKeyPair>, String>> + Send;

    /// Creates the person identity the sync space is anchored at when this device generates
    /// it. Returns whether an identity now exists.
    fn create_identity(
        &self,
        pool: &SqlitePool,
    ) -> impl Future<Output = Result<bool, String>> + Send;

    /// Refuses a vault whose domain rows still hold placeholder keys or unconverted files, which
    /// cannot replicate.
    fn assert_converted(
        &self,
        pool: &SqlitePool,
    ) -> impl Future<Output = Result<(), String>> + Send;

    /// Repairs the Notes structure around the Notes rows that applies changed and returns the
    /// tables the repair changed. Keys that still fail stay pending for a later pass.
    fn repair_notes(
        &self,
        pool: &SqlitePool,
    ) -> impl Future<Output = Result<Vec<String>, String>> + Send;

    /// Asks the coordinator for the person key in the background.
    fn request_person_key(&self);

    /// Shows the status of the active vault.
    fn emit_status(&self, view: &SyncStatusView);

    /// Tells views that rows of `tables` changed.
    fn emit_applied(&self, tables: Vec<String>);

    /// Refreshes process state that caches rows the hooks reported, such as the composed
    /// preference view. A host without such caches does nothing.
    fn refresh_cached(&self, _notices: &[ChangeNotice]) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Tells views which domain tables a pass changed and what the hooks reported, once per
    /// table and topic, after the cached rows were refreshed.
    fn report_changes(
        &self,
        tables: impl IntoIterator<Item = String> + Send,
        notices: &[ChangeNotice],
    ) -> impl Future<Output = ()> + Send {
        let mut changed: BTreeSet<String> = tables.into_iter().collect();
        changed.extend(notices.iter().map(|notice| notice.topic.to_string()));
        async move {
            self.refresh_cached(notices).await;
            self.emit_applied(changed.into_iter().collect());
        }
    }

    /// Adds the degraded reasons the platform probes find for a device in `role`, given what
    /// its latest coordinator dial found. A host without views probes nothing.
    fn note_platform_degraded(
        &self,
        _role: SyncRole,
        _dial: Option<&CoordinatorDialReport>,
        _reasons: &mut DegradedReasons,
    ) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Whether this host is a headless always-on server, which links no device on its own: it
    /// keeps enrolled devices it has no link to, and its log does not wait for members to return.
    fn is_server(&self) -> bool {
        false
    }

    /// Whether this host should take the anchor as soon as it can. Only a server asks.
    fn wants_anchor(&self) -> Result<bool, String> {
        Ok(false)
    }

    /// Tells a host that asked for the anchor that it holds it, so a later handover away from it
    /// stands.
    fn anchor_taken(&self) -> Result<(), String> {
        Ok(())
    }

    /// Keeps this device's member sync listener running for a pass and returns the endpoints
    /// it is reachable at, which the pass reports to the hubs it dials. A host that serves no
    /// listener reports none.
    fn serve_members(&self) -> impl Future<Output = Vec<String>> + Send {
        async { Vec::new() }
    }

    /// Stops this device's member sync listener while sync does not run in the active vault.
    fn stop_serving_members(&self) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Endpoints of the devices in `fingerprints` that answer LAN discovery for `vault_id`, by
    /// device id. A host that does not browse finds none.
    fn discover_members(
        &self,
        _vault_id: &str,
        _fingerprints: &[(String, [u8; 32])],
    ) -> impl Future<Output = BTreeMap<String, Vec<String>>> + Send {
        async { BTreeMap::new() }
    }

    /// Recovers after a failed pass when the platform key that seals device-local key files
    /// was found lost, so the device enrolls again with fresh keys. A host whose keys live in
    /// the operating system keyring has nothing to recover.
    fn recover_lost_keys(&self) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Whether a relay pass runs on a metered network, which keeps it to small asset files.
    /// A host that cannot be on a metered network reports `false`.
    fn relay_metered(&self) -> bool {
        false
    }

    /// This device's id.
    fn device_id(&self) -> Result<String, String>;

    /// The id of the active vault.
    fn active_vault_id(&self) -> Result<String, String>;

    /// The root of the active vault.
    fn active_vault_path(&self) -> Result<PathBuf, String>;

    /// The database file of the active vault.
    fn active_database_path(&self) -> Result<PathBuf, String>;

    /// Opens the active vault read-write for sync work.
    fn connect_for_sync(&self) -> impl Future<Output = Result<SqlitePool, String>> + Send;

    /// The active vault root with a managed write permit, for replicated asset files.
    fn replicated_asset_path(&self) -> Result<Self::WritablePath, String>;

    /// The device-local sync directory, created on first use.
    fn sync_directory(&self) -> Result<PathBuf, String> {
        let directory = self.config_dir()?.join(SYNC_DIRECTORY);
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("create sync directory: {error}"))?;
        Ok(directory)
    }

    /// Device-local sync files of `vault_id`.
    fn sync_files(&self, vault_id: &str) -> Result<SyncFiles, String> {
        SyncFiles::new(self.sync_directory()?, vault_id)
    }

    /// Whether this device seals for the active vault, which it does while it is linked in it or
    /// after recovering it through a server, which leaves no link.
    fn sync_enabled(&self) -> Result<bool, String> {
        let vault_id = self.active_vault_id()?;
        Ok(self.pairing().is_linked_for(&vault_id)?
            || self.sync_files(&vault_id)?.server_recovery_path().exists())
    }
}
