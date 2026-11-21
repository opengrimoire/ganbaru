//! Explicit Linux firewall access for the private-LAN handoff coordinator.
//!
//! The coordinator asks once for every rule it needs: TCP on its listener port from the local
//! network of each address it reports, for IPv4 and IPv6, and UDP 5353 for device discovery from
//! the same networks to the multicast DNS groups. Overlay interfaces get a rule qualified by the
//! interface with the overlay block as source. Rules carry a fixed comment, so the root helper
//! can also find and remove every rule the app added when the package is removed.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::str::FromStr;

use crate::dial::{
    DialAddressClass, ipv4_lan_block, ipv4_mask, ipv6_lan_block, ipv6_mask, reportable_address,
};
use crate::discovery::MDNS_PORT;
use crate::protocol::{COORDINATOR_PORT, DEVELOPMENT_COORDINATOR_PORT};

const STATE_FILE: &str = "vault-handoff-network-access.json";
const STATE_SCHEMA_VERSION: u32 = 3;
const RULE_COMMENT: &str = "Ganbaru device linking";
const MAX_AUTHORIZED_SCOPES: usize = 64;
/// Networks remembered per scope. Beyond it the oldest is forgotten and asks again.
const MAX_ALLOWED_NETWORKS: usize = 16;
const MDNS_IPV4_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_IPV6_GROUP: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb);
/// Source of IPv6 multicast DNS queries, which hosts send from their link-local address.
const IPV6_LINK_LOCAL_SOURCE: NetworkPrefix = NetworkPrefix {
    address: IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0)),
    length: 10,
};
/// `RTF_UP` and `RTF_GATEWAY` route flags in `/proc/net/route` and `/proc/net/ipv6_route`.
const ROUTE_UP: u32 = 0x1;
const ROUTE_GATEWAY: u32 = 0x2;
/// `RTF_REJECT` and `RTF_LOCAL` route flags, which mark routes that reach no neighbor.
const ROUTE_REJECT: u32 = 0x200;
const ROUTE_LOCAL: u32 = 0x8000_0000;
/// `ATF_COM` flag of a resolved neighbor in `/proc/net/arp`.
const NEIGHBOR_COMPLETE: u32 = 0x2;
/// Installed path of the packaged app, the only build that hands rules to the helper.
const PACKAGED_APP_PATH: &str = "/usr/bin/ganbaru";
/// Installed path of the root-run helper that the polkit policy names.
const PACKAGED_HELPER_PATH: &str = "/usr/lib/ganbaru/ganbaru-linking-firewall";
/// File name of the helper that Cargo builds next to a development app in the target folder.
#[cfg(debug_assertions)]
const DEVELOPMENT_HELPER_FILE_NAME: &str = "ganbaru-linking-firewall";
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

impl FirewallKind {
    fn name(self) -> &'static str {
        match self {
            Self::Ufw => "ufw",
            Self::Firewalld => "firewalld",
        }
    }
}

/// Transport protocol of a rule: TCP for the coordinator listener, UDP for discovery.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum RuleProtocol {
    Tcp,
    Udp,
}

impl RuleProtocol {
    fn name(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "tcp" => Some(Self::Tcp),
            "udp" => Some(Self::Udp),
            _ => None,
        }
    }
}

/// An aligned network prefix: an address with every bit past `length` cleared. It is written
/// as `address/length`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NetworkPrefix {
    address: IpAddr,
    length: u8,
}

impl NetworkPrefix {
    /// The prefix of `length` bits that holds `address`, or `None` past the family's width.
    fn containing(address: IpAddr, length: u8) -> Option<Self> {
        let address = match address {
            IpAddr::V4(address) if length <= 32 => IpAddr::V4(Ipv4Addr::from(
                u32::from(address) & ipv4_mask(u32::from(length)),
            )),
            IpAddr::V6(address) if length <= 128 => IpAddr::V6(Ipv6Addr::from(
                u128::from(address) & ipv6_mask(u32::from(length)),
            )),
            _ => return None,
        };
        Some(Self { address, length })
    }

    /// The prefix that holds only `address`.
    fn host(address: IpAddr) -> Self {
        Self {
            address,
            length: match address {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            },
        }
    }

    fn is_host(&self) -> bool {
        *self == Self::host(self.address)
    }

    /// Whether `address` lies in this prefix. Addresses of the other family never do.
    fn contains(&self, address: IpAddr) -> bool {
        Self::containing(address, self.length) == Some(*self)
    }

    /// Whether every address of `other` lies in this prefix.
    fn covers(&self, other: &Self) -> bool {
        other.length >= self.length && self.contains(other.address)
    }

    /// The address form for firewall commands: a host as its bare address, the form UFW prints
    /// and earlier builds wrote, and any other prefix as `address/length`.
    fn rule_text(&self) -> String {
        if self.is_host() {
            self.address.to_string()
        } else {
            self.to_string()
        }
    }

