//! Explicit Linux firewall access for the private-LAN handoff coordinator.

use serde::{Deserialize, Serialize};
use std::fs;
use std::net::Ipv4Addr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::dial::{ipv4_lan_block, ipv4_mask};
use crate::protocol::{COORDINATOR_PORT, DEVELOPMENT_COORDINATOR_PORT};

const STATE_FILE: &str = "vault-handoff-network-access.json";
const STATE_SCHEMA_VERSION: u32 = 2;
const RULE_COMMENT: &str = "Ganbaru device linking";
const MAX_AUTHORIZED_SCOPES: usize = 32;
const PACKAGED_HELPER_PATH: &str = "/usr/bin/ganbaru";
const PRIVILEGED_HELPER_ARGUMENT: &str = "--manage-linking-firewall";
const REVOKE_ALL_OPERATION: &str = "revoke-all";

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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredNetworkAccess {
    schema_version: u32,
    /// Scopes whose firewall rules are in place.
    scopes: Vec<AuthorizedScope>,
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
    if !can_request_authorization(&firewalls) {
        return NetworkAccessStatus {
            state: NetworkAccessState::ManualActionRequired,
        };
    }
    let Ok(stored) = read_state(config_dir) else {
        return NetworkAccessStatus {
            state: NetworkAccessState::ManualActionRequired,
        };
    };
    let all_recorded = firewalls
        .iter()
        .all(|candidate| stored.scopes.iter().any(|saved| saved == candidate));
    NetworkAccessStatus {
        state: if all_recorded {
            NetworkAccessState::Granted
        } else {
            NetworkAccessState::AuthorizationRequired
        },
    }
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
    if !can_request_authorization(&firewalls) {
        return Err("automatic Linux firewall authorization is unavailable".to_string());
    }

    let mut stored = read_state(config_dir)?;
    grant_scopes(
        &mut stored,
        firewalls,
        |state| persist_state(config_dir, state),
        |scope| apply_scope(scope, true),
    )?;
    Ok(NetworkAccessStatus {
        state: NetworkAccessState::Granted,
    })
}

/// Applies the rules of `candidates` that `stored` does not hold yet.
///
/// Capacity is checked before any rule changes. Each scope is persisted as pending before its
/// rule is applied and moved to the granted scopes after, so a failure or crash between the two
/// never leaves a root rule without a record that revoking can find.
fn grant_scopes(
    stored: &mut StoredNetworkAccess,
    candidates: Vec<AuthorizedScope>,
    mut persist: impl FnMut(&StoredNetworkAccess) -> Result<(), String>,
    mut apply: impl FnMut(&AuthorizedScope) -> Result<(), String>,
) -> Result<(), String> {
    let missing: Vec<AuthorizedScope> = candidates
        .into_iter()
        .filter(|candidate| !stored.scopes.contains(candidate))
        .collect();
    let new_records = missing
        .iter()
        .filter(|candidate| !stored.pending.contains(candidate))
        .count();
    if stored.scopes.len() + stored.pending.len() + new_records > MAX_AUTHORIZED_SCOPES {
        return Err("too many saved Linux network access scopes".to_string());
    }
    for candidate in missing {
        if !stored.pending.contains(&candidate) {
            stored.pending.push(candidate.clone());
            persist(stored)?;
        }
        apply(&candidate)?;
        stored.pending.retain(|pending| pending != &candidate);
        stored.scopes.push(candidate);
        persist(stored)?;
    }
    Ok(())
}

pub fn revoke(config_dir: &Path) -> Result<NetworkAccessStatus, String> {
    let mut stored = read_state(config_dir)?;
    let result = revoke_scopes(&mut stored, |scope| apply_scope(scope, false));
    persist_state(config_dir, &stored)?;
    result?;
    Ok(NetworkAccessStatus {
        state: NetworkAccessState::AuthorizationRequired,
    })
}

