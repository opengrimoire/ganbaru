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
mod tests {
    use super::*;

    const IPV4_ROUTES: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlp2s0\t00000000\tFE01A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
wlp2s0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
wlp2s0\tFE01A8C0\t00000000\t0005\t0\t0\t600\tFFFFFFFF\t0\t0\t0
eth0\t0000000A\t00000000\t0001\t0\t0\t0\t000000FE\t0\t0\t0
eth1\t0000000A\t00000000\t0001\t0\t0\t0\t00FF00FF\t0\t0\t0
eth2\t0002A8C0\t00000000\t0000\t0\t0\t0\t00FFFFFF\t0\t0\t0
bad/if\t0003A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0
";

    const IPV6_ROUTES: &str = "\
2806101e000e89700000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000258 00000001 00000000 00400001   wlp6s0
fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001   wlp6s0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000258 00000009 00000000 00400003   wlp6s0
00000000000000000000000000000001 80 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 0000000b 00000000 80200001       lo
2806101e000e89704a89e7fffecf5d55 80 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 00000003 00000000 80200001   wlp6s0
ff000000000000000000000000000000 08 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000008 00000000 00000001   wlp6s0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo
fd123456000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001      wg0
";

    const HOME: &str = "a4:2b:b0:11:22:33";
    const CAFE: &str = "f0:9f:c2:44:55:66";
    const GLOBAL: &str = "2806:101e:e:8970:4a89:e7ff:fecf:5d55";

    fn prefix(value: &str) -> NetworkPrefix {
        value.parse().expect("network prefix")
    }

    fn host(value: &str) -> NetworkPrefix {
        NetworkPrefix::host(value.parse().expect("address"))
    }

    fn routes() -> Vec<Route> {
        let mut routes = parse_ipv4_routes(IPV4_ROUTES);
        routes.extend(parse_ipv6_routes(IPV6_ROUTES));
        routes
    }

    fn local(interface: &str, address: &str, prefix_len: u8) -> LocalInterfaceAddress {
        LocalInterfaceAddress {
            interface: interface.to_string(),
            address: address.parse().expect("address"),
            prefix_len,
        }
    }

    fn rules_for(interface: &str, address: &str, prefix_len: u8) -> Option<AddressRules> {
        address_rules(&local(interface, address, prefix_len), &routes())
    }

    fn tcp(interface: &str, destination: &str, source: &str) -> AuthorizedScope {
        AuthorizedScope {
            firewall: FirewallKind::Ufw,
            protocol: RuleProtocol::Tcp,
            interface: interface.to_string(),
            destination: NetworkPrefix::parse_rule_address(destination).expect("destination"),
            source: prefix(source),
            port: COORDINATOR_PORT,
            zone: None,
        }
    }

    fn udp(interface: &str, group: &str, source: &str) -> AuthorizedScope {
        AuthorizedScope {
            protocol: RuleProtocol::Udp,
            destination: host(group),
            port: MDNS_PORT,
            ..tcp(interface, group, source)
        }
    }

    fn numbered_scope(index: u8) -> AuthorizedScope {
        let network = format!("192.168.{index}.0/24");
        tcp("wlp2s0", &network, &network)
    }

