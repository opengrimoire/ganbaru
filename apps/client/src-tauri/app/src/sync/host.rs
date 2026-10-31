//! The app's sync hosts. The Tauri app is one host; a headless host built from the app config
//! directory runs a pass without a Tauri runtime, such as when Android wakes the app in the
//! background. The trait itself is [`ganbaru_sync_replica::service::SyncHost`].

#[cfg(any(mobile, test))]
use super::key_protection::KeyProtection;
use super::status::{DegradedReasons, SyncRole, SyncStatusView, emit_applied, emit_status};
use super::transport::AppDialer;
use super::vault_assets::ActiveVaultAssets;
use super::writer::SyncKeyStore;
use crate::db::DatabaseState;
use crate::vault::WritableVaultPath;
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::transport::CoordinatorDialReport;
#[cfg(any(mobile, test))]
use crate::vault::process_managers::ProcessManagers;
use crate::vault::write_fence::VaultWriteFence;
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey};
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use ganbaru_sync::{ChangeNotice, Engine, Hooks};
use ganbaru_sync_replica::assets::AssetStore;
use ganbaru_sync_replica::recovery::RecoveryRestorers;
use ganbaru_sync_replica::service::transport::SyncDialer;
pub(crate) use ganbaru_sync_replica::service::{SYNC_DIRECTORY, SyncHost};
use sqlx::SqlitePool;
#[cfg(desktop)]
use std::collections::BTreeMap;
use std::ops::Deref;
#[cfg(any(mobile, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

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

/// Rebuilds the derived state of the vault staged at `staging` before anything opens it, for a
/// copy whose source seals none: an anchor server, or an encrypted backup it wrote.
pub(crate) async fn rebuild_staged_vault(staging: &std::path::Path) -> Result<(), String> {
    let pools = ganbaru_db::DatabasePoolRegistry::default();
    let pool = pools
        .connect_path(staging.join(crate::vault::APP_SQLITE_FILE))
        .await?;
    let rebuilt = async {
        let mut conn = pool
            .acquire()
            .await
            .map_err(|error| format!("acquire sync connection: {error}"))?;
        vault_engine(vault_hooks())?
            .rebuild_derived_state(&mut conn)
            .await
            .map_err(|error| format!("rebuild derived state: {error}"))
    }
    .await;
    let closed = pools.close_all().await;
    rebuilt.and(closed)
}

/// Opens the active vault of `config_dir` read-write for sync work through `pools`.
async fn connect_for_sync_in(
    pools: &DatabaseState,
    config_dir: PathBuf,
) -> Result<SqlitePool, String> {
    crate::db::connect_vault_for_sync_in(pools, &config_dir).await
}

/// Refuses Projects, Calendar, Notes, and Chat placeholder keys and unconverted Chat
/// attachment files.
async fn assert_converted(pool: &SqlitePool) -> Result<(), String> {
    ganbaru_projects::conversion::assert_converted(pool).await?;
    ganbaru_calendar::conversion::assert_converted(pool).await?;
    ganbaru_notes::sync_keys::assert_converted(pool).await?;
    ganbaru_chat::conversion::assert_converted(pool).await?;
    ganbaru_chat::attachment_files::assert_converted(pool).await
}

/// Repairs the Notes structure around the keys waiting in `notes_sync_repair_pending` and
/// returns the Notes tables it changed.
async fn repair_notes(pool: &SqlitePool) -> Result<Vec<String>, String> {
    if !ganbaru_notes::sync_repair::has_pending(pool).await? {
        return Ok(Vec::new());
    }
    let repair = ganbaru_notes::sync_repair::repair_pending(pool).await?;
    for failed in &repair.failed_keys {
        eprintln!("notes structure repair kept a failing key pending: {failed}");
    }
    if !repair.unplaced_blocks.is_empty() {
        eprintln!(
            "notes structure repair left {} block cycles without a page",
            repair.unplaced_blocks.len()
        );
    }
    if repair.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![
        ganbaru_sync::manifest::vault::notes::PAGES.name.to_string(),
        ganbaru_sync::manifest::vault::notes::BLOCKS
            .name
            .to_string(),
    ])
}