/// Removes the rules of every granted and pending scope, keeping the scopes whose removal
/// failed so a later revoke can retry them. Returns the first failure.
fn revoke_scopes(
    stored: &mut StoredNetworkAccess,
    mut remove: impl FnMut(&AuthorizedScope) -> Result<(), String>,
) -> Result<(), String> {
    let mut first_error = None;
    for scopes in [&mut stored.scopes, &mut stored.pending] {
        scopes.retain(|scope| match remove(scope) {
            Ok(()) => false,
            Err(error) => {
                first_error.get_or_insert(error);
                true
            }
        });
    }
    first_error.map_or(Ok(()), Err)
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
    command_path(&["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"])
        .and_then(|path| Command::new(path).arg("--state").output().ok())
        .is_some_and(|output| output.status.success() && output.stdout == b"running\n")
}

fn firewalld_zone(interface: &str) -> Option<String> {
    let path = command_path(&["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"])?;
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
    let pkexec = command_path(&["/usr/bin/pkexec", "/usr/local/bin/pkexec"]);
    pkexec.is_some()
        && scopes.iter().all(|scope| match scope.firewall {
            FirewallKind::Ufw => command_path(&["/usr/sbin/ufw", "/usr/bin/ufw"]).is_some(),
            FirewallKind::Firewalld => {
                command_path(&["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"]).is_some()
                    && scope.zone.is_some()
            }
        })
}

fn apply_scope(scope: &AuthorizedScope, grant: bool) -> Result<(), String> {
    validate_authorized_scope(scope)?;
    if let Some(helper) = trusted_packaged_helper() {
        return apply_with_packaged_helper(&helper, scope, grant);
    }
    match scope.firewall {
        FirewallKind::Ufw => apply_ufw(scope, grant),
        FirewallKind::Firewalld => apply_firewalld(scope, grant),
    }
}

fn trusted_packaged_helper() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?.canonicalize().ok()?;
    let expected = Path::new(PACKAGED_HELPER_PATH).canonicalize().ok()?;
    if current != expected {
        return None;
    }
    let metadata = expected.metadata().ok()?;
    (metadata.uid() == 0 && metadata.mode() & 0o022 == 0).then_some(expected)
}

fn apply_with_packaged_helper(
    helper: &Path,
    scope: &AuthorizedScope,
    grant: bool,
) -> Result<(), String> {
    if scope.port != COORDINATOR_PORT {
        return Err(
            "the packaged Linux network helper supports only the production coordinator port"
                .to_string(),
        );
    }
    let pkexec = required_command(&["/usr/bin/pkexec", "/usr/local/bin/pkexec"], "pkexec")?;
    let mut command = Command::new(pkexec);
    command
        .arg(helper)
        .arg(PRIVILEGED_HELPER_ARGUMENT)
        .arg(if grant { "grant" } else { "revoke" })
        .arg(match scope.firewall {
            FirewallKind::Ufw => "ufw",
            FirewallKind::Firewalld => "firewalld",
        })
        .arg(&scope.interface)
        .arg(scope.local_address.to_string())
        .arg(&scope.source_network)
        .arg(scope.zone.as_deref().unwrap_or("-"));
    run_authorized(
        command,
        "update the Ganbaru phone connection rule",
        &[11, 12],
    )
}

/// Handles the narrowly scoped firewall helper mode used by packaged Linux builds.
///
/// The package policy authorizes only the root-owned Ganbaru executable with
/// `--manage-linking-firewall` as its first argument. All remaining values are
/// parsed and validated again before a fixed firewall executable is invoked.
pub fn run_privileged_helper_if_requested() -> Option<Result<(), String>> {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() != Some(PRIVILEGED_HELPER_ARGUMENT) {
        return None;
    }
    Some(
        parse_privileged_helper_request(&mut arguments).and_then(|request| {
            if !effective_user_is_root() {
                return Err("the Linux network helper requires administrator access".to_string());
            }
            match request {
                HelperRequest::Apply { scope, grant } => apply_scope_privileged(&scope, grant),
                HelperRequest::RevokeAll => revoke_all_privileged(),
            }
        }),
    )
}

/// An operation of the privileged helper.
#[derive(Debug, PartialEq, Eq)]
enum HelperRequest {
    /// Adds or removes the rule of one scope.
    Apply { scope: AuthorizedScope, grant: bool },
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
    parse_privileged_helper_scope(arguments).map(|scope| HelperRequest::Apply { scope, grant })
}

