//! The sync server of the active vault's space for Settings: adding a relay or an anchor server
//! from its setup code, its status, and removing it. Claiming and removing the relay space run
//! through the replica relay API; the server configuration entry runs through the service as an
//! authority action, and an anchor server is set up in `anchor_server`.

use ganbaru_handoff::protocol::relay::RELAY_PROTOCOL_VERSION;
use ganbaru_sync::local;
use ganbaru_sync_contracts::ServerMode;
use ganbaru_sync_contracts::ids::hex;
use ganbaru_sync_replica::relay::{RelayError, RelayProblem, RelayTarget, probe};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use super::authority::{authorization, own_is_admin};
use super::host::{AppHost, SyncHost};
use super::service::RELAY_LOOPBACK;

#[cfg(desktop)]
use super::anchor_server::add_anchor_server;
#[cfg(desktop)]
use super::commands::run_authority;
#[cfg(desktop)]
use super::service::{AuthorityAction, SyncRuntime, blocking};
#[cfg(desktop)]
use ganbaru_handoff::protocol::anchor::{ANCHOR_SETUP_TEXT_PREFIX, AnchorSetupCode};
#[cfg(desktop)]
use ganbaru_handoff::protocol::relay::{RELAY_SETUP_TEXT_PREFIX, RelaySetupCode};
#[cfg(desktop)]
use ganbaru_sync::Engine;
#[cfg(desktop)]
use ganbaru_sync_contracts::{AdminKeyPair, Server, SpaceId};
#[cfg(desktop)]
use ganbaru_sync_replica::anchor_link::ServerTarget;
#[cfg(desktop)]
use ganbaru_sync_replica::anchor_setup::AnchorSetupError;
#[cfg(desktop)]
use ganbaru_sync_replica::authority::read_admin_key;
#[cfg(desktop)]
use ganbaru_sync_replica::keys::SpaceKeys;
#[cfg(desktop)]
use ganbaru_sync_replica::relay::{claim_space, relay_admin_set, remove_space};
#[cfg(desktop)]
use tauri::State;

/// Why a server command failed. The interface shows each code as localized text; details stay
/// in the log.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
#[cfg_attr(
    mobile,
    allow(
        dead_code,
        reason = "Android only reads the server; the add and remove failures are desktop paths"
    )
)]
pub enum SyncServerError {
    /// The user did not confirm that the server cryptography has had no external review.
    Unacknowledged,
    /// The text is not a server setup code.
    InvalidCode,
    /// The anchor server refused the setup secret, which is wrong, used, or expired.
    SetupRefused,
    /// The anchor server speaks an older protocol than this device.
    ServerNeedsUpdate,
    /// The anchor server speaks a newer protocol than this device.
    DeviceNeedsUpdate,
    /// The server anchors the vault, so another device takes the anchor before it goes.
    ServerIsAnchor,
    /// The active vault has not joined a sync space.
    NotSyncing,
    /// This device does not hold a live admin key of the space.
    NotAdmin,
    /// The space already names a server; remove it first.
    ServerExists,
    /// The space names no server.
    NoServer,
    /// This device holds no key epoch of the space yet.
    NoKeys,
    /// The relay refused or could not be reached.
    Relay { problem: RelayProblem },
    /// A local fault on this device.
    Failed,
}

impl From<String> for SyncServerError {
    fn from(detail: String) -> Self {
        eprintln!("sync server: {detail}");
        Self::Failed
    }
}

#[cfg(desktop)]
impl From<AnchorSetupError> for SyncServerError {
    fn from(error: AnchorSetupError) -> Self {
        match error {
            AnchorSetupError::ServerNeedsUpdate => Self::ServerNeedsUpdate,
            AnchorSetupError::DeviceNeedsUpdate => Self::DeviceNeedsUpdate,
            AnchorSetupError::Refused => Self::SetupRefused,
            AnchorSetupError::Other(detail) => detail.into(),
        }
    }
}

impl From<RelayError> for SyncServerError {
    fn from(error: RelayError) -> Self {
        eprintln!("sync server relay: {}", error.detail);
        Self::Relay {
            problem: error.problem,
        }
    }
}

/// What a server is for, in wire names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncServerMode {
    Relay,
    Anchor,
}

impl From<ServerMode> for SyncServerMode {
    fn from(mode: ServerMode) -> Self {
        match mode {
            ServerMode::Relay => Self::Relay,
            ServerMode::Anchor => Self::Anchor,
        }
    }
}

