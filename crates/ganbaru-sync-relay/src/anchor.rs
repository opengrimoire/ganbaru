//! Anchor mode: a headless enrolled full replica that serves every device of one vault and takes
//! the anchor role, so the space keeps moving while every personal device sleeps.
//!
//! The server holds its own device, admin, and writer keys and the space's epoch keys, so unlike
//! relay mode it reads the vault. It never receives an admin key of another device or the person
//! key, and it dials nobody. An admin device enrolls it through the reversed flow in
//! [`routes`]: the operator prints a setup code here, the device proves it, proposes the
//! enrollment, and uploads a snapshot the server activates.

pub mod activation;
pub mod host;
pub mod routes;
pub mod state;

#[cfg(test)]
mod tests;

use crate::config::{AnchorBackupConfig, AnchorConfig, AnchorSetupConfig};
use crate::lock_data_dir;
use crate::server::unix_time_ms;
use crate::setup::render_qr;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::pairing::{PairingManager, certificate_fingerprint, random_token};
use ganbaru_handoff::protocol::anchor::{ANCHOR_SETUP_LIFETIME, AnchorSetupCode};
use ganbaru_handoff::protocol::relay::{RelaySetupCode, RelaySetupToken};
use ganbaru_sync_replica::access::vault_connection_hook;
use ganbaru_sync_replica::backup::seal_keys::backup_seal_keys;
use ganbaru_sync_replica::backup::{ArchiveGate, create_encrypted_backup};
use ganbaru_sync_replica::peers::admission::MemberAdmission;
use ganbaru_sync_replica::serve::bundles::MemberBundles;
use ganbaru_sync_replica::serve::limits::ConnectionLimits;
use ganbaru_sync_replica::serve::{
    CoordinatorListener, MemberAdmissionSource, ServerServices, serve_server,
};
use ganbaru_sync_replica::service::{
    self, ServiceStart, SyncRuntime, member_admission_source, server_hub,
};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Notify, oneshot};

use host::AnchorHost;
use routes::{AnchorRoutes, SetupRoute};
use state::{Activation, DataPaths, KeyFiles};

/// The version this binary reports in a setup conversation.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn prepare(paths: &DataPaths) -> Result<(), String> {
    for directory in [paths.root().to_path_buf(), paths.config()] {
        ganbaru_atomic_files::create_private_directories(&directory)
            .map_err(|error| format!("create {}: {error}", directory.display()))?;
    }
    Ok(())
}

/// The server's stable device id, created on first use.
fn load_device_id(paths: &DataPaths) -> Result<String, String> {
    let path = paths.device_id();
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text.trim().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let device_id = random_token("server")?;
            ganbaru_atomic_files::write_file_atomically(
                &path,
                device_id.as_bytes(),
                ganbaru_atomic_files::FileAccess::OwnerOnly,
            )
            .map_err(|error| format!("write {}: {error}", path.display()))?;
            Ok(device_id)
        }
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

/// Opens the device identity and publishes its certificate fingerprint. The caller holds the
/// data directory lock.
fn open_identity(paths: &DataPaths) -> Result<(String, PairingManager, String), String> {
    prepare(paths)?;
    let device_id = load_device_id(paths)?;
    let pairing = PairingManager::default();
    pairing.initialize(paths.config(), device_id.clone())?;
    let fingerprint = certificate_fingerprint(pairing.certificate()?.as_ref());
    state::store_fingerprint(paths, &fingerprint)?;
    Ok((device_id, pairing, fingerprint))
}

/// Admission from the selected log, with the anchor's own forward requests answered.
fn server_admission(host: &AnchorHost) -> MemberAdmissionSource {
    let source = member_admission_source(host);
    Arc::new(move || {
        let source = source.clone();
        Box::pin(async move { Ok(source().await?.map(MemberAdmission::answering_forwards)) })
    })
}

/// Runs an anchor until the process stops.
pub async fn run(config: AnchorConfig) -> Result<(), String> {
    let listener = TcpListener::bind(config.listen)
        .await
        .map_err(|error| format!("listen on {}: {error}", config.listen))?;
    let (_stop, shutdown) = oneshot::channel();
    serve_on(listener, &config.data_dir, shutdown).await
}