    /// Parses a rule address that a firewall prints either as a prefix or as a bare host.
    fn parse_rule_address(value: &str) -> Option<Self> {
        if value.contains('/') {
            value.parse().ok()
        } else {
            value.parse().ok().map(Self::host)
        }
    }
}

impl fmt::Display for NetworkPrefix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.address, self.length)
    }
}

impl FromStr for NetworkPrefix {
    type Err = String;

    /// Parses `address/length`, refusing an address with bits set past the prefix.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("the network prefix {value:?} is invalid");
        let (address, length) = value.split_once('/').ok_or_else(invalid)?;
        let address = address.parse::<IpAddr>().map_err(|_| invalid())?;
        let length = length.parse::<u8>().map_err(|_| invalid())?;
        Self::containing(address, length)
            .filter(|prefix| prefix.address == address)
            .ok_or_else(invalid)
    }
}

impl Serialize for NetworkPrefix {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for NetworkPrefix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

/// One firewall rule the app adds: inbound `protocol` traffic on `interface` from `source` to
/// `destination` and `port`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthorizedScope {
    firewall: FirewallKind,
    protocol: RuleProtocol,
    interface: String,
    /// The local network prefix for the listener, or the multicast DNS group for discovery.
    destination: NetworkPrefix,
    source: NetworkPrefix,
    port: u16,
    #[serde(default)]
    zone: Option<String>,
}

/// A scope whose firewall rule is in place, and the networks the user allowed it on.
///
/// A rule matches addresses, not networks, so another network that hands out the same
/// interface and prefixes would reach the coordinator through it. The coordinator therefore
/// admits LAN peers only on networks recorded here; see [`allowed_on`].
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

impl NetworkAccessStatus {
    /// The access state.
    pub fn state(&self) -> NetworkAccessState {
        self.state
    }
}

/// Whether UFW or firewalld is active on this device. Checking firewalld starts a process.
pub fn has_active_firewall() -> bool {
    !active_firewalls().is_empty()
}

pub fn not_required() -> NetworkAccessStatus {
    NetworkAccessStatus {
        state: NetworkAccessState::NotRequired,
    }
}

/// A local address and the interface that holds it, as the interface list reports them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalInterfaceAddress {
    /// Interface name, such as `wlp2s0` or `tailscale0`.
    pub interface: String,
    pub address: IpAddr,
    /// Prefix length the interface assigns with the address.
    pub prefix_len: u8,
}

/// A scope this device needs now, and the identity of the network its interface reaches.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    scope: AuthorizedScope,
    network: Option<String>,
}

/// The rule addresses for one local address.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AddressRules {
    interface: String,
    listener_destination: NetworkPrefix,
    listener_source: NetworkPrefix,
    /// Source of the discovery rule. `None` on an overlay interface, which carries no
    /// multicast.
    discovery_source: Option<NetworkPrefix>,
}

/// An active firewall and whether it filters IPv6 too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ActiveFirewall {
    kind: FirewallKind,
    ipv6: bool,
}

/// An up route of either route table that reaches a neighbor or gateway.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Route {
    destination: NetworkPrefix,
    interface: String,
    gateway: bool,
}

/// The firewall access state for peers reaching the coordinator on `addresses` and `port`.
pub fn status(
    config_dir: &Path,
    addresses: &[LocalInterfaceAddress],
    port: u16,
) -> NetworkAccessStatus {
    let Ok(candidates) = candidates(addresses, port) else {
        return NetworkAccessStatus {
            state: NetworkAccessState::ManualActionRequired,
        };
    };
    if candidates.is_empty() {
        return not_required();
    }
    let Ok(stored) = read_state(config_dir) else {
        return NetworkAccessStatus {
            state: NetworkAccessState::ManualActionRequired,
        };
    };
    NetworkAccessStatus {
        state: access_state(&stored, &candidates, || {
            let scopes: Vec<AuthorizedScope> = candidates
                .iter()
                .map(|candidate| candidate.scope.clone())
                .collect();
            can_request_authorization(&scopes)
        }),
    }
}

/// Whether the coordinator may accept LAN peers that reach it on `local_address` and `port`
/// now, given the current interface `addresses`. It refuses them only where [`status`] asks the
/// user to confirm the current network, so a refusal always comes with an action that lifts it.
/// A local address missing from `addresses` has no rule to confirm and admits.
pub fn admits_lan_peers(
    config_dir: &Path,
    addresses: &[LocalInterfaceAddress],
    local_address: IpAddr,
    port: u16,
) -> bool {
    let local_address = local_address.to_canonical();
    let matching: Vec<LocalInterfaceAddress> = addresses
        .iter()
        .filter(|address| address.address.to_canonical() == local_address)
        .cloned()
        .collect();
    matching.is_empty()
        || status(config_dir, &matching, port).state
            != NetworkAccessState::NetworkConfirmationRequired
}