/// Whether the relay answered a probe just now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncServerProbe {
    /// The protocol version the relay answered with, `None` when it did not answer.
    pub protocol_version: Option<u16>,
    /// Whether this build speaks that protocol version.
    pub compatible: bool,
    /// Why the probe failed, `None` when the relay answered.
    pub problem: Option<RelayProblem>,
}

impl SyncServerProbe {
    /// The probe result of `outcome`.
    fn of(outcome: Result<u16, RelayError>) -> Self {
        match outcome {
            Ok(version) => Self {
                protocol_version: Some(version),
                compatible: version == RELAY_PROTOCOL_VERSION,
                problem: None,
            },
            Err(error) => {
                eprintln!("sync server probe: {}", error.detail);
                Self {
                    protocol_version: None,
                    compatible: false,
                    problem: Some(error.problem),
                }
            }
        }
    }
}

/// The server the space's selected configuration names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncServerView {
    pub mode: SyncServerMode,
    /// Addresses as `host:port`.
    pub endpoints: Vec<String>,
    /// SHA-256 of the server's DER certificate in lowercase hexadecimal.
    pub certificate_fingerprint: String,
    /// Whether this device may remove the server.
    pub can_manage: bool,
    /// A probe of a relay, `None` for an anchor server.
    pub probe: Option<SyncServerProbe>,
}

/// What removing the server did at the relay: the configuration removal always waits for the
/// anchor, while the relay space removal is best effort.
#[cfg(desktop)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncServerRemoval {
    /// Whether the relay dropped the space and what it stored for it.
    pub relay_cleared: bool,
    /// Why the relay did not drop the space, `None` when it did or the server is an anchor.
    pub problem: Option<RelayProblem>,
}

/// How far adding a server came.
#[cfg(desktop)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncServerAdded {
    /// The relay is claimed; devices follow its configuration once the anchor sequences it.
    Proposed,
    /// The anchor server waits for the current anchor; repeat the call with the same code.
    WaitingForAnchor,
    /// The anchor server runs the vault.
    Active,
}

/// A pasted server setup code.
#[cfg(desktop)]
enum SetupCode {
    Relay(RelaySetupCode),
    Anchor(AnchorSetupCode),
}

/// Parses a pasted server setup code by the kind its prefix names.
#[cfg(desktop)]
fn parse_setup_code(text: &str) -> Result<SetupCode, SyncServerError> {
    let text = text.trim();
    let parsed = if text.starts_with(RELAY_SETUP_TEXT_PREFIX) {
        RelaySetupCode::from_text(text).map(SetupCode::Relay)
    } else if text.starts_with(ANCHOR_SETUP_TEXT_PREFIX) {
        AnchorSetupCode::from_text(text).map(SetupCode::Anchor)
    } else {
        return Err(SyncServerError::InvalidCode);
    };
    parsed.map_err(|error| {
        eprintln!("sync server setup code: {error}");
        SyncServerError::InvalidCode
    })
}

/// The space id and admin key pair of a live admin desktop.
#[cfg(desktop)]
pub(super) struct AdminContext {
    space: SpaceId,
    admin: AdminKeyPair,
}

/// The selected authorization, the space, and this desktop's admin key, refusing a device that
/// is not a live admin.
#[cfg(desktop)]
pub(super) async fn admin_context<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(ganbaru_sync::AuthorizationView, AdminContext), SyncServerError> {
    let view = authorization(app)
        .await?
        .ok_or(SyncServerError::NotSyncing)?;
    let vault_id = AppHost::of(app).active_vault_id()?;
    let own = super::devices::stable_device_id(app)?;
    if !own_is_admin(app, &vault_id, &view, &own).await? {
        return Err(SyncServerError::NotAdmin);
    }
    let keys = AppHost::of(app).key_store()?;
    let admin_vault = vault_id.clone();
    let admin = blocking(move || read_admin_key(keys.as_ref(), &admin_vault))
        .await?
        .ok_or(SyncServerError::NotAdmin)?;
    let space = space_id(app).await?;
    Ok((view, AdminContext { space, admin }))
}

/// A connection to the active vault's database with its sync space.
async fn space_connection<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<
    (
        sqlx::pool::PoolConnection<sqlx::Sqlite>,
        ganbaru_sync::SpaceContext,
    ),
    SyncServerError,