/// LAN discovery of the member desktops in `fingerprints` for `vault_id`.
#[cfg(desktop)]
async fn discover_members(
    vault_id: &str,
    fingerprints: &[(String, [u8; 32])],
) -> BTreeMap<String, Vec<String>> {
    crate::vault::handoff::discovery::discover_members(vault_id, fingerprints)
        .await
        .into_iter()
        .map(|(device_id, endpoints)| {
            let endpoints = endpoints.iter().map(ToString::to_string).collect();
            (device_id, endpoints)
        })
        .collect()
}

/// Whether the network of the running pass is metered. Only a relay worker run probes its
/// network, so a foreground pass or another background pass counts as metered.
#[cfg(target_os = "android")]
fn android_relay_metered() -> bool {
    super::headless::relay_metered().unwrap_or(true)
}

/// Recovers after a failed pass when opening a sealed key file reported its platform key lost,
/// so the device enrolls again with fresh keys instead of retrying a key it cannot recover; see
/// [`super::key_protection::recover_after_key_loss`]. Only the losses the key wrapper recorded
/// count, never the pass error, whose text can come from a remote hub.
#[cfg(any(mobile, test))]
async fn recover_if_key_protection_lost<H: SyncHost>(host: &H, protection: Arc<KeyProtection>) {
    let lost = protection.take_lost();
    if lost.is_empty() {
        return;
    }
    let pairing = host.pairing();
    let result = match host.config_dir() {
        Ok(config_dir) => {
            super::service::blocking(move || {
                super::key_protection::recover_after_key_loss(
                    &config_dir,
                    &pairing,
                    protection.as_ref(),
                    &lost,
                )
            })
            .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("sync: recover after losing protected keys: {error}");
    }
}

/// The Tauri app as a sync host. It derefs to the app handle, so managed state reads as on the
/// handle.
pub(crate) struct AppHost<R: Runtime>(pub(crate) AppHandle<R>);

impl<R: Runtime> AppHost<R> {
    /// The host of the app `app`.
    pub(crate) fn of(app: &AppHandle<R>) -> Self {
        Self(app.clone())
    }
}

impl<R: Runtime> Clone for AppHost<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Runtime> Deref for AppHost<R> {
    type Target = AppHandle<R>;

    fn deref(&self) -> &AppHandle<R> {
        &self.0
    }
}

impl<R: Runtime> SyncHost for AppHost<R> {
    type WritablePath = WritableVaultPath;

    fn config_dir(&self) -> Result<PathBuf, String> {
        self.path()
            .app_config_dir()
            .map_err(|error| format!("find app config directory: {error}"))
    }

    fn pairing(&self) -> PairingManager {
        self.state::<PairingManager>().inner().clone()
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

    fn engine(&self) -> Result<Engine, String> {
        vault_engine(vault_hooks())
    }

    fn dialer(&self) -> Arc<dyn SyncDialer> {
        Arc::new(AppDialer)
    }

    fn asset_store(&self, vault_id: &str) -> Result<Arc<dyn AssetStore>, String> {
        Ok(Arc::new(ActiveVaultAssets::open(self, vault_id)?))
    }

    fn restorers(&self) -> &'static RecoveryRestorers {
        super::recovery::restorers()
    }

    async fn person_key(
        &self,
        public_key: &PersonPublicKey,
    ) -> Result<Option<PersonKeyPair>, String> {
        crate::contacts::identity::person_key_matching(self, public_key).await
    }

    async fn assert_converted(&self, pool: &SqlitePool) -> Result<(), String> {
        assert_converted(pool).await
    }

    async fn repair_notes(&self, pool: &SqlitePool) -> Result<Vec<String>, String> {
        repair_notes(pool).await
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

    #[cfg(desktop)]
    async fn discover_members(
        &self,
        vault_id: &str,
        fingerprints: &[(String, [u8; 32])],
    ) -> BTreeMap<String, Vec<String>> {
        discover_members(vault_id, fingerprints).await
    }

    #[cfg(mobile)]
    async fn recover_lost_keys(&self) {
        recover_if_key_protection_lost(
            self,
            crate::vault::process_managers::process_managers().key_protection(),
        )
        .await;
    }

    #[cfg(target_os = "android")]
    fn relay_metered(&self) -> bool {
        android_relay_metered()
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

    fn device_id(&self) -> Result<String, String> {
        crate::vault::ensure_device_id_in(&self.config_dir()?)
    }

    fn active_vault_id(&self) -> Result<String, String> {
        crate::vault::active_vault_id_in(&self.config_dir()?)
    }

    fn active_vault_path(&self) -> Result<PathBuf, String> {
        crate::vault::active_vault_path_in(&self.config_dir()?)
    }

    fn active_database_path(&self) -> Result<PathBuf, String> {
        crate::vault::active_database_path_in(&self.config_dir()?)
    }

    async fn connect_for_sync(&self) -> Result<SqlitePool, String> {
        let config_dir = self.config_dir()?;
        connect_for_sync_in(self.state::<DatabaseState>().inner(), config_dir).await
    }

    fn replicated_asset_path(&self) -> Result<WritableVaultPath, String> {
        crate::vault::writable_vault_path_in(
            &self.config_dir()?,
            self.state::<VaultWriteFence>().inner(),
        )
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
    type WritablePath = WritableVaultPath;

    fn config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.config_dir.clone())
    }

    fn pairing(&self) -> PairingManager {
        self.0.pairing.clone()
    }

    fn key_store(&self) -> Result<Arc<dyn SyncKeyStore>, String> {
        Ok(Arc::new(super::writer::FileKeyStore::new(
            self.sync_directory()?,
            self.0.key_protection.clone(),
        )))
    }

    fn engine(&self) -> Result<Engine, String> {
        vault_engine((self.0.hooks)())
    }

    fn dialer(&self) -> Arc<dyn SyncDialer> {
        Arc::new(AppDialer)
    }

    fn asset_store(&self, vault_id: &str) -> Result<Arc<dyn AssetStore>, String> {
        Ok(Arc::new(ActiveVaultAssets::open(self, vault_id)?))
    }

    fn restorers(&self) -> &'static RecoveryRestorers {
        super::recovery::restorers()
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

    async fn assert_converted(&self, pool: &SqlitePool) -> Result<(), String> {
        assert_converted(pool).await
    }

    async fn repair_notes(&self, pool: &SqlitePool) -> Result<Vec<String>, String> {
        repair_notes(pool).await
    }

    async fn create_identity(&self, _pool: &SqlitePool) -> Result<bool, String> {
        Ok(false)
    }

    fn request_person_key(&self) {}

    fn emit_status(&self, _view: &SyncStatusView) {}

    /// A background pass has no views, no Guardian runtime, and no notification scheduler, so
    /// views, reminders, Android alarms, and Guardian rules it changes refresh when the app next
    /// reaches the foreground.
    fn emit_applied(&self, tables: Vec<String>) {
        super::background_changes::record_tables(tables);
    }

    async fn refresh_cached(&self, notices: &[ChangeNotice]) {
        super::background_changes::record_notices(notices);
    }

    #[cfg(desktop)]
    async fn discover_members(
        &self,
        vault_id: &str,
        fingerprints: &[(String, [u8; 32])],
    ) -> BTreeMap<String, Vec<String>> {
        discover_members(vault_id, fingerprints).await
    }

    async fn recover_lost_keys(&self) {
        recover_if_key_protection_lost(self, self.0.key_protection.clone()).await;
    }

    #[cfg(target_os = "android")]
    fn relay_metered(&self) -> bool {
        android_relay_metered()
    }

    fn device_id(&self) -> Result<String, String> {
        crate::vault::ensure_device_id_in(&self.0.config_dir)
    }

    fn active_vault_id(&self) -> Result<String, String> {
        crate::vault::active_vault_id_in(&self.0.config_dir)
    }

    fn active_vault_path(&self) -> Result<PathBuf, String> {
        crate::vault::active_vault_path_in(&self.0.config_dir)
    }

    fn active_database_path(&self) -> Result<PathBuf, String> {
        crate::vault::active_database_path_in(&self.0.config_dir)
    }

    async fn connect_for_sync(&self) -> Result<SqlitePool, String> {
        connect_for_sync_in(&self.0.pools, self.0.config_dir.clone()).await
    }

    fn replicated_asset_path(&self) -> Result<WritableVaultPath, String> {
        crate::vault::writable_vault_path_in(&self.0.config_dir, &self.0.write_fence)
    }
}
