//! The app's side of the sync service: it hands the service the managed runtime state, the
//! vault commit notice, and the Tauri async runtime. The service itself is
//! [`ganbaru_sync_replica::service`].

use super::host::AppHost;
use super::writer::SyncFiles;
use crate::sync::hub::{HubPeerHints, SyncHub};
#[cfg(desktop)]
use ganbaru_sync_contracts::AdminPublicKey;
use ganbaru_sync_contracts::DevicePublicKey;
use ganbaru_sync_replica::peers::book::PeerEndpoints;
#[cfg(desktop)]
use ganbaru_sync_replica::serve::coordinator::QuarantinedDevices;
use ganbaru_sync_replica::serve::member::MemberAdmissionSource;
#[cfg(any(mobile, test))]
pub(crate) use ganbaru_sync_replica::service::HeadlessPass;
use ganbaru_sync_replica::service::peers::BundleSource;
use ganbaru_sync_replica::service::{self as replica, QuiescedSync, ServiceStart};
#[cfg(desktop)]
pub(crate) use ganbaru_sync_replica::service::{
    AuthorityAction, AuthorityAnswer, ForceRequest, LateDecision,
};
pub(crate) use ganbaru_sync_replica::service::{
    RELAY_LOOPBACK, SyncRuntime, blocking, release_runtime_handle,
};
#[cfg(mobile)]
use std::path::PathBuf;
#[cfg(desktop)]
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

/// The device key this installation signs with in `vault_id`, created on first use.
pub(crate) async fn ensure_device_key<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<DevicePublicKey, String> {
    replica::ensure_device_key(&AppHost::of(app), vault_id).await
}

/// The admin key this desktop holds for `vault_id`, created on first use.
#[cfg(desktop)]
pub(crate) async fn ensure_admin_key<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<AdminPublicKey, String> {
    replica::ensure_admin_key(&AppHost::of(app), vault_id).await
}

/// The admin key this desktop holds for `vault_id`, if any.
#[cfg(desktop)]
pub(crate) async fn stored_admin_key<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<Option<AdminPublicKey>, String> {
    replica::stored_admin_key(&AppHost::of(app), vault_id).await
}

/// Device-local sync files of `vault_id`.
pub(crate) fn sync_files<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<SyncFiles, String> {
    replica::sync_files(&AppHost::of(app), vault_id)
}

/// What a vault replacement seals before the vault is quiesced, or `None` while sync is off.
pub(crate) fn quiesced_sync<R: Runtime>(app: &AppHandle<R>) -> Option<QuiescedSync> {
    replica::quiesced_sync(&AppHost::of(app))
}

/// Seals the local changes `plan` names while the vault is quiesced.
pub(crate) async fn seal_quiesced<R: Runtime>(app: &AppHandle<R>, plan: Option<QuiescedSync>) {
    replica::seal_quiesced(&AppHost::of(app), plan).await;
}

/// How the member listener reads which devices the selected log admits.
pub(crate) fn member_admission_source<R: Runtime>(app: &AppHandle<R>) -> MemberAdmissionSource {
    replica::member_admission_source(&AppHost::of(app))
}

/// How the coordinator listener reads which devices the selected log admits.
#[cfg(desktop)]
pub(crate) fn log_admission_source<R: Runtime>(app: &AppHandle<R>) -> MemberAdmissionSource {
    replica::log_admission_source(&AppHost::of(app))
}

/// How the coordinator listener reads the devices this installation quarantined.
#[cfg(desktop)]
pub(crate) fn quarantined_devices<R: Runtime>(app: &AppHandle<R>) -> QuarantinedDevices {
    replica::quarantined_devices(&AppHost::of(app))
}

/// The peer hints a hub gives the devices it serves.
pub(crate) fn peer_hints<R: Runtime>(app: AppHandle<R>) -> HubPeerHints {
    replica::peers::peer_hints(AppHost(app))
}

/// The member endpoints the peer book of `vault_id` keeps.
pub(crate) fn read_peer_book<R: Runtime>(app: &AppHandle<R>, vault_id: &str) -> PeerEndpoints {
    replica::peers::read_peer_book(&AppHost::of(app), vault_id)
}

/// The linked devices a new member can copy the vault from besides the coordinator.
pub(crate) async fn bootstrap_members<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
    coordinator_device_id: &str,
) -> Result<Vec<BundleSource>, String> {
    replica::peers::bootstrap_members(&AppHost::of(app), vault_id, coordinator_device_id).await
}

/// What starts the service in the app: the vault commit notice and the Tauri async runtime.
fn service_start() -> ServiceStart {
    ServiceStart {
        commits: crate::db::vault_commits().clone(),
        spawner: tauri::async_runtime::handle().inner().clone(),
    }
}

/// Registers the service state as the runtime a headless pass hands its work to, and starts
/// the service. Runs once at startup, before the frontend opens the vault.
pub(crate) fn setup<R: Runtime>(app: &AppHandle<R>) {
    replica::setup(
        &AppHost::of(app),
        app.state::<SyncRuntime>().inner(),
        &service_start(),
    );
}

/// The hub the coordinator listener routes sync requests to, serving the active vault.
#[cfg(desktop)]
pub(crate) fn sync_hub<R: Runtime>(app: &AppHandle<R>) -> SyncHub {
    replica::sync_hub(&AppHost::of(app), app.state::<SyncRuntime>().inner())
}

/// The hub a member listener routes admitted sync requests to, serving the active vault.
pub(crate) fn member_hub<R: Runtime>(app: &AppHandle<R>) -> SyncHub {
    replica::member_hub(&AppHost::of(app), app.state::<SyncRuntime>().inner())
}

/// Stops the service before a vault transition, letting a running pass finish first.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    replica::stop_for_vault_handoff(&AppHost::of(app), app.state::<SyncRuntime>().inner()).await
}

/// Restarts the service after a vault transition.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    replica::resume_after_vault_handoff(
        &AppHost::of(app),
        app.state::<SyncRuntime>().inner(),
        &service_start(),
    )
}

/// Seals the enrollment of a device the coordinator just linked by running a pass of the app
/// service and waiting for it, which reconciles linked devices with the space.
#[cfg(desktop)]
pub(crate) fn enrollment_sealer<R: Runtime>(
    app: &AppHandle<R>,
) -> crate::vault::handoff::transport::EnrollmentSealer {
    let runtime = app.state::<SyncRuntime>().inner().clone();
    Arc::new(move || {
        let runtime = runtime.clone();
        Box::pin(async move { replica::seal_enrollment(&runtime).await })
    })
}

/// Runs one pass for the app config directory `config_dir` without a Tauri runtime. When the
/// app service of this process is running, it runs the pass instead.
#[cfg(mobile)]
pub(crate) async fn headless_pass(config_dir: PathBuf) -> Result<HeadlessPass, String> {
    let open = async move { blocking(move || super::host::HeadlessHost::open(&config_dir)).await };
    replica::headless_pass(open).await
}

#[cfg(all(test, desktop))]
mod headless_tests;
