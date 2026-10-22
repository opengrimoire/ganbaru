//! Explicit Linux firewall access for the private-LAN handoff coordinator.

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::dial::{ipv4_lan_block, ipv4_mask};
use crate::protocol::{COORDINATOR_PORT, DEVELOPMENT_COORDINATOR_PORT};

const STATE_FILE: &str = "vault-handoff-network-access.json";
const STATE_SCHEMA_VERSION: u32 = 2;
const RULE_COMMENT: &str = "Ganbaru device linking";
const MAX_AUTHORIZED_SCOPES: usize = 32;
/// Networks remembered per scope. Beyond it the oldest is forgotten and asks again.
const MAX_ALLOWED_NETWORKS: usize = 16;
/// `RTF_UP` and `RTF_GATEWAY` route flags in `/proc/net/route`.
const ROUTE_UP: u16 = 0x1;
const ROUTE_GATEWAY: u16 = 0x2;
/// `ATF_COM` flag of a resolved neighbor in `/proc/net/arp`.
const NEIGHBOR_COMPLETE: u32 = 0x2;
/// Installed path of the packaged app, the only build that hands rules to the helper.
const PACKAGED_APP_PATH: &str = "/usr/bin/ganbaru";
/// Installed path of the root-run helper that the polkit policy names.
const PACKAGED_HELPER_PATH: &str = "/usr/lib/ganbaru/ganbaru-linking-firewall";
const PRIVILEGED_HELPER_ARGUMENT: &str = "--manage-linking-firewall";
const REVOKE_ALL_OPERATION: &str = "revoke-all";
/// Fixed locations of the system commands the app and the helper run. Each must be a regular
/// file only root can replace; see [`command_path`].
const PKEXEC_PATHS: &[&str] = &["/usr/bin/pkexec"];
const UFW_PATHS: &[&str] = &["/usr/sbin/ufw", "/usr/bin/ufw"];
const FIREWALL_CMD_PATHS: &[&str] = &["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum FirewallKind {
    Ufw,
    Firewalld,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthorizedScope {
    firewall: FirewallKind,
    interface: String,
    local_address: Ipv4Addr,
    source_network: String,
    port: u16,
    #[serde(default)]
    zone: Option<String>,
}

/// A scope whose firewall rule is in place, and the networks the user allowed it on.
///
/// A rule matches addresses, not networks, so another network that hands out the same
/// interface, address, and subnet would reach the coordinator through it. The coordinator
/// therefore admits LAN peers only on networks recorded here; see [`allowed_on`].
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantedScope {
    #[serde(flatten)]
    rule: AuthorizedScope,
    /// Identities from [`current_network`], oldest first.
    #[serde(default)]
    networks: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredNetworkAccess {
    schema_version: u32,
    /// Scopes whose firewall rules are in place.
    scopes: Vec<GrantedScope>,
    /// Scopes recorded before their rules were applied. A rule that failed partway may exist,
    /// so revoking removes these too, and granting again applies them again.
    #[serde(default)]
    pending: Vec<AuthorizedScope>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkAccessState {
    NotRequired,
    AuthorizationRequired,
    Granted,
    ManualActionRequired,
    /// The rules for this network's addresses exist, but the user allowed them on another
    /// network. Confirming needs no administrator authorization.
    NetworkConfirmationRequired,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAccessStatus {
    state: NetworkAccessState,
}

pub fn not_required() -> NetworkAccessStatus {
    NetworkAccessStatus {
        state: NetworkAccessState::NotRequired,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LanScope {
    interface: String,
    local_address: Ipv4Addr,
    source_network: String,
}

pub fn status(config_dir: &Path, local_address: Ipv4Addr, port: u16) -> NetworkAccessStatus {
    let Ok(scope) = current_lan_scope(local_address) else {
        return NetworkAccessStatus {
            state: NetworkAccessState::ManualActionRequired,
        };
    };
    let firewalls = active_firewalls(&scope, port);
    if firewalls.is_empty() {
        return NetworkAccessStatus {
            state: NetworkAccessState::NotRequired,
        };
    }
    let Ok(stored) = read_state(config_dir) else {
        return NetworkAccessStatus {
            state: NetworkAccessState::ManualActionRequired,
        };
    };
    let network = current_network(&scope.interface);
    NetworkAccessStatus {
        state: access_state(&stored, &firewalls, network.as_deref(), || {
            can_request_authorization(&firewalls)
        }),
    }
}

/// Whether the coordinator may accept LAN peers that reach it on `local_address` and `port`
/// now. It refuses them only where [`status`] asks the user to confirm the current network, so
/// a refusal always comes with an action that lifts it. Firewall scopes are IPv4 only, so an
/// IPv6 local address has no allowed networks to confirm and admits.
pub fn admits_lan_peers(config_dir: &Path, local_address: IpAddr, port: u16) -> bool {
    let IpAddr::V4(local_address) = local_address else {
        return true;
    };
    status(config_dir, local_address, port).state != NetworkAccessState::NetworkConfirmationRequired
}

/// The access state for the active firewall `candidates` on the network identified by
/// `network`. Authorization is only checked when a rule is missing, because confirming a
/// network with every rule in place needs none.
fn access_state(
    stored: &StoredNetworkAccess,
    candidates: &[AuthorizedScope],
    network: Option<&str>,
    can_request_authorization: impl FnOnce() -> bool,
) -> NetworkAccessState {
    let granted: Option<Vec<&GrantedScope>> = candidates
        .iter()
        .map(|candidate| stored.scopes.iter().find(|saved| &saved.rule == candidate))
        .collect();
    match granted {
        Some(granted) if granted.iter().all(|saved| allowed_on(saved, network)) => {
            NetworkAccessState::Granted
        }
        Some(_) => NetworkAccessState::NetworkConfirmationRequired,
        None if can_request_authorization() => NetworkAccessState::AuthorizationRequired,
        None => NetworkAccessState::ManualActionRequired,
    }
}

/// Whether the user allowed `scope` on the network identified by `network`. A network without
/// an identity cannot be told apart from an allowed one, so it counts as allowed; a scope with
/// no recorded networks is allowed on no network that has one.
fn allowed_on(scope: &GrantedScope, network: Option<&str>) -> bool {
    network.is_none_or(|network| scope.networks.iter().any(|known| known == network))
}

pub fn grant(
    config_dir: &Path,
    local_address: Ipv4Addr,
    port: u16,
) -> Result<NetworkAccessStatus, String> {
    let scope = current_lan_scope(local_address)?;
    let firewalls = active_firewalls(&scope, port);
    if firewalls.is_empty() {
        return Ok(NetworkAccessStatus {
            state: NetworkAccessState::NotRequired,
        });
    }
    let mut stored = read_state(config_dir)?;
    let missing: Vec<AuthorizedScope> = firewalls
        .iter()
        .filter(|candidate| !stored.scopes.iter().any(|saved| &saved.rule == *candidate))
        .cloned()
        .collect();
    if !missing.is_empty() && !can_request_authorization(&missing) {
        return Err("automatic Linux firewall authorization is unavailable".to_string());
    }
    let network = current_network(&scope.interface);
    grant_scopes(
        &mut stored,
        firewalls,
        network.as_deref(),
        |state| persist_state(config_dir, state),
        |scopes| apply_scopes(scopes, true),
    )?;
    Ok(NetworkAccessStatus {
        state: NetworkAccessState::Granted,
    })
}

/// Applies the rules of `candidates` that `stored` does not hold yet and records `network` as
/// allowed for every candidate.
///
/// Capacity is checked before any rule changes. The missing scopes are persisted as pending,
/// applied in one authorized call so the flow asks for the administrator password once, and
/// moved to the granted scopes after, so a failure or crash between the two never leaves a root
/// rule without a record that revoking can find. A candidate already in place only gains the
/// network, with no firewall change.
fn grant_scopes(
    stored: &mut StoredNetworkAccess,
    candidates: Vec<AuthorizedScope>,
    network: Option<&str>,
    mut persist: impl FnMut(&StoredNetworkAccess) -> Result<(), String>,
    apply: impl FnOnce(&[AuthorizedScope]) -> Result<(), String>,
) -> Result<(), String> {
    let (present, missing): (Vec<AuthorizedScope>, Vec<AuthorizedScope>) = candidates
        .into_iter()
        .partition(|candidate| stored.scopes.iter().any(|saved| &saved.rule == candidate));
    let new_records: Vec<AuthorizedScope> = missing
        .iter()
        .filter(|candidate| !stored.pending.contains(candidate))
        .cloned()
        .collect();
    if stored.scopes.len() + stored.pending.len() + new_records.len() > MAX_AUTHORIZED_SCOPES {
        return Err("too many saved Linux network access scopes".to_string());
    }
    let mut confirmed = false;
    for saved in stored
        .scopes
        .iter_mut()
        .filter(|saved| present.contains(&saved.rule))
    {
        confirmed |= allow_network(saved, network);
    }
    if confirmed {
        persist(stored)?;
    }
    if missing.is_empty() {
        return Ok(());
    }
    if !new_records.is_empty() {
        stored.pending.extend(new_records);
        persist(stored)?;
    }
    apply(&missing)?;
    stored.pending.retain(|pending| !missing.contains(pending));
    for candidate in missing {
        let mut granted = GrantedScope {
            rule: candidate,
            networks: Vec::new(),
        };
        allow_network(&mut granted, network);
        stored.scopes.push(granted);
    }
    persist(stored)
}

/// Records `network` as allowed for `scope`, dropping the oldest network at the limit.
/// Returns whether the scope changed.
fn allow_network(scope: &mut GrantedScope, network: Option<&str>) -> bool {
    let Some(network) = network else {
        return false;
    };
    if scope.networks.iter().any(|known| known == network) {
        return false;
    }
    if scope.networks.len() >= MAX_ALLOWED_NETWORKS {
        scope.networks.remove(0);
    }
    scope.networks.push(network.to_string());
    true
}

pub fn revoke(config_dir: &Path) -> Result<NetworkAccessStatus, String> {
    let mut stored = read_state(config_dir)?;
    let result = revoke_scopes(&mut stored, |scopes| apply_scopes(scopes, false));
    persist_state(config_dir, &stored)?;
    result?;
    Ok(NetworkAccessStatus {
        state: NetworkAccessState::AuthorizationRequired,
    })
}

/// Removes the rules of every granted and pending scope in one authorized call. A failed call
/// keeps every scope so a later revoke retries them; removing a rule that is already gone
/// succeeds, so the retry is safe.
fn revoke_scopes(
    stored: &mut StoredNetworkAccess,
    remove: impl FnOnce(&[AuthorizedScope]) -> Result<(), String>,
) -> Result<(), String> {
    let scopes: Vec<AuthorizedScope> = stored
        .scopes
        .iter()
        .map(|saved| saved.rule.clone())
        .chain(stored.pending.iter().cloned())
        .collect();
    if scopes.is_empty() {
        return Ok(());
    }
    remove(&scopes)?;
    stored.scopes.clear();
    stored.pending.clear();
    Ok(())
}

/// The identity of the network reached through `interface`: the hardware address of its
/// default IPv4 gateway. `None` when the interface has no default gateway or the gateway has no
/// resolved neighbor entry.
fn current_network(interface: &str) -> Option<String> {
    let route = fs::read_to_string("/proc/net/route").ok()?;
    let gateway = parse_default_gateway(&route, interface)?;
    let neighbors = fs::read_to_string("/proc/net/arp").ok()?;
    parse_neighbor_hardware_address(&neighbors, gateway, interface)
}

/// The gateway of the default IPv4 route through `interface` in `/proc/net/route` contents.
fn parse_default_gateway(contents: &str, interface: &str) -> Option<Ipv4Addr> {
    contents.lines().skip(1).find_map(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 8 || fields[0] != interface || fields[1] != "00000000" {
            return None;
        }
        let flags = u16::from_str_radix(fields[3], 16).ok()?;
        if flags & (ROUTE_UP | ROUTE_GATEWAY) != ROUTE_UP | ROUTE_GATEWAY {
            return None;
        }
        let gateway = Ipv4Addr::from(u32::from_str_radix(fields[2], 16).ok()?.swap_bytes());
        (!gateway.is_unspecified()).then_some(gateway)
    })
}

/// The lowercase hardware address of a completed `/proc/net/arp` entry for `address` on
/// `interface`.
fn parse_neighbor_hardware_address(
    contents: &str,
    address: Ipv4Addr,
    interface: &str,
) -> Option<String> {
    contents.lines().skip(1).find_map(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6
            || fields[0].parse::<Ipv4Addr>().ok()? != address
            || fields[5] != interface
        {
            return None;
        }
        let flags = u32::from_str_radix(fields[2].strip_prefix("0x")?, 16).ok()?;
        let hardware = fields[3].to_ascii_lowercase();
        (flags & NEIGHBOR_COMPLETE != 0 && is_network_identity(&hardware)).then_some(hardware)
    })
}

/// Whether `value` is a nonzero hardware address in lowercase colon notation.
fn is_network_identity(value: &str) -> bool {
    let octets = value.split(':').collect::<Vec<_>>();
    octets.len() == 6
        && octets.iter().all(|octet| {
            octet.len() == 2
                && octet
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        && octets.iter().any(|octet| *octet != "00")
}

fn empty_state() -> StoredNetworkAccess {
    StoredNetworkAccess {
        schema_version: STATE_SCHEMA_VERSION,
        scopes: Vec::new(),
        pending: Vec::new(),
    }
}

fn current_lan_scope(local_address: Ipv4Addr) -> Result<LanScope, String> {
    let route = fs::read_to_string("/proc/net/route")
        .map_err(|error| format!("read Linux network route: {error}"))?;
    let (interface, mask) = parse_local_ipv4_route(&route, local_address)?;
    lan_scope(interface, local_address, mask)
}

/// The scope of a connected route, narrowed to the private block of `local_address` when the
/// route is broader, so the opened source never extends past that block.
fn lan_scope(
    interface: String,
    local_address: Ipv4Addr,
    mask: Ipv4Addr,
) -> Result<LanScope, String> {
    let (_, block_prefix) = ipv4_lan_block(local_address)
        .ok_or_else(|| "this device's address is not on a private network".to_string())?;
    let prefix = ipv4_prefix(mask)?.max(block_prefix);
    let network = Ipv4Addr::from(u32::from(local_address) & ipv4_mask(prefix));
    Ok(LanScope {
        interface,
        local_address,
        source_network: format!("{network}/{prefix}"),
    })
}

fn parse_local_ipv4_route(
    contents: &str,
    local_address: Ipv4Addr,
) -> Result<(String, Ipv4Addr), String> {
    let mut selected: Option<(String, Ipv4Addr, u32)> = None;
    for line in contents.lines().skip(1) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 8 || fields[2] != "00000000" {
            continue;
        }
        let flags = u16::from_str_radix(fields[3], 16)
            .map_err(|_| "Linux default route flags are invalid".to_string())?;
        if flags & 0x1 == 0 || !is_valid_interface(fields[0]) {
            continue;
        }
        let raw_mask = u32::from_str_radix(fields[7], 16)
            .map_err(|_| "Linux local route mask is invalid".to_string())?;
        let raw_destination = u32::from_str_radix(fields[1], 16)
            .map_err(|_| "Linux local route destination is invalid".to_string())?;
        let mask = Ipv4Addr::from(raw_mask.swap_bytes());
        let destination = Ipv4Addr::from(raw_destination.swap_bytes());
        let prefix = ipv4_prefix(mask)?;
        if prefix == 0
            || ipv4_lan_block(destination).is_none()
            || u32::from(local_address) & u32::from(mask) != u32::from(destination)
        {
            continue;
        }
        if selected
            .as_ref()
            .is_none_or(|(_, _, selected_prefix)| prefix > *selected_prefix)
        {
            selected = Some((fields[0].to_string(), mask, prefix));
        }
    }
    selected
        .map(|(interface, mask, _)| (interface, mask))
        .ok_or_else(|| "no usable Linux private network route is available".to_string())
}

fn is_valid_interface(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 15
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn ipv4_prefix(mask: Ipv4Addr) -> Result<u32, String> {
    let mask = u32::from(mask);
    let prefix = mask.leading_ones();
    if mask != u32::MAX.checked_shl(32 - prefix).unwrap_or(0) {
        return Err("Linux default route has a non-contiguous network mask".to_string());
    }
    Ok(prefix)
}

fn active_firewalls(scope: &LanScope, port: u16) -> Vec<AuthorizedScope> {
    let mut result = Vec::new();
    if is_ufw_enabled() {
        result.push(AuthorizedScope {
            firewall: FirewallKind::Ufw,
            interface: scope.interface.clone(),
            local_address: scope.local_address,
            source_network: scope.source_network.clone(),
            port,
            zone: None,
        });
    }
    if is_firewalld_running() {
        let zone = firewalld_zone(&scope.interface);
        result.push(AuthorizedScope {
            firewall: FirewallKind::Firewalld,
            interface: scope.interface.clone(),
            local_address: scope.local_address,
            source_network: scope.source_network.clone(),
            port,
            zone,
        });
    }
    result
}

fn is_ufw_enabled() -> bool {
    let Ok(contents) = fs::read_to_string("/etc/ufw/ufw.conf") else {
        return false;
    };
    contents.lines().any(|line| {
        line.split_once('=')
            .is_some_and(|(key, value)| key.trim() == "ENABLED" && value.trim() == "yes")
    })
}

fn is_firewalld_running() -> bool {
    command_path(FIREWALL_CMD_PATHS)
        .and_then(|path| Command::new(path).arg("--state").output().ok())
        .is_some_and(|output| output.status.success() && output.stdout == b"running\n")
}

fn firewalld_zone(interface: &str) -> Option<String> {
    let path = command_path(FIREWALL_CMD_PATHS)?;
    let output = Command::new(&path)
        .arg(format!("--get-zone-of-interface={interface}"))
        .output()
        .ok()?;
    let zone = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if output.status.success() && is_valid_zone(&zone) {
        return Some(zone);
    }
    let output = Command::new(path).arg("--get-default-zone").output().ok()?;
    let zone = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && is_valid_zone(&zone)).then_some(zone)
}

fn is_valid_zone(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn can_request_authorization(scopes: &[AuthorizedScope]) -> bool {
    command_path(PKEXEC_PATHS).is_some()
        && scopes.iter().all(|scope| match scope.firewall {
            FirewallKind::Ufw => command_path(UFW_PATHS).is_some(),
            FirewallKind::Firewalld => {
                command_path(FIREWALL_CMD_PATHS).is_some() && scope.zone.is_some()
            }
        })
}

/// Adds or removes the rules of `scopes`. The packaged app hands them all to the helper in one
/// authorized call; other builds apply each through the firewall command.
fn apply_scopes(scopes: &[AuthorizedScope], grant: bool) -> Result<(), String> {
    for scope in scopes {
        validate_authorized_scope(scope)?;
    }
    if let Some(helper) = trusted_packaged_helper() {
        return apply_with_packaged_helper(&helper, scopes, grant);
    }
    apply_each(scopes, grant, |scope| match scope.firewall {
        FirewallKind::Ufw => apply_ufw(scope, grant),
        FirewallKind::Firewalld => apply_firewalld(scope, grant),
    })
}

/// Applies each scope in order. A grant stops at the first failure, since its scopes stay
/// pending; a revoke attempts every scope so one failure leaves no other rule in place. Returns
/// the first failure.
fn apply_each(
    scopes: &[AuthorizedScope],
    grant: bool,
    mut apply: impl FnMut(&AuthorizedScope) -> Result<(), String>,
) -> Result<(), String> {
    let mut first_error = None;
    for scope in scopes {
        if let Err(error) = apply(scope) {
            if grant {
                return Err(error);
            }
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Returns the packaged helper when this process is the packaged app and the helper is a
/// regular file that only root can replace. Other builds apply rules through the firewall
/// command directly.
fn trusted_packaged_helper() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?.canonicalize().ok()?;
    if current != Path::new(PACKAGED_APP_PATH).canonicalize().ok()? {
        return None;
    }
    let helper = Path::new(PACKAGED_HELPER_PATH);
    let metadata = helper.symlink_metadata().ok()?;
    let folder = helper.parent()?.symlink_metadata().ok()?;
    (metadata.is_file()
        && root_controlled(&metadata)
        && folder.is_dir()
        && root_controlled(&folder))
    .then(|| helper.to_path_buf())
}

/// Whether only root can change the entry: owned by root and not group or world writable.
fn root_controlled(metadata: &fs::Metadata) -> bool {
    metadata.uid() == 0 && metadata.mode() & 0o022 == 0
}

fn apply_with_packaged_helper(
    helper: &Path,
    scopes: &[AuthorizedScope],
    grant: bool,
) -> Result<(), String> {
    let arguments = privileged_helper_arguments(scopes, grant)?;
    let pkexec = required_command(PKEXEC_PATHS, "pkexec")?;
    let mut command = Command::new(pkexec);
    command.arg(helper).args(arguments);
    run_authorized(
        command,
        "update the Ganbaru phone connection rule",
        &[11, 12],
    )
}

/// The helper arguments that apply every scope in one invocation, so the polkit policy asks for
/// the administrator password once per flow.
fn privileged_helper_arguments(
    scopes: &[AuthorizedScope],
    grant: bool,
) -> Result<Vec<String>, String> {
    if scopes.is_empty() || scopes.len() > MAX_AUTHORIZED_SCOPES {
        return Err("the Linux network helper request has an invalid scope count".to_string());
    }
    let mut arguments = vec![
        PRIVILEGED_HELPER_ARGUMENT.to_string(),
        if grant { "grant" } else { "revoke" }.to_string(),
    ];
    for scope in scopes {
        if scope.port != COORDINATOR_PORT {
            return Err(
                "the packaged Linux network helper supports only the production coordinator port"
                    .to_string(),
            );
        }
        arguments.extend([
            match scope.firewall {
                FirewallKind::Ufw => "ufw",
                FirewallKind::Firewalld => "firewalld",
            }
            .to_string(),
            scope.interface.clone(),
            scope.local_address.to_string(),
            scope.source_network.clone(),
            scope.zone.clone().unwrap_or_else(|| "-".to_string()),
        ]);
    }
    Ok(arguments)
}

/// Runs the root firewall helper request in the process arguments, if there is one.
///
/// The `ganbaru-linking-firewall` binary calls it. The package policy authorizes only that
/// root-owned helper with `--manage-linking-firewall` as its first argument. All remaining
/// values are parsed and validated again before a fixed firewall executable is invoked.
pub fn run_privileged_helper_if_requested() -> Option<Result<(), String>> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(OsStr::new(PRIVILEGED_HELPER_ARGUMENT)) {
        return None;
    }
    Some(
        arguments
            .map(|argument| {
                argument.into_string().map_err(|_| {
                    "the Linux network helper received an argument that is not UTF-8".to_string()
                })
            })
            .collect::<Result<Vec<String>, String>>()
            .and_then(|arguments| parse_privileged_helper_request(&mut arguments.into_iter()))
            .and_then(|request| {
                if !effective_user_is_root() {
                    return Err(
                        "the Linux network helper requires administrator access".to_string()
                    );
                }
                match request {
                    HelperRequest::Apply { scopes, grant } => {
                        apply_each(&scopes, grant, |scope| apply_scope_privileged(scope, grant))
                    }
                    HelperRequest::RevokeAll => revoke_all_privileged(),
                }
            }),
    )
}

/// An operation of the privileged helper.
#[derive(Debug, PartialEq, Eq)]
enum HelperRequest {
    /// Adds or removes the rules of one flow's scopes.
    Apply {
        scopes: Vec<AuthorizedScope>,
        grant: bool,
    },
    /// Removes every production coordinator rule the app added, found by its shape in the
    /// firewall rather than in per-user state. Package removal runs it as root.
    RevokeAll,
}

fn parse_privileged_helper_request(
    arguments: &mut impl Iterator<Item = String>,
) -> Result<HelperRequest, String> {
    let grant = match arguments.next().as_deref() {
        Some("grant") => true,
        Some("revoke") => false,
        Some(REVOKE_ALL_OPERATION) => {
            if arguments.next().is_some() {
                return Err("the Linux network helper received unexpected arguments".to_string());
            }
            return Ok(HelperRequest::RevokeAll);
        }
        _ => return Err("the Linux network helper operation is invalid".to_string()),
    };
    let mut scopes = Vec::new();
    while let Some(firewall) = arguments.next() {
        if scopes.len() == MAX_AUTHORIZED_SCOPES {
            return Err("the Linux network helper received too many scopes".to_string());
        }
        scopes.push(parse_privileged_helper_scope(&firewall, arguments)?);
    }
    if scopes.is_empty() {
        return Err("the Linux network helper scope is missing".to_string());
    }
    Ok(HelperRequest::Apply { scopes, grant })
}

fn parse_privileged_helper_scope(
    firewall: &str,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<AuthorizedScope, String> {
    let firewall = match firewall {
        "ufw" => FirewallKind::Ufw,
        "firewalld" => FirewallKind::Firewalld,
        _ => return Err("the Linux network helper firewall is invalid".to_string()),
    };
    let interface = arguments
        .next()
        .ok_or_else(|| "the Linux network helper interface is missing".to_string())?;
    let local_address = arguments
        .next()
        .ok_or_else(|| "the Linux network helper address is missing".to_string())?
        .parse::<Ipv4Addr>()
        .map_err(|_| "the Linux network helper address is invalid".to_string())?;
    let source_network = arguments
        .next()
        .ok_or_else(|| "the Linux network helper source is missing".to_string())?;
    let zone = match arguments
        .next()
        .ok_or_else(|| "the Linux network helper zone is missing".to_string())?
        .as_str()
    {
        "-" => None,
        value => Some(value.to_string()),
    };
    let scope = AuthorizedScope {
        firewall,
        interface,
        local_address,
        source_network,
        port: COORDINATOR_PORT,
        zone,
    };
    validate_authorized_scope(&scope)?;
    Ok(scope)
}

fn effective_user_is_root() -> bool {
    let Ok(status) = fs::read_to_string("/proc/self/status") else {
        return false;
    };
    status.lines().find_map(|line| {
        let values = line.strip_prefix("Uid:")?;
        values
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u32>().ok())
    }) == Some(0)
}

fn apply_scope_privileged(scope: &AuthorizedScope, grant: bool) -> Result<(), String> {
    validate_authorized_scope(scope)?;
    match scope.firewall {
        FirewallKind::Ufw => {
            let ufw = required_command(UFW_PATHS, "UFW")?;
            let mut command = Command::new(ufw);
            command.args(ufw_arguments(scope, grant));
            run_authorized(command, "update UFW", &[])
        }
        FirewallKind::Firewalld => apply_firewalld_direct(scope, grant),
    }
}

/// Removes every production coordinator rule carrying the app's shape from UFW and from every
/// firewalld zone, runtime and permanent. Firewalls that are absent or stopped are skipped, and
/// a failed removal does not stop the others. Returns the first failure.
fn revoke_all_privileged() -> Result<(), String> {
    let mut first_error = None;
    let mut record = |result: Result<(), String>| {
        if let Err(error) = result {
            first_error.get_or_insert(error);
        }
    };
    if let Some(ufw) = command_path(UFW_PATHS) {
        let mut command = Command::new(ufw);
        command.args(["show", "added"]);
        match command_stdout(command, "list UFW rules") {
            Ok(output) => {
                for scope in parse_ufw_added_rules(&output) {
                    record(apply_scope_privileged(&scope, false));
                }
            }
            Err(error) => record(Err(error)),
        }
    }
    if is_firewalld_running() {
        match firewalld_app_rules() {
            Ok(scopes) => {
                for scope in scopes {
                    record(apply_firewalld_direct(&scope, false));
                }
            }
            Err(error) => record(Err(error)),
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// The app's rules in every runtime and permanent firewalld zone, once each.
fn firewalld_app_rules() -> Result<Vec<AuthorizedScope>, String> {
    let firewall_cmd = required_command(FIREWALL_CMD_PATHS, "firewalld")?;
    let firewalld = |permanent: bool, arguments: &[String]| {
        let mut command = Command::new(&firewall_cmd);
        if permanent {
            command.arg("--permanent");
        }
        command.args(arguments);
        command_stdout(command, "list firewalld rules")
    };
    let mut zones = Vec::new();
    for permanent in [false, true] {
        for zone in firewalld(permanent, &["--get-zones".to_string()])?.split_whitespace() {
            if is_valid_zone(zone) && !zones.iter().any(|known| known == zone) {
                zones.push(zone.to_string());
            }
        }
    }
    let mut scopes: Vec<AuthorizedScope> = Vec::new();
    for zone in &zones {
        for permanent in [false, true] {
            let rules = firewalld(
                permanent,
                &[format!("--zone={zone}"), "--list-rich-rules".to_string()],
            )?;
            for scope in rules
                .lines()
                .filter_map(|rule| parse_firewalld_rich_rule(zone, rule))
            {
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
        }
    }
    Ok(scopes)
}

fn command_stdout(mut command: Command, action: &str) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("{action}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{action}: {}", output_message(&output)));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{action}: the output is not UTF-8"))
}

/// Scopes of the `ufw show added` lines that carry the app comment, allow TCP to the
/// production coordinator port from a private source, and pass scope validation.
fn parse_ufw_added_rules(output: &str) -> Vec<AuthorizedScope> {
    output.lines().filter_map(parse_ufw_added_rule).collect()
}

fn parse_ufw_added_rule(line: &str) -> Option<AuthorizedScope> {
    let (rule, comment) = line.trim().split_once(" comment ")?;
    if comment != format!("'{RULE_COMMENT}'") {
        return None;
    }
    let mut tokens = rule.split_whitespace();
    for expected in ["ufw", "allow", "in", "on"] {
        if tokens.next()? != expected {
            return None;
        }
    }
    let interface = tokens.next()?.to_string();
    let (mut source, mut destination, mut port, mut protocol) = (None, None, None, None);
    let mut port_follows_destination = false;
    while let Some(key) = tokens.next() {
        let value = tokens.next()?;
        match key {
            "from" if source.is_none() => source = Some(value),
            "to" if destination.is_none() => destination = Some(value),
            "port" if port_follows_destination && port.is_none() => port = Some(value),
            "proto" if protocol.is_none() => protocol = Some(value),
            _ => return None,
        }
        port_follows_destination = key == "to";
    }
    if protocol? != "tcp" || port?.parse::<u16>().ok()? != COORDINATOR_PORT {
        return None;
    }
    let scope = AuthorizedScope {
        firewall: FirewallKind::Ufw,
        interface,
        local_address: destination?.parse().ok()?,
        source_network: source?.to_string(),
        port: COORDINATOR_PORT,
        zone: None,
    };
    validate_authorized_scope(&scope).ok()?;
    Some(scope)
}

/// The scope of a firewalld rich rule in exactly the shape the app adds for the production
/// coordinator port. Rich rules carry no interface, so the scope has none; only its zone,
/// addresses, and port reach firewalld.
fn parse_firewalld_rich_rule(zone: &str, rule: &str) -> Option<AuthorizedScope> {
    let rest = rule
        .trim()
        .strip_prefix("rule family=\"ipv4\" source address=\"")?;
    let (source, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" destination address=\"")?;
    let (destination, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" port port=\"")?;
    let (port, rest) = rest.split_once('"')?;
    if rest != " protocol=\"tcp\" accept" || port.parse::<u16>().ok()? != COORDINATOR_PORT {
        return None;
    }
    let local_address: Ipv4Addr = destination
        .strip_suffix("/32")
        .unwrap_or(destination)
        .parse()
        .ok()?;
    validate_rule_addresses(local_address, source, COORDINATOR_PORT).ok()?;
    is_valid_zone(zone).then(|| AuthorizedScope {
        firewall: FirewallKind::Firewalld,
        interface: String::new(),
        local_address,
        source_network: source.to_string(),
        port: COORDINATOR_PORT,
        zone: Some(zone.to_string()),
    })
}

fn validate_authorized_scope(scope: &AuthorizedScope) -> Result<(), String> {
    if !is_valid_interface(&scope.interface) {
        return Err("saved Linux network access scope is invalid".to_string());
    }
    validate_rule_addresses(scope.local_address, &scope.source_network, scope.port)?;
    match scope.firewall {
        FirewallKind::Ufw if scope.zone.is_none() => Ok(()),
        FirewallKind::Firewalld if scope.zone.as_deref().is_some_and(is_valid_zone) => Ok(()),
        _ => Err("saved Linux firewall scope is inconsistent".to_string()),
    }
}

/// Checks that a rule opens a coordinator port on a private, link-local, or overlay address to
/// an aligned source range that holds the address and stays inside its block.
fn validate_rule_addresses(
    local_address: Ipv4Addr,
    source_network: &str,
    port: u16,
) -> Result<(), String> {
    let block = ipv4_lan_block(local_address);
    if block.is_none() || !matches!(port, COORDINATOR_PORT | DEVELOPMENT_COORDINATOR_PORT) {
        return Err("saved Linux network access scope is invalid".to_string());
    }
    let (network, prefix) = source_network
        .split_once('/')
        .ok_or_else(|| "saved Linux network source is invalid".to_string())?;
    let network = network
        .parse::<Ipv4Addr>()
        .map_err(|_| "saved Linux network source is invalid".to_string())?;
    let prefix = prefix
        .parse::<u32>()
        .map_err(|_| "saved Linux network prefix is invalid".to_string())?;
    if block.is_none_or(|(_, block_prefix)| prefix > 32 || prefix < block_prefix) {
        return Err("saved Linux network source extends past its private block".to_string());
    }
    let mask = ipv4_mask(prefix);
    if u32::from(network) & mask != u32::from(network)
        || u32::from(local_address) & mask != u32::from(network)
    {
        return Err("saved Linux network source does not contain its local address".to_string());
    }
    Ok(())
}

fn apply_ufw(scope: &AuthorizedScope, grant: bool) -> Result<(), String> {
    let pkexec = required_command(PKEXEC_PATHS, "pkexec")?;
    let ufw = required_command(UFW_PATHS, "UFW")?;
    let mut command = Command::new(pkexec);
    command.arg(ufw).args(ufw_arguments(scope, grant));
    run_authorized(command, "update UFW", &[])
}

fn ufw_arguments(scope: &AuthorizedScope, grant: bool) -> Vec<String> {
    let mut arguments = Vec::new();
    if !grant {
        arguments.push("delete".to_string());
    }
    arguments.extend([
        "allow".to_string(),
        "in".to_string(),
        "on".to_string(),
        scope.interface.clone(),
        "proto".to_string(),
        "tcp".to_string(),
        "from".to_string(),
        scope.source_network.clone(),
        "to".to_string(),
        scope.local_address.to_string(),
        "port".to_string(),
        scope.port.to_string(),
        "comment".to_string(),
        RULE_COMMENT.to_string(),
    ]);
    arguments
}

fn apply_firewalld(scope: &AuthorizedScope, grant: bool) -> Result<(), String> {
    let pkexec = required_command(PKEXEC_PATHS, "pkexec")?;
    apply_firewalld_commands(scope, grant, |firewall_cmd| {
        let mut command = Command::new(&pkexec);
        command.arg(firewall_cmd);
        command
    })
}

fn apply_firewalld_direct(scope: &AuthorizedScope, grant: bool) -> Result<(), String> {
    apply_firewalld_commands(scope, grant, |firewall_cmd| Command::new(firewall_cmd))
}

fn apply_firewalld_commands(
    scope: &AuthorizedScope,
    grant: bool,
    mut command_for: impl FnMut(&Path) -> Command,
) -> Result<(), String> {
    let firewall_cmd = required_command(FIREWALL_CMD_PATHS, "firewalld")?;
    let zone = scope
        .zone
        .as_deref()
        .filter(|zone| is_valid_zone(zone))
        .ok_or_else(|| "the active firewalld zone is unavailable".to_string())?;
    let operation = if grant {
        "--add-rich-rule"
    } else {
        "--remove-rich-rule"
    };
    let rule = format!(
        "rule family=\"ipv4\" source address=\"{}\" destination address=\"{}/32\" port port=\"{}\" protocol=\"tcp\" accept",
        scope.source_network, scope.local_address, scope.port
    );
    for permanent in [false, true] {
        let mut command = command_for(&firewall_cmd);
        command.arg(format!("--zone={zone}"));
        if permanent {
            command.arg("--permanent");
        }
        command.arg(format!("{operation}={rule}"));
        run_authorized(command, "update firewalld", &[11, 12])?;
    }
    Ok(())
}

fn run_authorized(
    mut command: Command,
    action: &str,
    accepted_statuses: &[i32],
) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("{action}: {error}"))?;
    if output.status.success()
        || output
            .status
            .code()
            .is_some_and(|code| accepted_statuses.contains(&code))
    {
        return Ok(());
    }
    if matches!(output.status.code(), Some(126 | 127)) {
        return Err("system authorization was cancelled or denied".to_string());
    }
    Err(format!("{action}: {}", output_message(&output)))
}

fn output_message(output: &Output) -> String {
    let bytes = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let value = String::from_utf8_lossy(bytes);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        format!("command exited with status {}", output.status)
    } else {
        trimmed.chars().take(500).collect()
    }
}

fn required_command(paths: &[&str], label: &str) -> Result<PathBuf, String> {
    command_path(paths).ok_or_else(|| format!("{label} is unavailable"))
}

/// The first of `paths` that resolves to a regular file in a folder, both only root can
/// replace, so neither the app nor the root helper runs a binary another account planted.
fn command_path(paths: &[&str]) -> Option<PathBuf> {
    paths.iter().find_map(|path| {
        let resolved = Path::new(path).canonicalize().ok()?;
        let metadata = resolved.symlink_metadata().ok()?;
        let folder = resolved.parent()?.symlink_metadata().ok()?;
        (metadata.is_file() && root_controlled(&metadata) && root_controlled(&folder))
            .then(|| PathBuf::from(path))
    })
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILE)
}

fn read_state(config_dir: &Path) -> Result<StoredNetworkAccess, String> {
    let path = state_path(config_dir);
    if !path.exists() {
        return Ok(empty_state());
    }
    let bytes = fs::read(&path).map_err(|error| format!("read network access state: {error}"))?;
    let state: StoredNetworkAccess = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse network access state: {error}"))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err("network access state version is unsupported".to_string());
    }
    if state.scopes.len() + state.pending.len() > MAX_AUTHORIZED_SCOPES {
        return Err("network access state has too many scopes".to_string());
    }
    for scope in state
        .scopes
        .iter()
        .map(|saved| &saved.rule)
        .chain(&state.pending)
    {
        validate_authorized_scope(scope)?;
    }
    for saved in &state.scopes {
        if saved.networks.len() > MAX_ALLOWED_NETWORKS
            || !saved
                .networks
                .iter()
                .all(|network| is_network_identity(network))
        {
            return Err("network access state has an invalid network".to_string());
        }
    }
    Ok(state)
}

fn persist_state(config_dir: &Path, state: &StoredNetworkAccess) -> Result<(), String> {
    let path = state_path(config_dir);
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("serialize network access state: {error}"))?;
    super::pairing::write_private_file_atomically(&path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_route_produces_a_bounded_private_scope() {
        let route = "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\n\
wlp2s0 00000000 FE01A8C0 0003 0 0 600 00000000 0 0 0\n\
wlp2s0 0001A8C0 00000000 0001 0 0 600 00FFFFFF 0 0 0\n";
        let address = Ipv4Addr::new(192, 168, 1, 66);
        let (interface, mask) =
            parse_local_ipv4_route(route, address).expect("connected private route");
        assert_eq!(interface, "wlp2s0");
        assert_eq!(mask, Ipv4Addr::new(255, 255, 255, 0));
        let network = Ipv4Addr::from(u32::from(address) & u32::from(mask));
        assert_eq!(
            format!("{network}/{}", ipv4_prefix(mask).unwrap()),
            "192.168.1.0/24"
        );
    }

    #[test]
    fn only_root_owned_entries_that_others_cannot_write_are_trusted() {
        assert!(root_controlled(&Path::new("/").symlink_metadata().unwrap()));
        assert_ne!(PACKAGED_HELPER_PATH, PACKAGED_APP_PATH);

        let folder = std::env::temp_dir().join(format!("ganbaru-helper-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let metadata = folder.symlink_metadata().unwrap();
        if metadata.uid() != 0 {
            assert!(!root_controlled(&metadata));
        }
        fs::remove_dir(&folder).unwrap();
    }

    #[test]
    fn route_parser_rejects_unsafe_interface_names() {
        let route = "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\n\
bad/interface 0001A8C0 00000000 0001 0 0 600 00FFFFFF 0 0 0\n";
        assert!(parse_local_ipv4_route(route, Ipv4Addr::new(192, 168, 1, 66)).is_err());
    }

    #[test]
    fn persisted_scopes_cannot_escape_the_private_network_boundary() {
        let valid = AuthorizedScope {
            firewall: FirewallKind::Ufw,
            interface: "wlp2s0".to_string(),
            local_address: Ipv4Addr::new(192, 168, 1, 66),
            source_network: "192.168.1.0/24".to_string(),
            port: COORDINATOR_PORT,
            zone: None,
        };
        assert!(validate_authorized_scope(&valid).is_ok());

        let mut broad = valid.clone();
        broad.source_network = "0.0.0.0/0".to_string();
        assert!(validate_authorized_scope(&broad).is_err());

        let mut unrelated = valid;
        unrelated.source_network = "192.168.2.0/24".to_string();
        assert!(validate_authorized_scope(&unrelated).is_err());

        let mut unsupported_port = unrelated;
        unsupported_port.source_network = "192.168.1.0/24".to_string();
        unsupported_port.port = 44_000;
        assert!(validate_authorized_scope(&unsupported_port).is_err());
    }

    #[test]
    fn opened_sources_stay_inside_the_private_block_of_the_local_address() {
        let scope = |local: [u8; 4], source: &str| AuthorizedScope {
            firewall: FirewallKind::Ufw,
            interface: "wlp2s0".to_string(),
            local_address: Ipv4Addr::from(local),
            source_network: source.to_string(),
            port: COORDINATOR_PORT,
            zone: None,
        };
        for (local, source) in [
            ([10, 1, 2, 3], "10.0.0.0/7"),
            ([192, 168, 1, 66], "192.168.0.0/13"),
            ([169, 254, 1, 2], "169.254.0.0/15"),
            ([100, 101, 2, 3], "100.0.0.0/8"),
        ] {
            assert!(
                validate_authorized_scope(&scope(local, source)).is_err(),
                "{source} must be refused"
            );
        }
        for (local, source) in [
            ([10, 1, 2, 3], "10.0.0.0/8"),
            ([172, 20, 1, 2], "172.16.0.0/12"),
            ([100, 101, 2, 3], "100.64.0.0/10"),
            ([100, 101, 2, 3], "100.101.2.0/24"),
        ] {
            assert!(
                validate_authorized_scope(&scope(local, source)).is_ok(),
                "{source} must be accepted"
            );
        }

        let mut broad = ["grant", "ufw", "wlp2s0", "10.1.2.3", "10.0.0.0/7", "-"]
            .into_iter()
            .map(str::to_string);
        assert!(parse_privileged_helper_request(&mut broad).is_err());
    }

    fn numbered_scope(index: u8) -> AuthorizedScope {
        AuthorizedScope {
            firewall: FirewallKind::Ufw,
            interface: "wlp2s0".to_string(),
            local_address: Ipv4Addr::new(192, 168, index, 66),
            source_network: format!("192.168.{index}.0/24"),
            port: COORDINATOR_PORT,
            zone: None,
        }
    }

    fn granted(index: u8, networks: &[&str]) -> GrantedScope {
        GrantedScope {
            rule: numbered_scope(index),
            networks: networks.iter().map(|network| network.to_string()).collect(),
        }
    }

    fn rules(stored: &StoredNetworkAccess) -> Vec<AuthorizedScope> {
        stored
            .scopes
            .iter()
            .map(|saved| saved.rule.clone())
            .collect()
    }

    const HOME: &str = "a4:2b:b0:11:22:33";
    const CAFE: &str = "f0:9f:c2:44:55:66";

    #[test]
    fn a_full_record_refuses_a_grant_before_any_rule_changes() {
        let mut stored = empty_state();
        stored.scopes = (0..30).map(|index| granted(index, &[])).collect();
        stored.pending = vec![numbered_scope(30)];
        let mut applied = Vec::new();
        let result = grant_scopes(
            &mut stored,
            vec![numbered_scope(31), numbered_scope(32)],
            None,
            |_| Ok(()),
            |scopes| {
                applied.extend_from_slice(scopes);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(applied.is_empty());
        assert_eq!(stored.scopes.len() + stored.pending.len(), 31);

        let granted = grant_scopes(
            &mut stored,
            vec![numbered_scope(30), numbered_scope(31)],
            None,
            |_| Ok(()),
            |scopes| {
                applied.extend_from_slice(scopes);
                Ok(())
            },
        );
        assert!(granted.is_ok());
        assert_eq!(applied, vec![numbered_scope(30), numbered_scope(31)]);
        assert_eq!(stored.scopes.len(), MAX_AUTHORIZED_SCOPES);
        assert!(stored.pending.is_empty());
    }

    #[test]
    fn one_flow_applies_its_scopes_in_one_call_and_a_failed_call_stays_revocable() {
        let mut stored = empty_state();
        let mut persisted: Vec<(usize, usize)> = Vec::new();
        let mut calls = Vec::new();
        let result = grant_scopes(
            &mut stored,
            vec![numbered_scope(1), numbered_scope(2)],
            Some(HOME),
            |state| {
                persisted.push((state.scopes.len(), state.pending.len()));
                Ok(())
            },
            |scopes| {
                calls.push(scopes.to_vec());
                Err("firewalld failed after the runtime rule".to_string())
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, vec![vec![numbered_scope(1), numbered_scope(2)]]);
        assert_eq!(persisted, vec![(0, 2)]);
        assert!(stored.scopes.is_empty());
        assert_eq!(stored.pending, vec![numbered_scope(1), numbered_scope(2)]);

        let granted_again = grant_scopes(
            &mut stored,
            vec![numbered_scope(1), numbered_scope(2), numbered_scope(3)],
            Some(HOME),
            |state| {
                persisted.push((state.scopes.len(), state.pending.len()));
                Ok(())
            },
            |scopes| {
                calls.push(scopes.to_vec());
                Ok(())
            },
        );
        assert!(granted_again.is_ok());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].len(), 3);
        assert_eq!(persisted[1..], [(0, 3), (3, 0)]);
        assert_eq!(rules(&stored).len(), 3);

        stored.pending = vec![numbered_scope(4)];
        let mut removals = Vec::new();
        let revoked = revoke_scopes(&mut stored, |scopes| {
            removals.push(scopes.to_vec());
            Err("authorization was cancelled".to_string())
        });
        assert!(revoked.is_err());
        assert_eq!(rules(&stored).len(), 3);
        assert_eq!(stored.pending, vec![numbered_scope(4)]);

        let revoked = revoke_scopes(&mut stored, |scopes| {
            removals.push(scopes.to_vec());
            Ok(())
        });
        assert!(revoked.is_ok());
        assert_eq!(removals.len(), 2);
        assert_eq!(
            removals[1],
            [1, 2, 3, 4].map(numbered_scope).to_vec(),
            "one call removes every granted and pending rule"
        );
        assert!(stored.scopes.is_empty() && stored.pending.is_empty());
        assert!(revoke_scopes(&mut stored, |_| panic!("nothing to remove")).is_ok());
    }

    #[test]
    fn rules_admit_peers_only_on_networks_the_user_confirmed() {
        let mut stored = empty_state();
        stored.scopes = vec![granted(1, &[HOME])];
        let candidates = [numbered_scope(1)];
        let no_authorization = || false;
        assert_eq!(
            access_state(&stored, &candidates, Some(HOME), no_authorization),
            NetworkAccessState::Granted
        );
        assert_eq!(
            access_state(&stored, &candidates, Some(CAFE), no_authorization),
            NetworkAccessState::NetworkConfirmationRequired
        );
        assert_eq!(
            access_state(&stored, &candidates, None, no_authorization),
            NetworkAccessState::Granted
        );
        let missing = [numbered_scope(1), numbered_scope(2)];
        assert_eq!(
            access_state(&stored, &missing, Some(HOME), || true),
            NetworkAccessState::AuthorizationRequired
        );
        assert_eq!(
            access_state(&stored, &missing, Some(HOME), no_authorization),
            NetworkAccessState::ManualActionRequired
        );

        stored.scopes = vec![granted(1, &[])];
        assert_eq!(
            access_state(&stored, &candidates, Some(HOME), no_authorization),
            NetworkAccessState::NetworkConfirmationRequired
        );
    }

    #[test]
    fn confirming_a_network_records_it_without_changing_the_firewall() {
        let mut stored = empty_state();
        stored.scopes = vec![granted(1, &[HOME])];
        let mut persisted = 0;
        let confirmed = grant_scopes(
            &mut stored,
            vec![numbered_scope(1)],
            Some(CAFE),
            |_| {
                persisted += 1;
                Ok(())
            },
            |_| panic!("an existing rule is not applied again"),
        );
        assert!(confirmed.is_ok());
        assert_eq!(persisted, 1);
        assert_eq!(stored.scopes, vec![granted(1, &[HOME, CAFE])]);
        let json = serde_json::to_vec(&stored).expect("serialize");
        let restored: StoredNetworkAccess = serde_json::from_slice(&json).expect("parse");
        assert_eq!(restored.scopes, stored.scopes);

        let mut saved = granted(1, &[]);
        let identities: Vec<String> = (0..=MAX_ALLOWED_NETWORKS)
            .map(|index| format!("02:00:00:00:00:{index:02x}"))
            .collect();
        for identity in &identities {
            assert!(allow_network(&mut saved, Some(identity)));
        }
        assert!(!allow_network(&mut saved, Some(&identities[1])));
        assert!(!allow_network(&mut saved, None));
        assert_eq!(saved.networks, identities[1..]);
    }

    #[test]
    fn the_network_identity_is_the_resolved_default_gateway_of_the_interface() {
        let routes = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
docker0\t00000000\t010011AC\t0003\t0\t0\t50\t00000000\t0\t0\t0
wlp2s0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
wlp2s0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
";
        assert_eq!(
            parse_default_gateway(routes, "wlp2s0"),
            Some(Ipv4Addr::new(192, 168, 1, 1))
        );
        assert_eq!(parse_default_gateway(routes, "eth0"), None);
        let down = routes.replace("0101A8C0\t0003", "0101A8C0\t0002");
        assert_eq!(parse_default_gateway(&down, "wlp2s0"), None);

        let neighbors = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         A4:2B:B0:11:22:33     *        wlp2s0
192.168.1.7      0x1         0x0         00:00:00:00:00:00     *        wlp2s0
172.17.0.1       0x1         0x2         f0:9f:c2:44:55:66     *        docker0
";
        let gateway = Ipv4Addr::new(192, 168, 1, 1);
        assert_eq!(
            parse_neighbor_hardware_address(neighbors, gateway, "wlp2s0").as_deref(),
            Some(HOME)
        );
        assert_eq!(
            parse_neighbor_hardware_address(neighbors, gateway, "eth0"),
            None
        );
        assert_eq!(
            parse_neighbor_hardware_address(neighbors, Ipv4Addr::new(192, 168, 1, 7), "wlp2s0"),
            None
        );
        let unresolved = neighbors.replace("0x2         A4", "0x0         A4");
        assert_eq!(
            parse_neighbor_hardware_address(&unresolved, gateway, "wlp2s0"),
            None
        );
        assert!(!is_network_identity("00:00:00:00:00:00"));
        assert!(!is_network_identity("A4:2B:B0:11:22:33"));
        assert!(!is_network_identity("a4:2b:b0:11:22"));
    }

    #[test]
    fn a_connected_route_broader_than_its_block_is_narrowed_to_the_block() {
        let local = Ipv4Addr::new(10, 1, 2, 3);
        let wide = lan_scope("eth0".to_string(), local, Ipv4Addr::new(254, 0, 0, 0)).unwrap();
        assert_eq!(wide.source_network, "10.0.0.0/8");
        let narrow = lan_scope("eth0".to_string(), local, Ipv4Addr::new(255, 255, 255, 0)).unwrap();
        assert_eq!(narrow.source_network, "10.1.2.0/24");
        let overlay = Ipv4Addr::new(100, 101, 2, 3);
        let route = "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\n\
tailscale0 00004064 00000000 0001 0 0 0 0000C0FF 0 0 0\n";
        let (interface, mask) = parse_local_ipv4_route(route, overlay).expect("overlay route");
        assert_eq!(interface, "tailscale0");
        assert_eq!(
            lan_scope(interface, overlay, mask).unwrap().source_network,
            "100.64.0.0/10"
        );
    }

    #[test]
    fn ufw_rule_is_limited_to_the_current_interface_address_and_subnet() {
        let scope = AuthorizedScope {
            firewall: FirewallKind::Ufw,
            interface: "wlp2s0".to_string(),
            local_address: Ipv4Addr::new(192, 168, 1, 66),
            source_network: "192.168.1.0/24".to_string(),
            port: DEVELOPMENT_COORDINATOR_PORT,
            zone: None,
        };
        assert_eq!(
            ufw_arguments(&scope, true),
            [
                "allow",
                "in",
                "on",
                "wlp2s0",
                "proto",
                "tcp",
                "from",
                "192.168.1.0/24",
                "to",
                "192.168.1.66",
                "port",
                "43822",
                "comment",
                "Ganbaru device linking",
            ]
        );
    }

    #[test]
    fn ufw_enabled_parser_ignores_comments_and_disabled_values() {
        assert!(!"# ENABLED=yes\nENABLED=no\n".lines().any(|line| {
            line.split_once('=')
                .is_some_and(|(key, value)| key.trim() == "ENABLED" && value.trim() == "yes")
        }));
    }

    #[test]
    fn privileged_helper_accepts_only_a_bounded_complete_scope() {
        let mut arguments = [
            "grant",
            "ufw",
            "wlp2s0",
            "192.168.1.66",
            "192.168.1.0/24",
            "-",
        ]
        .into_iter()
        .map(str::to_string);
        let Ok(HelperRequest::Apply { scopes, grant }) =
            parse_privileged_helper_request(&mut arguments)
        else {
            panic!("valid helper scope");
        };
        assert!(grant);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].firewall, FirewallKind::Ufw);
        assert_eq!(scopes[0].local_address, Ipv4Addr::new(192, 168, 1, 66));

        let mut broad = ["grant", "ufw", "wlp2s0", "192.168.1.66", "0.0.0.0/0", "-"]
            .into_iter()
            .map(str::to_string);
        assert!(parse_privileged_helper_request(&mut broad).is_err());

        let mut extra = [
            "grant",
            "ufw",
            "wlp2s0",
            "192.168.1.66",
            "192.168.1.0/24",
            "-",
            "unexpected",
        ]
        .into_iter()
        .map(str::to_string);
        assert!(parse_privileged_helper_request(&mut extra).is_err());

        let revoke_all = |arguments: &[&str]| {
            parse_privileged_helper_request(&mut arguments.iter().map(|value| value.to_string()))
        };
        assert_eq!(revoke_all(&["revoke-all"]), Ok(HelperRequest::RevokeAll));
        assert!(revoke_all(&["revoke-all", "ufw"]).is_err());
        assert!(revoke_all(&["grant"]).is_err());
    }

    #[test]
    fn one_helper_invocation_carries_every_scope_of_a_flow() {
        let firewalld = AuthorizedScope {
            firewall: FirewallKind::Firewalld,
            zone: Some("home".to_string()),
            ..numbered_scope(1)
        };
        let scopes = vec![numbered_scope(1), firewalld, numbered_scope(2)];
        for grant in [true, false] {
            let arguments = privileged_helper_arguments(&scopes, grant).expect("arguments");
            assert_eq!(arguments[0], PRIVILEGED_HELPER_ARGUMENT);
            let request = parse_privileged_helper_request(&mut arguments.into_iter().skip(1));
            assert_eq!(
                request,
                Ok(HelperRequest::Apply {
                    scopes: scopes.clone(),
                    grant
                })
            );
        }

        let most: Vec<AuthorizedScope> = (0..MAX_AUTHORIZED_SCOPES)
            .map(|index| numbered_scope(index as u8))
            .collect();
        let arguments = privileged_helper_arguments(&most, false).expect("bounded arguments");
        let mut too_many: Vec<String> = arguments.into_iter().skip(1).collect();
        too_many.extend(too_many[1..6].to_vec());
        assert!(parse_privileged_helper_request(&mut too_many.into_iter()).is_err());
        assert!(privileged_helper_arguments(&[], true).is_err());
        assert!(privileged_helper_arguments(&[most.clone(), most].concat(), true).is_err());
        let development = AuthorizedScope {
            port: DEVELOPMENT_COORDINATOR_PORT,
            ..numbered_scope(1)
        };
        assert!(privileged_helper_arguments(&[development], true).is_err());

        let mut truncated = privileged_helper_arguments(&scopes, true).expect("arguments");
        truncated.pop();
        assert!(parse_privileged_helper_request(&mut truncated.into_iter().skip(1)).is_err());
    }

    #[test]
    fn a_grant_stops_at_its_first_failure_while_a_revoke_attempts_every_scope() {
        let scopes = [1, 2, 3].map(numbered_scope);
        let fail_second = |attempted: &mut Vec<AuthorizedScope>, scope: &AuthorizedScope| {
            attempted.push(scope.clone());
            if scope == &numbered_scope(2) {
                Err(format!("failed {}", scope.source_network))
            } else {
                Ok(())
            }
        };
        let mut attempted = Vec::new();
        let granted = apply_each(&scopes, true, |scope| fail_second(&mut attempted, scope));
        assert_eq!(granted, Err("failed 192.168.2.0/24".to_string()));
        assert_eq!(attempted, scopes[..2]);

        let mut attempted = Vec::new();
        let revoked = apply_each(&scopes, false, |scope| fail_second(&mut attempted, scope));
        assert_eq!(revoked, Err("failed 192.168.2.0/24".to_string()));
        assert_eq!(attempted, scopes);
    }

    #[test]
    fn system_commands_resolve_only_to_files_root_controls() {
        use std::os::unix::fs::PermissionsExt;

        assert!(
            PKEXEC_PATHS
                .iter()
                .all(|path| !path.starts_with("/usr/local/"))
        );
        assert_eq!(
            command_path(&["/nonexistent/pkexec", "/usr/bin/env"]),
            Some(PathBuf::from("/usr/bin/env"))
        );

        let folder = std::env::temp_dir().join(format!("ganbaru-command-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let planted = folder.join("pkexec");
        fs::write(&planted, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&planted, fs::Permissions::from_mode(0o755)).unwrap();
        if planted.symlink_metadata().unwrap().uid() != 0 {
            assert_eq!(command_path(&[planted.to_str().unwrap()]), None);
        }
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn package_removal_finds_only_the_app_ufw_rules() {
        let output = "\
Added user rules (see 'ufw status' for running firewall):
ufw allow 22/tcp
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'My own rule'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 8080 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 0.0.0.0/0 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 port 43821 to 192.168.1.66 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto udp comment 'Ganbaru device linking'
ufw deny in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on eth0 from 10.0.0.0/8 to 10.4.0.2 port 43821 proto tcp comment 'Ganbaru device linking'
";
        let scopes = parse_ufw_added_rules(output);
        assert_eq!(
            scopes,
            vec![
                AuthorizedScope {
                    firewall: FirewallKind::Ufw,
                    interface: "wlp2s0".to_string(),
                    local_address: Ipv4Addr::new(192, 168, 1, 66),
                    source_network: "192.168.1.0/24".to_string(),
                    port: COORDINATOR_PORT,
                    zone: None,
                },
                AuthorizedScope {
                    firewall: FirewallKind::Ufw,
                    interface: "eth0".to_string(),
                    local_address: Ipv4Addr::new(10, 4, 0, 2),
                    source_network: "10.0.0.0/8".to_string(),
                    port: COORDINATOR_PORT,
                    zone: None,
                },
            ]
        );
        assert_eq!(
            ufw_arguments(&scopes[0], false)[..4],
            ["delete", "allow", "in", "on"].map(str::to_string)
        );
    }

    #[test]
    fn package_removal_finds_only_the_app_firewalld_rules() {
        let app = "rule family=\"ipv4\" source address=\"192.168.1.0/24\" destination address=\"192.168.1.66/32\" port port=\"43821\" protocol=\"tcp\" accept";
        let scope = parse_firewalld_rich_rule("home", app).expect("app rule");
        assert_eq!(scope.zone.as_deref(), Some("home"));
        assert_eq!(scope.local_address, Ipv4Addr::new(192, 168, 1, 66));
        assert_eq!(scope.source_network, "192.168.1.0/24");
        let normalized = app.replace("192.168.1.66/32", "192.168.1.66");
        assert_eq!(parse_firewalld_rich_rule("home", &normalized), Some(scope));

        for foreign in [
            app.replace("43821", "8080"),
            app.replace("accept", "reject"),
            app.replace("\"tcp\"", "\"udp\""),
            app.replace("192.168.1.0/24", "0.0.0.0/0"),
            app.replace("ipv4", "ipv6"),
            format!("{app} log"),
            "rule service name=\"ssh\" accept".to_string(),
        ] {
            assert_eq!(
                parse_firewalld_rich_rule("home", &foreign),
                None,
                "{foreign}"
            );
        }
        assert_eq!(parse_firewalld_rich_rule("bad zone", app), None);
    }
}