fn parse_privileged_helper_scope(
    arguments: &mut impl Iterator<Item = String>,
) -> Result<AuthorizedScope, String> {
    let firewall = match arguments.next().as_deref() {
        Some("ufw") => FirewallKind::Ufw,
        Some("firewalld") => FirewallKind::Firewalld,
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
    if arguments.next().is_some() {
        return Err("the Linux network helper received unexpected arguments".to_string());
    }
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
            let ufw = required_command(&["/usr/sbin/ufw", "/usr/bin/ufw"], "UFW")?;
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
    if let Some(ufw) = command_path(&["/usr/sbin/ufw", "/usr/bin/ufw"]) {
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
    let firewall_cmd = required_command(
        &["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"],
        "firewalld",
    )?;
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
    let pkexec = required_command(&["/usr/bin/pkexec", "/usr/local/bin/pkexec"], "pkexec")?;
    let ufw = required_command(&["/usr/sbin/ufw", "/usr/bin/ufw"], "UFW")?;
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
    let pkexec = required_command(&["/usr/bin/pkexec", "/usr/local/bin/pkexec"], "pkexec")?;
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
    let firewall_cmd = required_command(
        &["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"],
        "firewalld",
    )?;
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

fn command_path(paths: &[&str]) -> Option<PathBuf> {
    paths.iter().map(PathBuf::from).find(|path| path.is_file())
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
    for scope in state.scopes.iter().chain(&state.pending) {
        validate_authorized_scope(scope)?;
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

    #[test]
    fn a_full_record_refuses_a_grant_before_any_rule_changes() {
        let mut stored = empty_state();
        stored.scopes = (0..30).map(numbered_scope).collect();
        stored.pending = vec![numbered_scope(30)];
        let mut applied = Vec::new();
        let result = grant_scopes(
            &mut stored,
            vec![numbered_scope(31), numbered_scope(32)],
            |_| Ok(()),
            |scope| {
                applied.push(scope.clone());
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(applied.is_empty());
        assert_eq!(stored.scopes.len() + stored.pending.len(), 31);

        let granted = grant_scopes(
            &mut stored,
            vec![numbered_scope(30), numbered_scope(31)],
            |_| Ok(()),
            |scope| {
                applied.push(scope.clone());
                Ok(())
            },
        );
        assert!(granted.is_ok());
        assert_eq!(applied, vec![numbered_scope(30), numbered_scope(31)]);
        assert_eq!(stored.scopes.len(), MAX_AUTHORIZED_SCOPES);
        assert!(stored.pending.is_empty());
    }

    #[test]
    fn a_rule_is_recorded_before_it_is_applied_and_a_failed_one_stays_revocable() {
        let mut stored = empty_state();
        let mut persisted: Vec<(usize, usize)> = Vec::new();
        let result = grant_scopes(
            &mut stored,
            vec![numbered_scope(1), numbered_scope(2)],
            |state| {
                persisted.push((state.scopes.len(), state.pending.len()));
                Ok(())
            },
            |scope| {
                if scope == &numbered_scope(2) {
                    Err("firewalld failed after the runtime rule".to_string())
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(persisted, vec![(0, 1), (1, 0), (1, 1)]);
        assert_eq!(stored.scopes, vec![numbered_scope(1)]);
        assert_eq!(stored.pending, vec![numbered_scope(2)]);

        let mut removed = Vec::new();
        let revoked = revoke_scopes(&mut stored, |scope| {
            removed.push(scope.clone());
            if scope == &numbered_scope(1) {
                Err("authorization was cancelled".to_string())
            } else {
                Ok(())
            }
        });
        assert!(revoked.is_err());
        assert_eq!(removed, vec![numbered_scope(1), numbered_scope(2)]);
        assert_eq!(stored.scopes, vec![numbered_scope(1)]);
        assert!(stored.pending.is_empty());
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
        let Ok(HelperRequest::Apply { scope, grant }) =
            parse_privileged_helper_request(&mut arguments)
        else {
            panic!("valid helper scope");
        };
        assert!(grant);
        assert_eq!(scope.firewall, FirewallKind::Ufw);
        assert_eq!(scope.local_address, Ipv4Addr::new(192, 168, 1, 66));

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