/// The access state for the scopes this device needs now. Only listener scopes count: a missing
/// discovery rule degrades discovery but never blocks linking. Authorization is only checked
/// when a rule is missing, because confirming a network with every rule in place needs none.
fn access_state(
    stored: &StoredNetworkAccess,
    candidates: &[Candidate],
    can_request_authorization: impl FnOnce() -> bool,
) -> NetworkAccessState {
    let mut granted = Vec::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.scope.protocol == RuleProtocol::Tcp)
    {
        match stored
            .scopes
            .iter()
            .find(|saved| saved.rule == candidate.scope)
        {
            Some(saved) => granted.push((saved, candidate.network.as_deref())),
            None if can_request_authorization() => {
                return NetworkAccessState::AuthorizationRequired;
            }
            None => return NetworkAccessState::ManualActionRequired,
        }
    }
    if granted
        .iter()
        .all(|(saved, network)| allowed_on(saved, *network))
    {
        NetworkAccessState::Granted
    } else {
        NetworkAccessState::NetworkConfirmationRequired
    }
}

/// Whether the user allowed `scope` on the network identified by `network`. A network without
/// an identity cannot be told apart from an allowed one, so it counts as allowed; a scope with
/// no recorded networks is allowed on no network that has one.
fn allowed_on(scope: &GrantedScope, network: Option<&str>) -> bool {
    network.is_none_or(|network| scope.networks.iter().any(|known| known == network))
}

/// Adds the rules peers need to reach the coordinator on `addresses` and `port`, and allows the
/// current networks for the rules already in place.
pub fn grant(
    config_dir: &Path,
    addresses: &[LocalInterfaceAddress],
    port: u16,
) -> Result<NetworkAccessStatus, String> {
    let candidates = candidates(addresses, port)?;
    if candidates.is_empty() {
        return Ok(not_required());
    }
    let mut stored = read_state(config_dir)?;
    let missing: Vec<AuthorizedScope> = candidates
        .iter()
        .filter(|candidate| {
            !stored
                .scopes
                .iter()
                .any(|saved| saved.rule == candidate.scope)
        })
        .map(|candidate| candidate.scope.clone())
        .collect();
    if !missing.is_empty() && !can_request_authorization(&missing) {
        return Err("automatic Linux firewall authorization is unavailable".to_string());
    }
    grant_scopes(
        &mut stored,
        candidates,
        |state| persist_state(config_dir, state),
        |scopes| apply_scopes(scopes, true),
    )?;
    Ok(NetworkAccessStatus {
        state: NetworkAccessState::Granted,
    })
}