> {
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let vault_id = AppHost::of(app).active_vault_id()?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let ctx = local::space_context(&mut conn, &vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .ok_or(SyncServerError::NotSyncing)?;
    Ok((conn, ctx))
}

/// The active vault's space id.
async fn space_id<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<ganbaru_sync_contracts::SpaceId, SyncServerError> {
    Ok(space_connection(app).await?.1.space)
}

/// The newest key epoch this installation opened, which the relay space is claimed at.
#[cfg(desktop)]
async fn newest_space_key<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<ganbaru_sync_contracts::SpaceKey, SyncServerError> {
    let vault_id = AppHost::of(app).active_vault_id()?;
    let keys = AppHost::of(app).key_store()?;
    let holder = blocking(move || SpaceKeys::load(keys.as_ref(), &vault_id)).await?;
    let (mut conn, ctx) = space_connection(app).await?;
    let newest = holder
        .open_epochs(&Engine::vault(), &mut conn, &ctx)
        .await?
        .ok_or(SyncServerError::NoKeys)?;
    holder.epoch_key(newest).ok_or(SyncServerError::NoKeys)
}

/// On a live admin desktop, adds the server a pasted setup code names to the active vault.
/// `acknowledged_unreviewed` confirms the user accepted that the server cryptography has had no
/// external review. `take_anchor` asks an anchor server to take the anchor once it can; a relay
/// ignores it.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_add_server<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
    code: String,
    acknowledged_unreviewed: bool,
    take_anchor: bool,
) -> Result<SyncServerAdded, SyncServerError> {
    if !acknowledged_unreviewed {
        return Err(SyncServerError::Unacknowledged);
    }
    match parse_setup_code(&code)? {
        SetupCode::Relay(code) => add_relay(&app, &runtime, code).await,
        SetupCode::Anchor(code) => add_anchor_server(&app, &runtime, &code, take_anchor).await,
    }
}

/// Claims the relay `code` names for the active vault's space and proposes the server
/// configuration that names it, which every device follows once the anchor sequences it. A
/// failed proposal removes the claimed relay space again.
#[cfg(desktop)]
async fn add_relay<R: Runtime>(
    app: &AppHandle<R>,
    runtime: &SyncRuntime,
    code: RelaySetupCode,
) -> Result<SyncServerAdded, SyncServerError> {
    let (view, context) = admin_context(app).await?;
    if view.server.is_some() {
        return Err(SyncServerError::ServerExists);
    }
    let first = newest_space_key(app).await?;
    let admins: Vec<_> = relay_admin_set(&view).into_iter().collect();
    let target = RelayTarget {
        endpoints: code.endpoints.clone(),
        certificate_fingerprint: code.certificate_fingerprint,
    };
    claim_space(
        &target,
        context.space,
        RELAY_LOOPBACK,
        &code.token,
        &first,
        &context.admin,
        &admins,
    )
    .await?;
    let server = Server {
        mode: ServerMode::Relay,
        endpoints: code.endpoints,
        certificate_fingerprint: code.certificate_fingerprint,
    };
    if let Err(error) = run_authority(runtime, AuthorityAction::SetServer(Some(server))).await {
        if let Err(cleanup) =
            remove_space(&target, context.space, RELAY_LOOPBACK, &context.admin).await
        {
            eprintln!("sync server relay cleanup: {}", cleanup.detail);
        }
        return Err(error.into());
    }
    runtime.sync_now();
    Ok(SyncServerAdded::Proposed)
}

/// Whether the server the configuration names is the vault's anchor. Devices stop dialing a
/// removed server, so a vault it anchors would lose its anchor with it.
#[cfg(desktop)]
fn server_anchors(view: &ganbaru_sync::AuthorizationView) -> bool {
    ServerTarget::of(view)
        .is_some_and(|target| target.device_id(view) == Some(view.anchor_device_id.as_str()))
}

/// On a live admin desktop, proposes removing the space's server configuration, then asks a
/// relay to drop the space and everything it stores for it. The relay removal is best effort:
/// an unreachable relay keeps its copy of the sealed records, which it cannot open. An anchor
/// server that anchors the vault is refused until another device takes the anchor; once
/// removed from the configuration it stays enrolled until it is removed from the device list.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_remove_server<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
) -> Result<SyncServerRemoval, SyncServerError> {
    let (view, context) = admin_context(&app).await?;
    let target = match &view.server {
        None => return Err(SyncServerError::NoServer),
        Some(_) if server_anchors(&view) => return Err(SyncServerError::ServerIsAnchor),
        Some(_) => RelayTarget::of(&view),
    };
    run_authority(&runtime, AuthorityAction::SetServer(None)).await?;
    runtime.sync_now();
    let Some(target) = target else {
        return Ok(SyncServerRemoval {
            relay_cleared: false,
            problem: None,
        });
    };
    Ok(
        match remove_space(&target, context.space, RELAY_LOOPBACK, &context.admin).await {
            Ok(()) => SyncServerRemoval {
                relay_cleared: true,
                problem: None,
            },
            Err(error) => {
                eprintln!("sync server relay removal: {}", error.detail);
                SyncServerRemoval {
                    relay_cleared: false,
                    problem: Some(error.problem),
                }
            }
        },
    )
}

