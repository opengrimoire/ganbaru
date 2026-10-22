//! What a sync pass needs from the process it runs in. The Tauri app is one host; a headless
//! host built from the app config directory runs a pass without a Tauri runtime, such as when
//! Android wakes the app in the background.

use super::status::{SyncStatusView, emit_applied, emit_status};
use super::writer::{SyncFiles, WriterKeyStore};
use crate::db::DatabaseState;
use crate::vault::WritableVaultPath;
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::ownership::VaultOwnershipManager;
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey};
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use ganbaru_sync::{ApplyReport, ChangeNotice, Engine, Hooks};
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::future::Future;
#[cfg(any(mobile, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

/// Directory under the app config directory that holds writer records and carry bundles.
const SYNC_DIRECTORY: &str = "sync";

/// The materialize hooks every sync engine of the app registers, so the service, a headless
/// pass, and recovery run the same domain code.
pub(crate) fn vault_hooks() -> Hooks {
    ganbaru_sync::domains::vault_hooks()
}

/// The vault engine with `hooks`.
fn vault_engine(hooks: Hooks) -> Result<Engine, String> {
    Engine::with(
        &VAULT_MANIFEST,
        ganbaru_sync::domains::vault_adapters(),
        hooks,
    )
    .map_err(|error| format!("build sync engine: {error}"))
}

/// The process a sync pass runs in: where device state lives, the shared pools, pairing and
/// ownership state, key stores, and where status and change notices go.
pub(crate) trait SyncHost: Clone + Send + Sync + 'static {
    /// The app config directory, which holds device-local state.
    fn config_dir(&self) -> Result<PathBuf, String>;

    /// Pairing state of this device.
    fn pairing(&self) -> PairingManager;

    /// The process pool registry.
    fn pools(&self) -> &DatabaseState;

    /// Durable vault ownership of this device.
    fn ownership(&self) -> &VaultOwnershipManager;

    /// Where writer keys are kept.
    fn key_store(&self) -> Result<Arc<dyn WriterKeyStore>, String>;

    /// The engine a pass seals and applies with, with the registered hooks.
    fn engine(&self) -> Result<Engine, String>;

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

    /// Asks the coordinator for the person key in the background.
    fn request_person_key(&self);

    /// Shows the status of the active vault.
    fn emit_status(&self, view: &SyncStatusView);

    /// Tells views that rows of `tables` changed.
    fn emit_applied(&self, tables: Vec<String>);

    /// Updates process state that caches rows `report` applied, such as the composed preference
    /// view. A host without such caches does nothing.
    fn on_sync_applied(&self, _report: &ApplyReport) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Tells views about the changes hooks reported, once per topic.
    fn report_changes(&self, notices: &[ChangeNotice]) {
        let topics: BTreeSet<&str> = notices.iter().map(|notice| notice.topic).collect();
        self.emit_applied(topics.into_iter().map(str::to_string).collect());
    }

    /// This device's id.
    fn device_id(&self) -> Result<String, String> {
        crate::vault::ensure_device_id_in(&self.config_dir()?)
    }

    /// The id of the active vault.
    fn active_vault_id(&self) -> Result<String, String> {
        crate::vault::active_vault_id_in(&self.config_dir()?)
    }

    /// The root of the active vault.
    fn active_vault_path(&self) -> Result<PathBuf, String> {
        crate::vault::active_vault_path_in(&self.config_dir()?)
    }

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

    /// Whether this device seals for the active vault: a linked owner, or a guarded replica.
    fn sync_enabled(&self) -> Result<bool, String> {
        let status =
            crate::vault::ownership::active_status_in(&self.config_dir()?, self.ownership())?;
        if status.linked_replica {
            return Ok(true);
        }
        Ok(status.can_write && self.ownership().is_linked(&status.vault_id))
    }

    /// Opens the active vault for sync work, or `None` while the pool cannot write.
    fn connect_for_sync(&self) -> impl Future<Output = Result<Option<SqlitePool>, String>> + Send {
        async move {
            let config_dir = self.config_dir()?;
            crate::db::connect_vault_for_sync_in(self.pools(), self.ownership(), &config_dir).await
        }
    }

    /// The active vault root with the replicated asset permit of the sync `domain`.
    fn replicated_asset_path(&self, domain: &str) -> Result<WritableVaultPath, String> {
        crate::vault::replicated_asset_path_in(&self.config_dir()?, self.ownership(), domain)
    }
}

impl<R: Runtime> SyncHost for AppHandle<R> {
    fn config_dir(&self) -> Result<PathBuf, String> {
        self.path()
            .app_config_dir()
            .map_err(|error| format!("find app config directory: {error}"))
    }

    fn pairing(&self) -> PairingManager {
        self.state::<PairingManager>().inner().clone()
    }

    fn pools(&self) -> &DatabaseState {
        self.state::<DatabaseState>().inner()
    }

    fn ownership(&self) -> &VaultOwnershipManager {
        self.state::<VaultOwnershipManager>().inner()
    }

    #[cfg(desktop)]
    fn key_store(&self) -> Result<Arc<dyn WriterKeyStore>, String> {
        Ok(Arc::new(super::writer::KeyringStore))
    }