/// Applies the rules of `candidates` that `stored` does not hold yet and records each
/// candidate's network as allowed.
///
/// Capacity is checked before any rule changes. The missing scopes are persisted as pending,
/// applied in one authorized call so the flow asks for the administrator password once, and
/// moved to the granted scopes after, so a failure or crash between the two never leaves a root
/// rule without a record that revoking can find. A candidate already in place only gains the
/// network, with no firewall change.
fn grant_scopes(
    stored: &mut StoredNetworkAccess,
    candidates: Vec<Candidate>,
    mut persist: impl FnMut(&StoredNetworkAccess) -> Result<(), String>,
    apply: impl FnOnce(&[AuthorizedScope]) -> Result<(), String>,
) -> Result<(), String> {
    let (present, missing): (Vec<Candidate>, Vec<Candidate>) =
        candidates.into_iter().partition(|candidate| {
            stored
                .scopes
                .iter()
                .any(|saved| saved.rule == candidate.scope)
        });
    let new_records: Vec<AuthorizedScope> = missing
        .iter()
        .map(|candidate| &candidate.scope)
        .filter(|scope| !stored.pending.contains(scope))
        .cloned()
        .collect();
    if stored.scopes.len() + stored.pending.len() + new_records.len() > MAX_AUTHORIZED_SCOPES {
        return Err("too many saved Linux network access scopes".to_string());
    }
    let mut confirmed = false;
    for candidate in &present {
        if let Some(saved) = stored
            .scopes
            .iter_mut()
            .find(|saved| saved.rule == candidate.scope)
        {
            confirmed |= allow_network(saved, candidate.network.as_deref());
        }
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
    let scopes: Vec<AuthorizedScope> = missing
        .iter()
        .map(|candidate| candidate.scope.clone())
        .collect();
    apply(&scopes)?;
    stored.pending.retain(|pending| !scopes.contains(pending));
    for candidate in missing {
        let mut granted = GrantedScope {
            rule: candidate.scope,
            networks: Vec::new(),
        };
        allow_network(&mut granted, candidate.network.as_deref());
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

/// The scopes peers need to reach the coordinator on `addresses` and `port` through the active
/// firewalls, listener scopes first, each with the identity of its interface's network.
fn candidates(addresses: &[LocalInterfaceAddress], port: u16) -> Result<Vec<Candidate>, String> {
    let routes = read_routes()?;
    let rules: Vec<AddressRules> = addresses
        .iter()
        .filter_map(|address| address_rules(address, &routes))
        .collect();
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let firewalls = active_firewalls();
    if firewalls.is_empty() {
        return Ok(Vec::new());
    }
    let mut networks: Vec<(String, Option<String>)> = Vec::new();
    Ok(rule_scopes(&rules, port, &firewalls, firewalld_zone)
        .into_iter()
        .map(|scope| {
            let network = match networks
                .iter()
                .find(|(interface, _)| *interface == scope.interface)
            {
                Some((_, network)) => network.clone(),
                None => {
                    let network = current_network(&scope.interface);
                    networks.push((scope.interface.clone(), network.clone()));
                    network
                }
            };
            Candidate { scope, network }
        })
        .collect())
}

/// The rule addresses for `address`, or `None` when it is not a reportable LAN or overlay
/// address on a usable interface.
///
/// An overlay address (in `100.64.0.0/10`, or unique local on an interface with no default
/// route) admits the whole overlay block to the address's own prefix, because overlays such
/// as Tailscale keep their routes outside the main table. A LAN address opens its connected
/// network, from the longest connected route on its interface or else the interface prefix,
/// narrowed to its policy block, so a new lease inside the same network needs no new rule.
fn address_rules(address: &LocalInterfaceAddress, routes: &[Route]) -> Option<AddressRules> {
    let local = address.address.to_canonical();
    if !is_valid_interface(&address.interface) {
        return None;
    }
    let class = reportable_address(local).ok()?;
    let block = lan_block(local)?;
    let unique_local = matches!(local, IpAddr::V6(local) if local.is_unique_local());
    if class == DialAddressClass::Overlay
        || (unique_local && !has_default_route(routes, &address.interface))
    {
        return Some(AddressRules {
            interface: address.interface.clone(),
            listener_destination: NetworkPrefix::containing(
                local,
                address.prefix_len.max(block.length),
            )?,
            listener_source: block,
            discovery_source: None,
        });
    }
    let length = connected_prefix(routes, &address.interface, local)
        .unwrap_or(address.prefix_len)
        .max(block.length);
    let network = NetworkPrefix::containing(local, length)?;
    Some(AddressRules {
        interface: address.interface.clone(),
        listener_destination: network,
        listener_source: network,
        discovery_source: Some(match local {
            IpAddr::V4(_) => network,
            IpAddr::V6(_) => IPV6_LINK_LOCAL_SOURCE,
        }),
    })
}

/// The policy block that holds `address`, the widest range a rule built from it may open.
fn lan_block(address: IpAddr) -> Option<NetworkPrefix> {
    let (network, length) = match address {
        IpAddr::V4(address) => {
            ipv4_lan_block(address).map(|(network, length)| (IpAddr::V4(network), length))?
        }
        IpAddr::V6(address) => {
            ipv6_lan_block(address).map(|(network, length)| (IpAddr::V6(network), length))?
        }
    };
    NetworkPrefix::containing(network, u8::try_from(length).ok()?)
}

/// The multicast DNS group of the family of `address`.
fn mdns_group(address: IpAddr) -> NetworkPrefix {
    NetworkPrefix::host(match address {
        IpAddr::V4(_) => IpAddr::V4(MDNS_IPV4_GROUP),
        IpAddr::V6(_) => IpAddr::V6(MDNS_IPV6_GROUP),
    })
}

/// The scopes for `rules` in each of `firewalls`: every listener scope, then every discovery
/// scope, once each. A firewall that does not filter IPv6 gets no IPv6 scope, since its rule
/// would fail and IPv6 already passes it. `zone_of` names the firewalld zone of an interface.
fn rule_scopes(
    rules: &[AddressRules],
    port: u16,
    firewalls: &[ActiveFirewall],
    mut zone_of: impl FnMut(&str) -> Option<String>,
) -> Vec<AuthorizedScope> {
    let mut zones: Vec<(String, Option<String>)> = Vec::new();
    let mut zone = |interface: &str| {
        if let Some((_, zone)) = zones.iter().find(|(known, _)| known == interface) {
            return zone.clone();
        }
        let zone = zone_of(interface);
        zones.push((interface.to_string(), zone.clone()));
        zone
    };
    let mut scopes: Vec<AuthorizedScope> = Vec::new();
    for protocol in [RuleProtocol::Tcp, RuleProtocol::Udp] {
        for rule in rules {
            let (destination, source, rule_port) = match protocol {
                RuleProtocol::Tcp => (rule.listener_destination, rule.listener_source, port),
                RuleProtocol::Udp => {
                    let Some(source) = rule.discovery_source else {
                        continue;
                    };
                    (mdns_group(source.address), source, MDNS_PORT)
                }
            };
            for firewall in firewalls {
                if source.address.is_ipv6() && !firewall.ipv6 {
                    continue;
                }
                let scope = AuthorizedScope {
                    firewall: firewall.kind,
                    protocol,
                    interface: rule.interface.clone(),
                    destination,
                    source,
                    port: rule_port,
                    zone: match firewall.kind {
                        FirewallKind::Ufw => None,
                        FirewallKind::Firewalld => zone(&rule.interface),
                    },
                };
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
        }
    }
    scopes
}

/// The identity of the network reached through `interface`: the hardware address of its
/// default IPv4 gateway. IPv6 scopes on the interface share it. `None` when the interface has no
/// default gateway or the gateway has no resolved neighbor entry.
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
        let flags = u32::from_str_radix(fields[3], 16).ok()?;
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

/// The routes of the main IPv4 table and of the IPv6 tables. A system without IPv6 has no IPv6
/// table and no IPv6 address that would need one.
fn read_routes() -> Result<Vec<Route>, String> {
    let ipv4 = fs::read_to_string("/proc/net/route")
        .map_err(|error| format!("read Linux IPv4 routes: {error}"))?;
    let ipv6 = match fs::read_to_string("/proc/net/ipv6_route") {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("read Linux IPv6 routes: {error}")),
    };
    let mut routes = parse_ipv4_routes(&ipv4);
    routes.extend(parse_ipv6_routes(&ipv6));
    Ok(routes)
}

/// The usable routes in `/proc/net/route` contents. Rows with an unsafe interface name, a
/// non-contiguous mask, or no up flag are skipped.
fn parse_ipv4_routes(contents: &str) -> Vec<Route> {
    contents
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 8 || !is_valid_interface(fields[0]) {
                return None;
            }
            let flags = u32::from_str_radix(fields[3], 16).ok()?;
            let raw_destination = u32::from_str_radix(fields[1], 16).ok()?;
            let raw_mask = u32::from_str_radix(fields[7], 16).ok()?;
            let length = ipv4_prefix(Ipv4Addr::from(raw_mask.swap_bytes()))?;
            route(
                IpAddr::V4(Ipv4Addr::from(raw_destination.swap_bytes())),
                length,
                flags,
                fields[0],
            )
        })
        .collect()
}

/// The usable routes in `/proc/net/ipv6_route` contents: destination, prefix length, source,
/// source length, next hop, metric, reference count, use count, flags, and interface per row.
fn parse_ipv6_routes(contents: &str) -> Vec<Route> {
    contents
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 10 || fields[0].len() != 32 || !is_valid_interface(fields[9]) {
                return None;
            }
            let destination = Ipv6Addr::from(u128::from_str_radix(fields[0], 16).ok()?);
            let length = u8::from_str_radix(fields[1], 16).ok()?;
            let flags = u32::from_str_radix(fields[8], 16).ok()?;
            route(IpAddr::V6(destination), length, flags, fields[9])
        })
        .collect()
}