    fn candidate(index: u8, network: Option<&str>) -> Candidate {
        Candidate {
            scope: numbered_scope(index),
            network: network.map(str::to_string),
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

    fn helper_request(arguments: &[&str]) -> Result<HelperRequest, String> {
        parse_privileged_helper_request(&mut arguments.iter().map(|value| value.to_string()))
    }

    fn scratch_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "ganbaru-network-access-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn prefixes_are_aligned_and_hosts_print_bare() {
        assert_eq!(prefix("192.168.1.0/24").to_string(), "192.168.1.0/24");
        assert_eq!(prefix("fe80::/10").to_string(), "fe80::/10");
        for invalid in [
            "192.168.1.66/24",
            "10.0.0.0/33",
            "::/129",
            "fe80::1/10",
            "192.168.1.0",
            "192.168.1.0/",
            "example/24",
        ] {
            assert!(invalid.parse::<NetworkPrefix>().is_err(), "{invalid}");
        }
        assert_eq!(
            NetworkPrefix::containing("192.168.1.66".parse().unwrap(), 24),
            Some(prefix("192.168.1.0/24"))
        );
        assert_eq!(
            NetworkPrefix::containing("10.0.0.1".parse().unwrap(), 33),
            None
        );

        assert_eq!(host("192.168.1.66").rule_text(), "192.168.1.66");
        assert_eq!(host("ff02::fb").rule_text(), "ff02::fb");
        assert_eq!(prefix("192.168.1.66/32"), host("192.168.1.66"));
        assert_eq!(prefix("10.0.0.0/8").rule_text(), "10.0.0.0/8");
        assert_eq!(
            NetworkPrefix::parse_rule_address("192.168.1.66"),
            Some(host("192.168.1.66"))
        );
        assert_eq!(
            NetworkPrefix::parse_rule_address("2806:101e:e:8970::/64"),
            Some(prefix("2806:101e:e:8970::/64"))
        );
        assert_eq!(NetworkPrefix::parse_rule_address("192.168.1.66/24"), None);

        assert!(prefix("192.168.0.0/16").covers(&prefix("192.168.1.0/24")));
        assert!(!prefix("192.168.1.0/24").covers(&prefix("192.168.0.0/16")));
        assert!(!prefix("0.0.0.0/0").contains("::1".parse().unwrap()));

        let json = serde_json::to_string(&prefix("fc00::/7")).unwrap();
        assert_eq!(json, "\"fc00::/7\"");
        assert_eq!(
            serde_json::from_str::<NetworkPrefix>(&json).unwrap(),
            prefix("fc00::/7")
        );
        assert!(serde_json::from_str::<NetworkPrefix>("\"10.1.0.0/8\"").is_err());
    }

    #[test]
    fn route_tables_keep_only_up_routes_that_reach_a_neighbor() {
        let route = |destination: &str, interface: &str, gateway: bool| Route {
            destination: prefix(destination),
            interface: interface.to_string(),
            gateway,
        };
        assert_eq!(
            parse_ipv4_routes(IPV4_ROUTES),
            vec![
                route("0.0.0.0/0", "wlp2s0", true),
                route("192.168.1.0/24", "wlp2s0", false),
                route("192.168.1.254/32", "wlp2s0", false),
                route("10.0.0.0/7", "eth0", false),
            ],
            "non-contiguous masks, down routes, and unsafe interface names are skipped"
        );
        assert_eq!(
            parse_ipv6_routes(IPV6_ROUTES),
            vec![
                route("2806:101e:e:8970::/64", "wlp6s0", false),
                route("fe80::/64", "wlp6s0", false),
                route("::/0", "wlp6s0", true),
                route("ff00::/8", "wlp6s0", false),
                route("fd12:3456::/64", "wg0", false),
            ],
            "local and reject routes are skipped"
        );
        assert!(parse_ipv6_routes("2806101e 40 x\n").is_empty());

        let routes = routes();
        assert!(has_default_route(&routes, "wlp6s0"));
        assert!(has_default_route(&routes, "wlp2s0"));
        assert!(!has_default_route(&routes, "wg0"));
        assert_eq!(
            connected_prefix(&routes, "wlp2s0", "192.168.1.66".parse().unwrap()),
            Some(24)
        );
        assert_eq!(
            connected_prefix(&routes, "wlp2s0", "192.168.1.254".parse().unwrap()),
            Some(32)
        );
        assert_eq!(
            connected_prefix(&routes, "wlp2s0", "10.1.2.3".parse().unwrap()),
            None,
            "a default route is never a connected network"
        );
        assert_eq!(
            connected_prefix(&routes, "wlp6s0", GLOBAL.parse().unwrap()),
            Some(64)
        );
    }

    #[test]
    fn lan_addresses_open_their_connected_network_within_their_block() {
        let ipv4 = rules_for("wlp2s0", "192.168.1.66", 24).expect("IPv4 LAN rules");
        assert_eq!(ipv4.listener_destination, prefix("192.168.1.0/24"));
        assert_eq!(ipv4.listener_source, prefix("192.168.1.0/24"));
        assert_eq!(ipv4.discovery_source, Some(prefix("192.168.1.0/24")));

        let wide = rules_for("eth0", "10.1.2.3", 7).expect("wide connected route");
        assert_eq!(
            wide.listener_source,
            prefix("10.0.0.0/8"),
            "a connected route broader than its private block is narrowed to the block"
        );

        let global = rules_for("wlp6s0", GLOBAL, 64).expect("global IPv6 rules");
        assert_eq!(global.listener_destination, prefix("2806:101e:e:8970::/64"));
        assert_eq!(global.listener_source, prefix("2806:101e:e:8970::/64"));
        assert_eq!(global.discovery_source, Some(prefix("fe80::/10")));

        let dhcpv6 = rules_for("wlp6s0", "2806:101e:e:8970::1234", 128).expect("DHCPv6");
        assert_eq!(
            dhcpv6.listener_source,
            prefix("2806:101e:e:8970::/64"),
            "a /128 lease opens the connected route that holds it"
        );

        let unrouted = rules_for("eth9", "2806:101e:e:8970::1", 48).expect("unrouted");
        assert_eq!(
            unrouted.listener_source,
            prefix("2806:101e:e:8970::/64"),
            "a global prefix never extends past its on-link /64"
        );

        let routed_ula = rules_for("wlp6s0", "fd12:3456::1", 64).expect("routed ULA");
        assert_eq!(routed_ula.listener_source, prefix("fd12:3456::/64"));
        assert_eq!(routed_ula.discovery_source, Some(prefix("fe80::/10")));

        for (interface, address, prefix_len) in [
            ("wlp6s0", "fe80::4a89:e7ff:fecf:5d55", 64),
            ("lo", "127.0.0.1", 8),
            ("lo", "::1", 128),
            ("eth0", "8.8.8.8", 24),
            ("eth0", "2001:db8::1", 64),
            ("bad/if", "192.168.1.66", 24),
            ("wlp2s0", "0.0.0.0", 0),
        ] {
            assert_eq!(
                rules_for(interface, address, prefix_len),
                None,
                "{address} on {interface}"
            );
        }
    }

    #[test]
    fn overlay_addresses_admit_their_block_on_their_interface_without_discovery() {
        let tailscale = rules_for("tailscale0", "100.101.2.3", 32).expect("Tailscale");
        assert_eq!(tailscale.interface, "tailscale0");
        assert_eq!(tailscale.listener_destination, host("100.101.2.3"));
        assert_eq!(tailscale.listener_source, prefix("100.64.0.0/10"));
        assert_eq!(tailscale.discovery_source, None);

        let tailscale_ipv6 =
            rules_for("tailscale0", "fd7a:115c:a1e0::4a01:c903", 128).expect("Tailscale IPv6");
        assert_eq!(
            tailscale_ipv6.listener_destination,
            host("fd7a:115c:a1e0::4a01:c903")
        );
        assert_eq!(tailscale_ipv6.listener_source, prefix("fc00::/7"));
        assert_eq!(tailscale_ipv6.discovery_source, None);

        let wireguard = rules_for("wg0", "fd12:3456::1", 64).expect("unique local overlay");
        assert_eq!(
            wireguard.listener_destination,
            prefix("fd12:3456::/64"),
            "unique local without a default route is an overlay"
        );
        assert_eq!(wireguard.listener_source, prefix("fc00::/7"));
        assert_eq!(wireguard.discovery_source, None);
    }

    #[test]
    fn scopes_list_listeners_first_once_each_for_every_active_firewall() {
        let addresses = [
            local("wlp2s0", "192.168.1.66", 24),
            local("wlp6s0", GLOBAL, 64),
            local("wlp6s0", "2806:101e:e:8970:1c2d:3e4f:5a6b:7c8d", 64),
            local("tailscale0", "100.101.2.3", 32),
        ];
        let routes = routes();
        let rules: Vec<AddressRules> = addresses
            .iter()
            .filter_map(|address| address_rules(address, &routes))
            .collect();
        assert_eq!(rules.len(), 4);

        let ipv4_only = rule_scopes(
            &rules,
            COORDINATOR_PORT,
            &[ActiveFirewall {
                kind: FirewallKind::Ufw,
                ipv6: false,
            }],
            |_| panic!("UFW has no zones"),
        );
        assert_eq!(
            ipv4_only,
            vec![
                tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24"),
                tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
                udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
            ],
            "a UFW without IPv6 filtering gets no IPv6 rule"
        );

        let mut zone_lookups = Vec::new();
        let both = rule_scopes(
            &rules,
            DEVELOPMENT_COORDINATOR_PORT,
            &[
                ActiveFirewall {
                    kind: FirewallKind::Ufw,
                    ipv6: true,
                },
                ActiveFirewall {
                    kind: FirewallKind::Firewalld,
                    ipv6: true,
                },
            ],
            |interface| {
                zone_lookups.push(interface.to_string());
                Some("home".to_string())
            },
        );
        assert_eq!(zone_lookups, ["wlp2s0", "wlp6s0", "tailscale0"]);
        assert_eq!(
            both.len(),
            10,
            "two addresses in one /64 share their scopes"
        );
        let tcp_count = both
            .iter()
            .take_while(|scope| scope.protocol == RuleProtocol::Tcp)
            .count();
        assert_eq!(tcp_count, 6);
        assert!(
            both[tcp_count..]
                .iter()
                .all(|scope| scope.protocol == RuleProtocol::Udp && scope.port == MDNS_PORT)
        );
        assert!(both.contains(&AuthorizedScope {
            firewall: FirewallKind::Firewalld,
            zone: Some("home".to_string()),
            ..udp("wlp6s0", "ff02::fb", "fe80::/10")
        }));
        assert!(both.contains(&AuthorizedScope {
            port: DEVELOPMENT_COORDINATOR_PORT,
            ..tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64")
        }));
        for scope in &both {
            assert!(validate_authorized_scope(scope).is_ok(), "{scope:?}");
        }
    }

    #[test]
    fn the_validator_accepts_only_listener_and_discovery_rules_from_local_sources() {
        for valid in [
            tcp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
            tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24"),
            tcp("eth0", "10.0.0.0/8", "10.0.0.0/8"),
            tcp("eth0", "172.20.1.0/24", "172.16.0.0/12"),
            tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
            tcp("tailscale0", "fd7a:115c:a1e0::1", "fc00::/7"),
            tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64"),
            AuthorizedScope {
                port: DEVELOPMENT_COORDINATOR_PORT,
                ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
            },
            udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
            udp("wlp2s0", "224.0.0.251", "169.254.0.0/16"),
            udp("wlp6s0", "ff02::fb", "fe80::/10"),
            udp("wlp6s0", "ff02::fb", "2806:101e:e:8970::/64"),
        ] {
            assert!(validate_authorized_scope(&valid).is_ok(), "{valid:?}");
        }

        let base = tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24");
        let discovery = udp("wlp2s0", "224.0.0.251", "192.168.1.0/24");
        let invalid = [
            (
                "public IPv4 source",
                tcp("eth0", "8.8.8.0/24", "8.8.8.0/24"),
            ),
            ("any source", tcp("eth0", "192.168.1.66", "0.0.0.0/0")),
            ("any IPv6 source", tcp("eth0", "fd00::1", "::/0")),
            (
                "source wider than its block",
                tcp("eth0", "10.1.2.3", "10.0.0.0/7"),
            ),
            (
                "overlay source wider than its block",
                tcp("eth0", "100.101.2.3", "100.0.0.0/8"),
            ),
            (
                "global source wider than /64",
                tcp("eth0", "2806:101e:e::/64", "2806:101e:e::/48"),
            ),
            (
                "documentation source",
                tcp("eth0", "2001:db8::/64", "2001:db8::/64"),
            ),
            ("multicast source", tcp("eth0", "ff02::fb", "ff02::/16")),
            ("loopback source", tcp("lo", "127.0.0.1", "127.0.0.0/8")),
            (
                "destination outside its source",
                tcp("eth0", "192.168.2.0/24", "192.168.1.0/24"),
            ),
            (
                "destination of another family",
                tcp("eth0", "fd00::1", "192.168.1.0/24"),
            ),
            (
                "other TCP port",
                AuthorizedScope {
                    port: 44_000,
                    ..base.clone()
                },
            ),
            (
                "discovery port over TCP",
                AuthorizedScope {
                    port: MDNS_PORT,
                    ..base.clone()
                },
            ),
            (
                "coordinator port over UDP",
                AuthorizedScope {
                    port: COORDINATOR_PORT,
                    ..discovery.clone()
                },
            ),
            (
                "UDP to a unicast destination",
                udp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
            ),
            (
                "UDP to the other family's group",
                udp("wlp2s0", "ff02::fb", "192.168.1.0/24"),
            ),
            (
                "UDP from a public source",
                udp("eth0", "224.0.0.251", "8.8.8.0/24"),
            ),
            (
                "empty interface",
                AuthorizedScope {
                    interface: String::new(),
                    ..base.clone()
                },
            ),
            (
                "unsafe interface",
                AuthorizedScope {
                    interface: "eth0;reboot".to_string(),
                    ..discovery
                },
            ),
            (
                "long interface",
                AuthorizedScope {
                    interface: "a".repeat(16),
                    ..base.clone()
                },
            ),
            (
                "UFW with a zone",
                AuthorizedScope {
                    zone: Some("home".to_string()),
                    ..base.clone()
                },
            ),
            (
                "firewalld without a zone",
                AuthorizedScope {
                    firewall: FirewallKind::Firewalld,
                    ..base.clone()
                },
            ),
            (
                "firewalld with an unsafe zone",
                AuthorizedScope {
                    firewall: FirewallKind::Firewalld,
                    zone: Some("home zone".to_string()),
                    ..base
                },
            ),
        ];
        for (reason, scope) in invalid {
            assert!(validate_authorized_scope(&scope).is_err(), "{reason}");
        }
    }

    #[test]
    fn the_helper_parses_only_complete_valid_scopes() {
        let valid = [
            "grant",
            "ufw",
            "tcp",
            "wlp2s0",
            "192.168.1.0/24",
            "192.168.1.0/24",
            "43822",
            "-",
            "firewalld",
            "udp",
            "wlp6s0",
            "ff02::fb/128",
            "fe80::/10",
            "5353",
            "home",
        ];
        let Ok(HelperRequest::Apply { scopes, grant }) = helper_request(&valid) else {
            panic!("valid helper scopes");
        };
        assert!(grant);
        assert_eq!(
            scopes,
            vec![
                AuthorizedScope {
                    port: DEVELOPMENT_COORDINATOR_PORT,
                    ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
                },
                AuthorizedScope {
                    firewall: FirewallKind::Firewalld,
                    zone: Some("home".to_string()),
                    ..udp("wlp6s0", "ff02::fb", "fe80::/10")
                },
            ]
        );

        let replace = |index: usize, value: &'static str| {
            let mut arguments = valid;
            arguments[index] = value;
            helper_request(&arguments)
        };
        for (index, value) in [
            (0, "allow"),
            (1, "iptables"),
            (2, "icmp"),
            (2, "TCP"),
            (3, "wlp2s0;id"),
            (4, "192.168.1.66/24"),
            (4, "192.168.1.66"),
            (5, "0.0.0.0/0"),
            (5, "8.8.8.0/24"),
            (6, "22"),
            (6, "43823"),
            (6, "5353"),
            (6, "port"),
            (7, "home"),
            (9, "tcp"),
            (12, "fe80::/9"),
            (13, "53"),
            (14, "-"),
        ] {
            assert!(
                replace(index, value).is_err(),
                "argument {index} as {value}"
            );
        }
        assert!(
            helper_request(&valid[..valid.len() - 1]).is_err(),
            "truncated"
        );
        let mut extra = valid.to_vec();
        extra.push("unexpected");
        assert!(helper_request(&extra).is_err(), "trailing argument");
        assert!(helper_request(&["grant"]).is_err());

        assert_eq!(
            helper_request(&["revoke-all"]),
            Ok(HelperRequest::RevokeAll)
        );
        assert!(helper_request(&["revoke-all", "ufw"]).is_err());
    }

