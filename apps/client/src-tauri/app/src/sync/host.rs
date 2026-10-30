//! What a sync pass needs from the process it runs in. The Tauri app is one host; a headless
//! host built from the app config directory runs a pass without a Tauri runtime, such as when
//! Android wakes the app in the background.

use super::key_protection::KeyProtection;
use super::status::{DegradedReasons, SyncRole, SyncStatusView, emit_applied, emit_status};
use super::writer::{SyncFiles, SyncKeyStore};
use crate::db::DatabaseState;
use crate::vault::WritableVaultPath;
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::transport::CoordinatorDialReport;
use crate::vault::write_fence::VaultWriteFence;
#[cfg(any(mobile, test))]
use crate::vault::process_managers::ProcessManagers;
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey};
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use ganbaru_sync::{ChangeNotice, Engine, Hooks};
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::future::Future;
#[cfg(any(mobile, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

/// Directory under the app config directory that holds writer records and carry bundles.
pub(super) const SYNC_DIRECTORY: &str = "sync";

/// The materialize hooks every sync engine of the app registers, so the service, a headless
/// pass, and recovery run the same domain code.
pub(crate) fn vault_hooks() -> Hooks {
    crate::chat::sync::with_hooks(ganbaru_notes::sync_apply::with_hooks(
        ganbaru_calendar::sync_apply::with_hooks(crate::music::sync::with_hooks(
            ganbaru_sync::domains::vault_hooks(),
        )),
    ))
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

/// The process a sync pass runs in: where device state lives, the shared pools, pairing state,
/// the managed write fence, key stores, and where status and change notices go.
pub(crate) trait SyncHost: Clone + Send + Sync + 'static {
    /// The app config directory, which holds device-local state.
    fn config_dir(&self) -> Result<PathBuf, String>;

    /// Pairing state of this device.
    fn pairing(&self) -> PairingManager;

    /// The process pool registry.
    fn pools(&self) -> &DatabaseState;

    /// The fence managed vault writes pass while the vault is not being replaced.
    fn write_fence(&self) -> &VaultWriteFence;

    /// Where writer keys are kept.
    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String>;

    /// The platform key that seals device-local key files and records the keys found lost, when
    /// this host keeps keys in files. A host whose keys live in the operating system keyring has
    /// none.
    fn key_protection(&self) -> Option<Arc<KeyProtection>> {
        None
    }

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

    /// Whether this device seals for the active vault, which it does while it is linked in it.
    fn sync_enabled(&self) -> Result<bool, String> {
        self.pairing().is_linked_for(&self.active_vault_id()?)
    }

    /// Opens the active vault read-write for sync work.
    fn connect_for_sync(&self) -> impl Future<Output = Result<SqlitePool, String>> + Send {
        async move {
            let config_dir = self.config_dir()?;
            crate::db::connect_vault_for_sync_in(self.pools(), &config_dir).await
        }
    }

    /// The active vault root with a managed write permit, for replicated asset files.
    fn replicated_asset_path(&self) -> Result<WritableVaultPath, String> {
        crate::vault::writable_vault_path_in(&self.config_dir()?, self.write_fence())
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

    fn write_fence(&self) -> &VaultWriteFence {
        self.state::<VaultWriteFence>().inner()
    }

    #[cfg(desktop)]
    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String> {
        Ok(Arc::new(super::writer::KeyringStore))
    }

    #[cfg(mobile)]
    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String> {
        Ok(Arc::new(super::writer::FileKeyStore::new(
            self.sync_directory()?,
            crate::vault::process_managers::process_managers().key_wrapper(),
        )))
    }

    #[cfg(mobile)]
    fn key_protection(&self) -> Option<Arc<KeyProtection>> {
        Some(crate::vault::process_managers::process_managers().key_protection())
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
        #[cfg(target_os = "android")]
        super::headless::configure_background(self, view);
    }

    fn emit_applied(&self, tables: Vec<String>) {
        crate::distractions::rule_store::on_sync_changes(self, &tables);
        emit_applied(self, tables);
    }

    async fn refresh_cached(&self, notices: &[ChangeNotice]) {
        crate::vault::preferences::on_sync_notices(self, notices).await;
        crate::projects::sync::on_sync_notices(self, notices);
        crate::calendar::sync::on_sync_notices(self, notices);
        crate::pomodoro::sync::on_sync_notices(self, notices);
        #[cfg(desktop)]
        crate::music::sync::on_sync_notices(self, notices).await;
        #[cfg(desktop)]
        crate::chat::sync::on_sync_notices(self, notices);
    }

    /// A phone serves members only while the app is in the foreground, and checks again after
    /// the listener started, so a move to the background during the start still stops it.
    async fn serve_members(&self) -> Vec<String> {
        #[cfg(mobile)]
        if !self.state::<super::SyncRuntime>().in_foreground() {
            self.stop_serving_members().await;
            return Vec::new();
        }
        let endpoints = match crate::vault::handoff::ensure_member_listener(self).await {
            Ok(endpoints) => endpoints,
            Err(error) => {
                eprintln!("sync: member listener: {error}");
                Vec::new()
            }
        };
        #[cfg(mobile)]
        if !self.state::<super::SyncRuntime>().in_foreground() {
            self.stop_serving_members().await;
            return Vec::new();
        }
        endpoints
    }

    async fn stop_serving_members(&self) {
        self.state::<crate::vault::handoff::MemberListenerLifecycle>()
            .stop()
            .await;
    }

    async fn note_platform_degraded(
        &self,
        role: SyncRole,
        dial: Option<&CoordinatorDialReport>,
        reasons: &mut DegradedReasons,
    ) {
        #[cfg(target_os = "linux")]
        super::connectivity::firewall::note(self, role, dial, reasons).await;
        #[cfg(target_os = "android")]
        {
            let _ = (role, dial);
            super::connectivity::note_android(self, reasons).await;
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let _ = (role, dial, reasons);
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
    write_fence: VaultWriteFence,
    pools: DatabaseState,
    key_protection: Arc<KeyProtection>,
    hooks: Box<dyn Fn() -> Hooks + Send + Sync>,
}

#[cfg(any(mobile, test))]
impl HeadlessHost {
    /// Opens a host on the managers this process shares with the app runtime.
    #[cfg(mobile)]
    pub(crate) fn open(config_dir: &Path) -> Result<Self, String> {
        Self::open_with(
            config_dir,
            crate::vault::process_managers::process_managers(),
        )
    }

    /// Opens a host on `managers`, loading pairing state from `config_dir` as app startup does
    /// unless the process already loaded it. The host reads only the app config directory and
    /// the active vault it points at.
    pub(crate) fn open_with(config_dir: &Path, managers: &ProcessManagers) -> Result<Self, String> {
        let device_id = crate::vault::ensure_device_id_in(config_dir)?;
        managers.load_pairing(config_dir, device_id)?;
        Ok(Self(Arc::new(HeadlessParts {
            config_dir: config_dir.to_path_buf(),
            pairing: managers.pairing().clone(),
            write_fence: managers.write_fence().clone(),
            pools: crate::db::vault_pools().clone(),
            key_protection: managers.key_protection(),
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

    fn write_fence(&self) -> &VaultWriteFence {
        &self.0.write_fence
    }

    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String> {
        Ok(Arc::new(super::writer::FileKeyStore::new(
            self.sync_directory()?,
            self.0.key_protection.clone(),
        )))
    }

    fn key_protection(&self) -> Option<Arc<KeyProtection>> {
        Some(self.0.key_protection.clone())
    }

    fn engine(&self) -> Result<Engine, String> {
        vault_engine((self.0.hooks)())
    }

    async fn person_key(
        &self,
        public_key: &PersonPublicKey,
    ) -> Result<Option<PersonKeyPair>, String> {
        let vault_id = self.active_vault_id()?;
        let path = crate::contacts::identity::person_key_path_in(&self.0.config_dir, &vault_id);
        let public_key = *public_key;
        let wrapper = self.0.key_protection.clone();
        super::service::blocking(move || {
            crate::contacts::identity::person_key_file_matching(
                wrapper.as_ref(),
                &path,
                &vault_id,
                &public_key,
            )
        })
        .await
    }

    async fn create_identity(&self, _pool: &SqlitePool) -> Result<bool, String> {
        Ok(false)
    }

    fn request_person_key(&self) {}

    fn emit_status(&self, _view: &SyncStatusView) {}

    /// A background pass has no views, no Guardian runtime, and no notification scheduler, so
    /// reminders, Android alarms, and Guardian rules it changes refresh when the app next opens.
    fn emit_applied(&self, _tables: Vec<String>) {}
}