/// Serves the anchor in `data_dir` on `listener` until `shutdown`.
pub async fn serve_on(
    listener: TcpListener,
    data_dir: &Path,
    shutdown: oneshot::Receiver<()>,
) -> Result<(), String> {
    let paths = DataPaths::new(data_dir.to_path_buf());
    ganbaru_atomic_files::create_private_directories(paths.root())
        .map_err(|error| format!("create {}: {error}", paths.root().display()))?;
    let _lock = lock_data_dir(paths.root())?;
    let (device_id, pairing, fingerprint) = open_identity(&paths)?;
    routes::clear_incoming(&paths)?;

    let commits = Arc::new(Notify::new());
    let pools = DatabasePoolRegistry::default();
    pools.set_connection_hook(Some(vault_connection_hook(commits.clone())));
    let host = AnchorHost::new(paths, device_id, pairing.clone(), pools);
    let runtime = SyncRuntime::default();
    service::setup(
        &host,
        &runtime,
        &ServiceStart {
            commits: commits.clone(),
            spawner: tokio::runtime::Handle::current(),
        },
    );
    let hub = server_hub(&host, &runtime);
    let route = SetupRoute {
        host: host.clone(),
        hub: hub.clone(),
        commits,
        version: VERSION,
        upload: tokio::sync::Mutex::new(()),
    };
    let bundles = MemberBundles::new(activation::snapshot_preparer(
        host.clone(),
        pairing.clone(),
        VERSION,
    ));
    let services = ServerServices {
        hub,
        admission: server_admission(&host),
        routes: AnchorRoutes::new(Arc::new(route), bundles),
    };
    let address = listener
        .local_addr()
        .map_err(|error| format!("read the listen address: {error}"))?;
    println!("Ganbaru anchor listening on {address} with certificate fingerprint {fingerprint}");
    serve_server(
        CoordinatorListener::from(listener),
        ConnectionLimits::default(),
        pairing,
        services,
        shutdown,
    )
    .await;
    Ok(())
}

/// Prints a fresh setup secret, which replaces any earlier one. It works while the server runs,
/// which reads the secret from the data directory for each proof.
pub fn setup(config: &AnchorSetupConfig) -> Result<String, String> {
    let paths = DataPaths::new(config.data_dir.clone());
    let fingerprint = match state::load_fingerprint(&paths)? {
        Some(fingerprint) => fingerprint,
        None => {
            ganbaru_atomic_files::create_private_directories(paths.root())
                .map_err(|error| format!("create {}: {error}", paths.root().display()))?;
            let _lock = lock_data_dir(paths.root())?;
            let (_, _, fingerprint) = open_identity(&paths)?;
            state::parse_fingerprint(fingerprint.as_bytes())?
        }
    };
    let token = RelaySetupToken::generate()?;
    let lifetime = i64::try_from(ANCHOR_SETUP_LIFETIME.as_millis())
        .map_err(|_| "setup lifetime is too large".to_string())?;
    let code = AnchorSetupCode(RelaySetupCode {
        endpoints: config.endpoints.clone(),
        certificate_fingerprint: fingerprint,
        token,
    });
    // Encoding first means an unusable code never replaces a working secret.
    let text = describe_setup(&code)?;
    state::store_setup_secret(
        &paths,
        &code.0.token,
        unix_time_ms().saturating_add(lifetime),
    )?;
    Ok(text)
}

fn describe_setup(code: &AnchorSetupCode) -> Result<String, String> {
    let matrix = ganbaru_handoff::protocol::qr_matrix_for_bytes(&code.to_bytes()?)?;
    let minutes = ANCHOR_SETUP_LIFETIME.as_secs() / 60;
    let mut text = render_qr(&matrix);
    let _ = writeln!(text);
    let _ = writeln!(text, "Setup code: {}", code.to_text()?);
    let _ = writeln!(
        text,
        "The code works once, for {minutes} minutes, and the newest code replaces older ones. Keep it private until it is used."
    );
    Ok(text)
}

/// Writes an encrypted backup of the stopped anchor's vault to the path in `config`, and returns
/// the text to print. The file opens with the recovery kit on any new device.
pub async fn backup(config: AnchorBackupConfig) -> Result<String, String> {
    let paths = DataPaths::new(config.data_dir.clone());
    if !paths.root().is_dir() {
        return Err(format!("no anchor data in {}", paths.root().display()));
    }
    let _lock = lock_data_dir(paths.root())?;
    let active =
        Activation::load(&paths)?.ok_or_else(|| "this server runs no vault yet".to_string())?;
    let pools = DatabasePoolRegistry::default();
    let pool = pools.connect_path(paths.database()).await?;
    let keys = async {
        let mut conn = pool
            .acquire()
            .await
            .map_err(|error| format!("open the vault: {error}"))?;
        backup_seal_keys(
            &mut conn,
            Arc::new(KeyFiles::new(paths.keys())),
            &active.vault_id,
        )
        .await
    }
    .await;
    pools.close_all().await?;
    let keys = keys?.ok_or_else(|| "this vault has no recovery kit".to_string())?;
    ganbaru_atomic_files::create_private_directories(&paths.backup_staging())
        .map_err(|error| format!("create backup staging: {error}"))?;
    let bytes = create_encrypted_backup(
        &ArchiveGate::process(),
        &paths.vault(),
        &paths.backup_staging(),
        &keys,
        &config.out,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "Wrote {bytes} bytes to {}. The file opens with the recovery kit.\n",
        config.out.display()
    ))
}