/// A route when its flags mark it up and reaching a neighbor or gateway, and its destination
/// is an aligned prefix.
fn route(destination: IpAddr, length: u8, flags: u32, interface: &str) -> Option<Route> {
    if flags & ROUTE_UP == 0 || flags & (ROUTE_REJECT | ROUTE_LOCAL) != 0 {
        return None;
    }
    let prefix = NetworkPrefix::containing(destination, length)?;
    (prefix.address == destination).then(|| Route {
        destination: prefix,
        interface: interface.to_string(),
        gateway: flags & ROUTE_GATEWAY != 0,
    })
}

/// The length of the longest connected route on `interface` that holds `address`. A default
/// route never counts, even without a gateway.
fn connected_prefix(routes: &[Route], interface: &str, address: IpAddr) -> Option<u8> {
    routes
        .iter()
        .filter(|route| {
            !route.gateway
                && route.interface == interface
                && route.destination.length > 0
                && route.destination.contains(address)
        })
        .map(|route| route.destination.length)
        .max()
}

/// Whether `interface` carries a default route of either family.
fn has_default_route(routes: &[Route], interface: &str) -> bool {
    routes
        .iter()
        .any(|route| route.interface == interface && route.destination.length == 0)
}

fn is_valid_interface(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 15
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// The prefix length of a contiguous IPv4 mask.
fn ipv4_prefix(mask: Ipv4Addr) -> Option<u8> {
    let mask = u32::from(mask);
    let prefix = mask.leading_ones();
    u8::try_from(prefix)
        .ok()
        .filter(|_| mask == ipv4_mask(prefix))
}

fn active_firewalls() -> Vec<ActiveFirewall> {
    let mut result = Vec::new();
    if let Ok(contents) = fs::read_to_string("/etc/ufw/ufw.conf")
        && config_value_is_yes(&contents, "ENABLED")
    {
        result.push(ActiveFirewall {
            kind: FirewallKind::Ufw,
            ipv6: fs::read_to_string("/etc/default/ufw")
                .is_ok_and(|contents| config_value_is_yes(&contents, "IPV6")),
        });
    }
    if is_firewalld_running() {
        result.push(ActiveFirewall {
            kind: FirewallKind::Firewalld,
            ipv6: true,
        });
    }
    result
}

/// Whether a `KEY=value` configuration file sets `key` to `yes`, ignoring comments.
fn config_value_is_yes(contents: &str, key: &str) -> bool {
    contents.lines().any(|line| {
        line.split_once('=').is_some_and(|(name, value)| {
            name.trim() == key && value.trim().trim_matches('"') == "yes"
        })
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

/// Adds or removes the rules of `scopes`. The packaged app, and a development build whose helper
/// Cargo built, hand them all to the helper in one authorized call; other builds apply each
/// through the firewall command, which asks for the password once per rule.
fn apply_scopes(scopes: &[AuthorizedScope], grant: bool) -> Result<(), String> {
    for scope in scopes {
        validate_authorized_scope(scope)?;
    }
    if let Some(helper) = trusted_packaged_helper().or_else(development_helper) {
        return apply_with_helper(&helper, scopes, grant);
    }
    apply_each(scopes, grant, |scope| match scope.firewall {
        FirewallKind::Ufw => apply_ufw(scope, grant),
        FirewallKind::Firewalld => apply_firewalld(scope, grant),
    })
}

/// Applies each scope in order and returns the first failure that counts.
///
/// A grant stops at the first failed listener rule, since its scopes stay pending, and only
/// logs a failed discovery rule: discovery is then degraded, while linking through reported
/// addresses still works, and removing the absent rule later succeeds. A revoke attempts every
/// scope so one failure leaves no other rule in place.
fn apply_each(
    scopes: &[AuthorizedScope],
    grant: bool,
    mut apply: impl FnMut(&AuthorizedScope) -> Result<(), String>,
) -> Result<(), String> {
    let mut first_error = None;
    for scope in scopes {
        if let Err(error) = apply(scope) {
            if !grant {
                first_error.get_or_insert(error);
            } else if scope.protocol == RuleProtocol::Udp {
                eprintln!("the device discovery firewall rule was not added: {error}");
            } else {
                return Err(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Returns the packaged helper when this process is the packaged app and the helper is a
/// regular file that only root can replace.
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

/// In a development build, the helper Cargo built next to the running app, so a flow asks for
/// the password once as the packaged app does. The prompt names the helper and every rule, and
/// the helper validates each rule again as root. The developer can replace this binary, which a
/// development build accepts: only a process already running as that user could swap it, and
/// such a process could ask for the password itself.
#[cfg(debug_assertions)]
fn development_helper() -> Option<PathBuf> {
    let app = std::env::current_exe().ok()?.canonicalize().ok()?;
    let helper = app.parent()?.join(DEVELOPMENT_HELPER_FILE_NAME);
    helper.symlink_metadata().ok()?.is_file().then_some(helper)
}

#[cfg(not(debug_assertions))]
fn development_helper() -> Option<PathBuf> {
    None
}

/// Whether only root can change the entry: owned by root and not group or world writable.
fn root_controlled(metadata: &fs::Metadata) -> bool {
    metadata.uid() == 0 && metadata.mode() & 0o022 == 0
}

fn apply_with_helper(helper: &Path, scopes: &[AuthorizedScope], grant: bool) -> Result<(), String> {
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
/// the administrator password once per flow. Each scope is written as its firewall, protocol,
/// interface, destination, source, port, and zone or `-`.
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
        validate_authorized_scope(scope)?;
        arguments.extend([
            scope.firewall.name().to_string(),
            scope.protocol.name().to_string(),
            scope.interface.clone(),
            scope.destination.to_string(),
            scope.source.to_string(),
            scope.port.to_string(),
            scope.zone.clone().unwrap_or_else(|| "-".to_string()),
        ]);
    }
    Ok(arguments)
}

/// Runs the root firewall helper request in the process arguments, if there is one.
///
/// The `ganbaru-linking-firewall` binary calls it. The package policy authorizes only that
/// root-owned helper with `--manage-linking-firewall` as its first argument. All remaining
/// values are parsed and validated again before a fixed firewall executable is invoked, and a
/// grant's global IPv6 source must be an on-link prefix of this host.
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
                        if grant {
                            validate_privileged_grant(&scopes)?;
                        }
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
    /// Removes every rule the app added, found by its shape in the firewall rather than in
    /// per-user state. Package removal runs it as root.
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
    let mut next = |name: &str| {
        arguments
            .next()
            .ok_or_else(|| format!("the Linux network helper {name} is missing"))
    };
    let firewall = match firewall {
        "ufw" => FirewallKind::Ufw,
        "firewalld" => FirewallKind::Firewalld,
        _ => return Err("the Linux network helper firewall is invalid".to_string()),
    };
    let protocol = RuleProtocol::parse(&next("protocol")?)
        .ok_or_else(|| "the Linux network helper protocol is invalid".to_string())?;
    let interface = next("interface")?;
    let destination = next("destination")?.parse::<NetworkPrefix>()?;
    let source = next("source")?.parse::<NetworkPrefix>()?;
    let port = next("port")?
        .parse::<u16>()
        .map_err(|_| "the Linux network helper port is invalid".to_string())?;
    let zone = match next("zone")?.as_str() {
        "-" => None,
        value => Some(value.to_string()),
    };
    let scope = AuthorizedScope {
        firewall,
        protocol,
        interface,
        destination,
        source,
        port,
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

/// Removes every rule carrying the app's shape from UFW and from every firewalld zone, runtime
/// and permanent. Firewalls that are absent or stopped are skipped, and a failed removal does
/// not stop the others. Returns the first failure.
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

/// Scopes of the `ufw show added` lines that carry the app comment, allow a listener or
/// discovery rule of either family, and pass scope validation. Host destinations of earlier
/// builds parse as /32 or /128 prefixes.
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
    let scope = AuthorizedScope {
        firewall: FirewallKind::Ufw,
        protocol: RuleProtocol::parse(protocol?)?,
        interface,
        destination: NetworkPrefix::parse_rule_address(destination?)?,
        source: NetworkPrefix::parse_rule_address(source?)?,
        port: port?.parse().ok()?,
        zone: None,
    };
    validate_authorized_scope(&scope).ok()?;
    Some(scope)
}

/// The scope of a firewalld rich rule in exactly the shape the app adds. Rich rules carry no
/// interface, so the scope has none; only its zone, addresses, protocol, and port reach
/// firewalld.
fn parse_firewalld_rich_rule(zone: &str, rule: &str) -> Option<AuthorizedScope> {
    let rest = rule.trim().strip_prefix("rule family=\"")?;
    let (family, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" source address=\"")?;
    let (source, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" destination address=\"")?;
    let (destination, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" port port=\"")?;
    let (port, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" protocol=\"")?;
    let (protocol, rest) = rest.split_once('"')?;
    if rest != " accept" {
        return None;
    }
    let source = NetworkPrefix::parse_rule_address(source)?;
    if family != firewalld_family(source.address) {
        return None;
    }
    let protocol = RuleProtocol::parse(protocol)?;
    let destination = NetworkPrefix::parse_rule_address(destination)?;
    let port = port.parse::<u16>().ok()?;
    validate_rule(protocol, destination, source, port).ok()?;
    is_valid_zone(zone).then(|| AuthorizedScope {
        firewall: FirewallKind::Firewalld,
        protocol,
        interface: String::new(),
        destination,
        source,
        port,
        zone: Some(zone.to_string()),
    })
}

fn firewalld_family(address: IpAddr) -> &'static str {
    match address {
        IpAddr::V4(_) => "ipv4",
        IpAddr::V6(_) => "ipv6",
    }
}

fn validate_authorized_scope(scope: &AuthorizedScope) -> Result<(), String> {
    if !is_valid_interface(&scope.interface) {
        return Err("saved Linux network access scope is invalid".to_string());
    }
    validate_rule(scope.protocol, scope.destination, scope.source, scope.port)?;
    match scope.firewall {
        FirewallKind::Ufw if scope.zone.is_none() => Ok(()),
        FirewallKind::Firewalld if scope.zone.as_deref().is_some_and(is_valid_zone) => Ok(()),
        _ => Err("saved Linux firewall scope is inconsistent".to_string()),
    }
}

/// Checks that a rule has one of the two shapes the app adds: TCP to a coordinator port, or UDP
/// to the multicast DNS port and group. Either way the source is a private, link-local,
/// overlay, or on-link global prefix inside its policy block, and a listener destination lies
/// inside its source.
fn validate_rule(
    protocol: RuleProtocol,
    destination: NetworkPrefix,
    source: NetworkPrefix,
    port: u16,
) -> Result<(), String> {
    let block = lan_block(source.address)
        .ok_or_else(|| "saved Linux network source is not a local network".to_string())?;
    if source.length < block.length {
        return Err("saved Linux network source extends past its private block".to_string());
    }
    match protocol {
        RuleProtocol::Tcp => {
            if !matches!(port, COORDINATOR_PORT | DEVELOPMENT_COORDINATOR_PORT) {
                return Err("saved Linux network access port is invalid".to_string());
            }
            if !source.covers(&destination) {
                return Err(
                    "saved Linux network source does not contain its local network".to_string(),
                );
            }
        }
        RuleProtocol::Udp => {
            if port != MDNS_PORT || destination != mdns_group(source.address) {
                return Err("saved Linux discovery rule is invalid".to_string());
            }
        }
    }
    Ok(())
}

/// Whether `source` is a global IPv6 prefix, which only an on-link network of this host may
/// open.
fn is_global_ipv6_source(source: &NetworkPrefix) -> bool {
    source.address.is_ipv6()
        && reportable_address(source.address) == Ok(DialAddressClass::GlobalIpv6)
}

/// Checks that every global IPv6 source among `scopes` lies inside one of `own_prefixes`, the
/// on-link prefixes of this host's global IPv6 addresses. Other sources are left to
/// [`validate_rule`].
fn validate_own_global_sources(
    scopes: &[AuthorizedScope],
    own_prefixes: &[NetworkPrefix],
) -> Result<(), String> {
    let foreign = scopes.iter().any(|scope| {
        is_global_ipv6_source(&scope.source)
            && !own_prefixes
                .iter()
                .any(|prefix| prefix.covers(&scope.source))
    });
    if foreign {
        return Err("the Linux network source is not a network of this device".to_string());
    }
    Ok(())
}

/// The on-link prefixes of the global IPv6 addresses in `/proc/net/if_inet6` contents:
/// address, interface index, prefix length, scope, flags, and interface name per row. Each
/// address contributes its policy block, the same prefix a rule built from it opens at most.
fn parse_own_global_ipv6_prefixes(contents: &str) -> Vec<NetworkPrefix> {
    contents
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 6 || fields[0].len() != 32 {
                return None;
            }
            let address = IpAddr::V6(Ipv6Addr::from(u128::from_str_radix(fields[0], 16).ok()?));
            (reportable_address(address) == Ok(DialAddressClass::GlobalIpv6))
                .then(|| lan_block(address))
                .flatten()
        })
        .collect()
}

/// The on-link prefixes of this host's global IPv6 addresses, read by the helper itself so a
/// caller cannot widen them. A system without IPv6 has none.
fn own_global_ipv6_prefixes() -> Result<Vec<NetworkPrefix>, String> {
    match fs::read_to_string("/proc/net/if_inet6") {
        Ok(contents) => Ok(parse_own_global_ipv6_prefixes(&contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("read Linux IPv6 addresses: {error}")),
    }
}

/// Refuses a grant whose global IPv6 source is not an on-link prefix of this host. Revokes
/// are not checked, so a rule for a prefix the host has since lost can still be removed.
fn validate_privileged_grant(scopes: &[AuthorizedScope]) -> Result<(), String> {
    if !scopes
        .iter()
        .any(|scope| is_global_ipv6_source(&scope.source))
    {
        return Ok(());
    }
    validate_own_global_sources(scopes, &own_global_ipv6_prefixes()?)
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
        scope.protocol.name().to_string(),
        "from".to_string(),
        scope.source.rule_text(),
        "to".to_string(),
        scope.destination.rule_text(),
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

/// The firewalld rich rule of `scope`.
fn firewalld_rich_rule(scope: &AuthorizedScope) -> String {
    format!(
        "rule family=\"{}\" source address=\"{}\" destination address=\"{}\" port port=\"{}\" protocol=\"{}\" accept",
        firewalld_family(scope.source.address),
        scope.source,
        scope.destination,
        scope.port,
        scope.protocol.name()
    )
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
    let rule = firewalld_rich_rule(scope);
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

/// Reads the saved scopes. A file from another schema version is refused rather than
/// converted; development state from an earlier build is removed by hand, and package removal
/// removes the rules it described.
fn read_state(config_dir: &Path) -> Result<StoredNetworkAccess, String> {
    let path = state_path(config_dir);
    if !path.exists() {
        return Ok(empty_state());
    }
    let bytes = fs::read(&path).map_err(|error| format!("read network access state: {error}"))?;
    let version = serde_json::from_slice::<StateVersion>(&bytes)
        .map_err(|error| format!("parse network access state: {error}"))?;
    if version.schema_version != STATE_SCHEMA_VERSION {
        return Err(format!(
            "network access state version is unsupported; delete {} from the app config folder",
            path.display()
        ));
    }
    let state: StoredNetworkAccess = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse network access state: {error}"))?;
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

/// The schema version of a saved state file, read before its scopes so that a file of another
/// version is reported as such.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateVersion {
    schema_version: u32,
}

fn persist_state(config_dir: &Path, state: &StoredNetworkAccess) -> Result<(), String> {
    let path = state_path(config_dir);
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("serialize network access state: {error}"))?;
    super::pairing::write_private_file_atomically(&path, &bytes)
}

#[cfg(test)]
mod tests;