    #[test]
    fn one_helper_invocation_carries_every_scope_of_a_flow() {
        let firewalld = AuthorizedScope {
            firewall: FirewallKind::Firewalld,
            zone: Some("home".to_string()),
            ..numbered_scope(1)
        };
        let scopes = vec![
            numbered_scope(1),
            firewalld,
            AuthorizedScope {
                port: DEVELOPMENT_COORDINATOR_PORT,
                ..tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64")
            },
            udp("wlp6s0", "ff02::fb", "fe80::/10"),
            tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
        ];
        for grant in [true, false] {
            let arguments = privileged_helper_arguments(&scopes, grant).expect("arguments");
            assert_eq!(arguments[0], PRIVILEGED_HELPER_ARGUMENT);
            assert_eq!(arguments.len(), 2 + 7 * scopes.len());
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
            .map(|index| numbered_scope(u8::try_from(index).unwrap()))
            .collect();
        let arguments = privileged_helper_arguments(&most, false).expect("bounded arguments");
        let mut too_many: Vec<String> = arguments.into_iter().skip(1).collect();
        too_many.extend(too_many[1..8].to_vec());
        assert!(parse_privileged_helper_request(&mut too_many.into_iter()).is_err());
        assert!(privileged_helper_arguments(&[], true).is_err());
        assert!(privileged_helper_arguments(&[most.clone(), most].concat(), true).is_err());
        let other_port = AuthorizedScope {
            port: 8080,
            ..numbered_scope(1)
        };
        assert!(privileged_helper_arguments(&[other_port], true).is_err());
    }

    #[test]
    fn a_grant_stops_at_a_failed_listener_rule_but_not_at_a_failed_discovery_rule() {
        let discovery = udp("wlp2s0", "224.0.0.251", "192.168.1.0/24");
        let scopes = [
            numbered_scope(1),
            numbered_scope(2),
            discovery.clone(),
            numbered_scope(3),
        ];
        let run = |grant: bool, failures: &[AuthorizedScope]| {
            let mut attempted = Vec::new();
            let result = apply_each(&scopes, grant, |scope| {
                attempted.push(scope.clone());
                if failures.contains(scope) {
                    Err(format!("failed {}", scope.source))
                } else {
                    Ok(())
                }
            });
            (result, attempted)
        };

        let (result, attempted) = run(true, std::slice::from_ref(&discovery));
        assert_eq!(result, Ok(()));
        assert_eq!(attempted, scopes);

        let failures = [numbered_scope(2), discovery];
        let (result, attempted) = run(true, &failures);
        assert_eq!(result, Err("failed 192.168.2.0/24".to_string()));
        assert_eq!(attempted, scopes[..2]);

        let (result, attempted) = run(false, &failures);
        assert_eq!(result, Err("failed 192.168.2.0/24".to_string()));
        assert_eq!(attempted, scopes, "a revoke attempts every scope");
    }

    #[test]
    fn a_full_record_refuses_a_grant_before_any_rule_changes() {
        let last = u8::try_from(MAX_AUTHORIZED_SCOPES).unwrap() - 2;
        let mut stored = empty_state();
        stored.scopes = (0..last).map(|index| granted(index, &[])).collect();
        stored.pending = vec![numbered_scope(last)];
        let mut applied = Vec::new();
        let result = grant_scopes(
            &mut stored,
            vec![candidate(last + 1, None), candidate(last + 2, None)],
            |_| Ok(()),
            |scopes| {
                applied.extend_from_slice(scopes);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(applied.is_empty());
        assert_eq!(
            stored.scopes.len() + stored.pending.len(),
            MAX_AUTHORIZED_SCOPES - 1
        );

        let granted = grant_scopes(
            &mut stored,
            vec![candidate(last, None), candidate(last + 1, None)],
            |_| Ok(()),
            |scopes| {
                applied.extend_from_slice(scopes);
                Ok(())
            },
        );
        assert!(granted.is_ok());
        assert_eq!(
            applied,
            vec![numbered_scope(last), numbered_scope(last + 1)]
        );
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
            vec![candidate(1, Some(HOME)), candidate(2, Some(HOME))],
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
            vec![
                candidate(1, Some(HOME)),
                candidate(2, Some(HOME)),
                candidate(3, Some(CAFE)),
            ],
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
        assert_eq!(
            stored.scopes,
            vec![
                granted(1, &[HOME]),
                granted(2, &[HOME]),
                granted(3, &[CAFE])
            ],
            "each scope records the network of its own interface"
        );

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
        let state = |stored: &StoredNetworkAccess, candidates: &[Candidate], can: bool| {
            access_state(stored, candidates, || can)
        };
        assert_eq!(
            state(&stored, &[candidate(1, Some(HOME))], false),
            NetworkAccessState::Granted
        );
        assert_eq!(
            state(&stored, &[candidate(1, Some(CAFE))], false),
            NetworkAccessState::NetworkConfirmationRequired
        );
        assert_eq!(
            state(&stored, &[candidate(1, None)], false),
            NetworkAccessState::Granted
        );
        let missing = [candidate(1, Some(HOME)), candidate(2, Some(HOME))];
        assert_eq!(
            state(&stored, &missing, true),
            NetworkAccessState::AuthorizationRequired
        );
        assert_eq!(
            state(&stored, &missing, false),
            NetworkAccessState::ManualActionRequired
        );
        let discovery = Candidate {
            scope: udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
            network: Some(HOME.to_string()),
        };
        assert_eq!(
            state(&stored, &[candidate(1, Some(HOME)), discovery], true),
            NetworkAccessState::Granted,
            "a missing discovery rule never blocks linking"
        );

        stored.scopes = vec![granted(1, &[])];
        assert_eq!(
            state(&stored, &[candidate(1, Some(HOME))], false),
            NetworkAccessState::NetworkConfirmationRequired
        );
        assert!(
            admits_lan_peers(
                Path::new("/nonexistent"),
                &[local("wlp2s0", "192.168.1.66", 24)],
                "10.0.0.2".parse().unwrap(),
                COORDINATOR_PORT
            ),
            "a local address without an interface entry has no rule to confirm"
        );
    }

    #[test]
    fn confirming_a_network_records_it_without_changing_the_firewall() {
        let mut stored = empty_state();
        stored.scopes = vec![granted(1, &[HOME])];
        let mut persisted = 0;
        let confirmed = grant_scopes(
            &mut stored,
            vec![candidate(1, Some(CAFE))],
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
    fn saved_state_of_another_schema_version_is_refused() {
        let folder = scratch_folder("state");
        let mut stored = empty_state();
        stored.scopes = vec![granted(1, &[HOME])];
        stored.pending = vec![udp("wlp6s0", "ff02::fb", "fe80::/10")];
        persist_state(&folder, &stored).expect("persist");
        let restored = read_state(&folder).expect("read");
        assert_eq!(restored.scopes, stored.scopes);
        assert_eq!(restored.pending, stored.pending);

        let earlier = r#"{"schemaVersion":2,"scopes":[{"firewall":"ufw","interface":"wlp2s0","localAddress":"192.168.1.66","sourceNetwork":"192.168.1.0/24","port":43821,"networks":[]}],"pending":[]}"#;
        fs::write(state_path(&folder), earlier).unwrap();
        let error = read_state(&folder).expect_err("schema version 2");
        assert!(error.contains("unsupported"), "{error}");

        let broad = serde_json::to_string(&StoredNetworkAccess {
            schema_version: STATE_SCHEMA_VERSION,
            scopes: Vec::new(),
            pending: vec![tcp("eth0", "192.168.1.66", "0.0.0.0/0")],
        })
        .unwrap();
        fs::write(state_path(&folder), broad).unwrap();
        assert!(read_state(&folder).is_err(), "saved scopes are validated");
        fs::remove_dir_all(&folder).unwrap();
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
    fn firewall_commands_name_the_protocol_interface_and_both_prefixes() {
        assert_eq!(
            ufw_arguments(
                &AuthorizedScope {
                    port: DEVELOPMENT_COORDINATOR_PORT,
                    ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
                },
                true
            ),
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
                "192.168.1.0/24",
                "port",
                "43822",
                "comment",
                "Ganbaru device linking",
            ]
        );
        let discovery = udp("wlp6s0", "ff02::fb", "fe80::/10");
        assert_eq!(
            ufw_arguments(&discovery, false)[..12],
            [
                "delete",
                "allow",
                "in",
                "on",
                "wlp6s0",
                "proto",
                "udp",
                "from",
                "fe80::/10",
                "to",
                "ff02::fb",
                "port",
            ]
            .map(str::to_string)
        );
        assert_eq!(
            firewalld_rich_rule(&discovery),
            "rule family=\"ipv6\" source address=\"fe80::/10\" destination address=\"ff02::fb/128\" port port=\"5353\" protocol=\"udp\" accept"
        );
    }

    #[test]
    fn configuration_flags_ignore_comments_and_other_values() {
        assert!(!config_value_is_yes(
            "# ENABLED=yes\nENABLED=no\n",
            "ENABLED"
        ));
        assert!(config_value_is_yes("ENABLED=yes\n", "ENABLED"));
        assert!(config_value_is_yes("IPV6=\"yes\"\n", "IPV6"));
        assert!(!config_value_is_yes("IPV6=yes\n", "ENABLED"));
        assert!(!config_value_is_yes("NOIPV6=yes\n", "IPV6"));
    }

    #[test]
    fn only_root_owned_entries_that_others_cannot_write_are_trusted() {
        use std::os::unix::fs::PermissionsExt;

        assert!(root_controlled(&Path::new("/").symlink_metadata().unwrap()));
        assert_ne!(PACKAGED_HELPER_PATH, PACKAGED_APP_PATH);
        assert!(
            PKEXEC_PATHS
                .iter()
                .all(|path| !path.starts_with("/usr/local/"))
        );
        assert_eq!(
            command_path(&["/nonexistent/pkexec", "/usr/bin/env"]),
            Some(PathBuf::from("/usr/bin/env"))
        );

        let folder = scratch_folder("command");
        let planted = folder.join("pkexec");
        fs::write(&planted, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&planted, fs::Permissions::from_mode(0o755)).unwrap();
        if planted.symlink_metadata().unwrap().uid() != 0 {
            assert!(!root_controlled(&folder.symlink_metadata().unwrap()));
            assert_eq!(command_path(&[planted.to_str().unwrap()]), None);
        }
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn package_removal_finds_every_app_ufw_rule_shape() {
        let output = "\
Added user rules (see 'ufw status' for running firewall):
ufw allow 22/tcp
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.0/24 port 43822 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 224.0.0.251 port 5353 proto udp comment 'Ganbaru device linking'
ufw allow in on wlp6s0 from 2806:101e:e:8970::/64 to 2806:101e:e:8970::/64 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp6s0 from fe80::/10 to ff02::fb port 5353 proto udp comment 'Ganbaru device linking'
ufw allow in on tailscale0 from 100.64.0.0/10 to 100.101.2.3 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'My own rule'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 8080 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 0.0.0.0/0 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp6s0 from ::/0 to ff02::fb port 5353 proto udp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 port 43821 to 192.168.1.66 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto udp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 5353 proto udp comment 'Ganbaru device linking'
ufw deny in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
";
        assert_eq!(
            parse_ufw_added_rules(output),
            vec![
                tcp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
                AuthorizedScope {
                    port: DEVELOPMENT_COORDINATOR_PORT,
                    ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
                },
                udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
                tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64"),
                udp("wlp6s0", "ff02::fb", "fe80::/10"),
                tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
            ]
        );

        for scope in [
            tcp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
            udp("wlp6s0", "ff02::fb", "fe80::/10"),
        ] {
            let line = format!(
                "ufw {} '{RULE_COMMENT}'",
                ufw_arguments(&scope, true)[..13].join(" ")
            );
            assert_eq!(parse_ufw_added_rules(&line), vec![scope], "{line}");
        }
    }

    #[test]
    fn package_removal_finds_every_app_firewalld_rule_shape() {
        let earlier = "rule family=\"ipv4\" source address=\"192.168.1.0/24\" destination address=\"192.168.1.66/32\" port port=\"43821\" protocol=\"tcp\" accept";
        let scope = parse_firewalld_rich_rule("home", earlier).expect("earlier host rule");
        assert_eq!(scope.zone.as_deref(), Some("home"));
        assert_eq!(scope.destination, host("192.168.1.66"));
        assert_eq!(scope.source, prefix("192.168.1.0/24"));
        let bare = earlier.replace("192.168.1.66/32", "192.168.1.66");
        assert_eq!(parse_firewalld_rich_rule("home", &bare), Some(scope));

        let zoned = |scope: AuthorizedScope| AuthorizedScope {
            firewall: FirewallKind::Firewalld,
            interface: String::new(),
            zone: Some("home".to_string()),
            ..scope
        };
        for scope in [
            zoned(tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")),
            zoned(udp("wlp2s0", "224.0.0.251", "192.168.1.0/24")),
            zoned(tcp(
                "wlp6s0",
                "2806:101e:e:8970::/64",
                "2806:101e:e:8970::/64",
            )),
            zoned(udp("wlp6s0", "ff02::fb", "fe80::/10")),
            zoned(tcp("tailscale0", "fd7a:115c:a1e0::1", "fc00::/7")),
        ] {
            let rule = firewalld_rich_rule(&scope);
            assert_eq!(
                parse_firewalld_rich_rule("home", &rule),
                Some(scope),
                "{rule}"
            );
        }

        let ipv6 = "rule family=\"ipv6\" source address=\"fe80::/10\" destination address=\"ff02::fb\" port port=\"5353\" protocol=\"udp\" accept";
        assert!(parse_firewalld_rich_rule("public", ipv6).is_some());
        for foreign in [
            earlier.replace("43821", "8080"),
            earlier.replace("accept", "reject"),
            earlier.replace("\"tcp\"", "\"udp\""),
            earlier.replace("192.168.1.0/24", "0.0.0.0/0"),
            earlier.replace("ipv4", "ipv6"),
            ipv6.replace("ipv6", "ipv4"),
            ipv6.replace("ff02::fb", "ff02::1"),
            ipv6.replace("5353", "43821"),
            ipv6.replace("fe80::/10", "::/0"),
            format!("{earlier} log"),
            "rule service name=\"ssh\" accept".to_string(),
        ] {
            assert_eq!(
                parse_firewalld_rich_rule("home", &foreign),
                None,
                "{foreign}"
            );
        }
        assert_eq!(parse_firewalld_rich_rule("bad zone", earlier), None);
    }
}