/// The server the active vault's space names, with a probe of a relay, or `None` before the
/// vault joined a sync space or while it names no server.
#[tauri::command]
pub async fn sync_server_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<SyncServerView>, SyncServerError> {
    let Some(view) = authorization(&app).await? else {
        return Ok(None);
    };
    let Some(server) = &view.server else {
        return Ok(None);
    };
    let vault_id = AppHost::of(&app).active_vault_id()?;
    let own = super::devices::stable_device_id(&app)?;
    let can_manage = own_is_admin(&app, &vault_id, &view, &own).await?;
    let probe = match RelayTarget::of(&view) {
        Some(target) => {
            let space = space_id(&app).await?;
            Some(SyncServerProbe::of(
                probe(&target, space, RELAY_LOOPBACK).await,
            ))
        }
        None => None,
    };
    Ok(Some(SyncServerView {
        mode: server.mode.into(),
        endpoints: server.endpoints.clone(),
        certificate_fingerprint: hex(&server.certificate_fingerprint),
        can_manage,
        probe,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(desktop)]
    use ganbaru_handoff::protocol::relay::{RELAY_SETUP_TOKEN_BYTES, RelaySetupToken};

    #[test]
    fn server_errors_reach_the_frontend_as_codes() {
        let wire = serde_json::to_value(SyncServerError::ServerNeedsUpdate).expect("serialize");
        assert_eq!(wire, serde_json::json!({ "code": "serverNeedsUpdate" }));
        let wire = serde_json::to_value(SyncServerError::Relay {
            problem: RelayProblem::SetupCodeRefused,
        })
        .expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({ "code": "relay", "problem": "setupCodeRefused" })
        );
    }

    #[test]
    fn probes_report_the_relay_version_and_compatibility() {
        let answered = SyncServerProbe::of(Ok(RELAY_PROTOCOL_VERSION));
        assert_eq!(answered.protocol_version, Some(RELAY_PROTOCOL_VERSION));
        assert!(answered.compatible);
        assert_eq!(answered.problem, None);
        let newer = SyncServerProbe::of(Ok(RELAY_PROTOCOL_VERSION + 1));
        assert!(!newer.compatible);
        let silent = SyncServerProbe::of(Err(RelayError::new(
            RelayProblem::Unreachable,
            "no address answered",
        )));
        assert_eq!(silent.protocol_version, None);
        assert_eq!(silent.problem, Some(RelayProblem::Unreachable));
        let wire = serde_json::to_value(&silent).expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({
                "protocolVersion": null,
                "compatible": false,
                "problem": "unreachable",
            })
        );
    }

    #[cfg(desktop)]
    #[test]
    fn setup_codes_parse_by_the_kind_their_prefix_names() {
        let anchor = AnchorSetupCode(RelaySetupCode {
            endpoints: vec!["server.example:7443".to_string()],
            certificate_fingerprint: [7; 32],
            token: RelaySetupToken::from_bytes([9; RELAY_SETUP_TOKEN_BYTES]),
        });
        let text = format!("  {}\n", anchor.to_text().expect("encode"));
        assert!(matches!(parse_setup_code(&text), Ok(SetupCode::Anchor(_))));
        for text in [
            "  hello".to_string(),
            format!("{ANCHOR_SETUP_TEXT_PREFIX}not-a-code"),
            format!("{RELAY_SETUP_TEXT_PREFIX}not-a-code"),
        ] {
            assert_eq!(
                parse_setup_code(&text).err(),
                Some(SyncServerError::InvalidCode)
            );
        }
    }

    #[cfg(desktop)]
    #[test]
    fn setup_failures_reach_the_frontend_as_codes() {
        assert_eq!(
            SyncServerError::from(AnchorSetupError::ServerNeedsUpdate),
            SyncServerError::ServerNeedsUpdate
        );
        assert_eq!(
            SyncServerError::from(AnchorSetupError::Refused),
            SyncServerError::SetupRefused
        );
        assert_eq!(
            SyncServerError::from(AnchorSetupError::Other("reset".to_string())),
            SyncServerError::Failed
        );
    }
}