    #[cfg(mobile)]
    fn key_store(&self) -> Result<Arc<dyn WriterKeyStore>, String> {
        Ok(Arc::new(super::writer::FileKeyStore {
            directory: self.sync_directory()?,
        }))
    }

    fn engine(&self) -> Result<Engine, String> {
        vault_engine(vault_hooks())
    }

    async fn person_key(
        &self,
        public_key: &PersonPublicKey,
    ) -> Result<Option<PersonKeyPair>, String> {
        crate::contacts::identity::person_key_matching(self, public_key).await
    }

    async fn create_identity(&self, pool: &SqlitePool) -> Result<bool, String> {
        match crate::contacts::identity::ensure_identity(self, pool).await {
            Ok(_) => Ok(true),
            Err(crate::contacts::ContactsError::IdentityUnavailable(_)) => Ok(false),
            Err(error) => Err(format!("create person identity: {}", error.into_message())),
        }
    }

    fn request_person_key(&self) {
        let app = self.clone();
        tauri::async_runtime::spawn(async move {
            crate::contacts::identity::try_release(&app).await;
        });
    }

    fn emit_status(&self, view: &SyncStatusView) {
        emit_status(self, view);
    }

    fn emit_applied(&self, tables: Vec<String>) {
        emit_applied(self, tables);
    }

    async fn on_sync_applied(&self, report: &ApplyReport) {
        crate::vault::preferences::on_sync_applied(self, report).await;
    }
}

/// A host without a Tauri runtime, built from the app config directory. It shares the process
/// pool registry, reads the person key from its file, never creates an identity or requests a
/// key, and has no views to notify, so status and change notices are dropped.
#[cfg(any(mobile, test))]
#[derive(Clone)]
pub(crate) struct HeadlessHost(Arc<HeadlessParts>);

#[cfg(any(mobile, test))]
struct HeadlessParts {
    config_dir: PathBuf,
    pairing: PairingManager,
    ownership: VaultOwnershipManager,
    pools: DatabaseState,
    hooks: Box<dyn Fn() -> Hooks + Send + Sync>,
}

#[cfg(any(mobile, test))]
impl HeadlessHost {
    /// Loads pairing and ownership state from `config_dir` as app startup does. A vault
    /// handoff this device started is left to the app, which reconciles it at startup, so the
    /// host refuses to open while one is recorded.
    pub(crate) fn open(config_dir: &Path) -> Result<Self, String> {
        let device_id = crate::vault::ensure_device_id_in(config_dir)?;
        let pairing = PairingManager::default();
        pairing.initialize(config_dir.to_path_buf(), device_id)?;
        if pairing.outgoing_transfer()?.is_some() {
            return Err("a vault handoff is in progress; the app finishes it".to_string());
        }
        let ownership = VaultOwnershipManager::default();
        ownership.initialize_in(config_dir)?;
        if let Some(coordinator) = pairing.coordinator_pin()? {
            ownership.register_remote_owner_if_missing(
                &coordinator.vault_id,
                coordinator.device_id,
                coordinator.generation,
            )?;
        }
        let probe = pairing.clone();
        ownership.set_link_probe(Some(Arc::new(move |vault_id: &str| {
            probe.is_linked_for(vault_id).unwrap_or(false)
        })))?;
        Ok(Self(Arc::new(HeadlessParts {
            config_dir: config_dir.to_path_buf(),
            pairing,
            ownership,
            pools: crate::db::vault_pools().clone(),
            hooks: Box::new(vault_hooks),
        })))
    }

    /// The same host with engines that register the hooks `hooks` builds.
    #[cfg(test)]
    pub(crate) fn with_hooks(self, hooks: impl Fn() -> Hooks + Send + Sync + 'static) -> Self {
        let parts = Arc::try_unwrap(self.0)
            .unwrap_or_else(|_| panic!("a host with hooks is built before it is shared"));
        Self(Arc::new(HeadlessParts {
            hooks: Box::new(hooks),
            ..parts
        }))
    }
}

#[cfg(any(mobile, test))]
impl SyncHost for HeadlessHost {
    fn config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.config_dir.clone())
    }

    fn pairing(&self) -> PairingManager {
        self.0.pairing.clone()
    }

    fn pools(&self) -> &DatabaseState {
        &self.0.pools
    }

    fn ownership(&self) -> &VaultOwnershipManager {
        &self.0.ownership
    }

    fn key_store(&self) -> Result<Arc<dyn WriterKeyStore>, String> {
        Ok(Arc::new(super::writer::FileKeyStore {
            directory: self.sync_directory()?,
        }))
    }

    fn engine(&self) -> Result<Engine, String> {
        vault_engine((self.0.hooks)())
    }

    async fn person_key(
        &self,
        public_key: &PersonPublicKey,
    ) -> Result<Option<PersonKeyPair>, String> {
        let path = crate::contacts::identity::person_key_path_in(
            &self.0.config_dir,
            &self.active_vault_id()?,
        );
        let public_key = *public_key;
        super::service::blocking(move || {
            crate::contacts::identity::person_key_file_matching(&path, &public_key)
        })
        .await
    }

    async fn create_identity(&self, _pool: &SqlitePool) -> Result<bool, String> {
        Ok(false)
    }

    fn request_person_key(&self) {}

    fn emit_status(&self, _view: &SyncStatusView) {}

    fn emit_applied(&self, _tables: Vec<String>) {}
}
