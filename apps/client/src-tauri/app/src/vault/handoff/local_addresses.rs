//! Local addresses this desktop names to other devices.
//!
//! The coordinator listens on every local address, so the address it hands out in invitations,
//! contact cards, and the Linux firewall rules is chosen separately from the bind.

use ganbaru_handoff::dial::{DialAddressClass, reportable_address};
use ganbaru_handoff::protocol::MAX_COORDINATOR_ENDPOINTS;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Documentation address whose route reveals the source address the system prefers. No packet
/// is sent: connecting a UDP socket only selects a route.
const ROUTE_PROBE_TARGET: (Ipv4Addr, u16) = (Ipv4Addr::new(192, 0, 2, 1), 9);

/// How long gathered reported endpoints are reused.
const REPORTED_ENDPOINTS_LIFETIME: Duration = Duration::from_secs(5);

/// Error shown when this desktop has no address another device could reach.
const NO_LOCAL_NETWORK_ADDRESS: &str = "no local network address is currently available";

/// The address other devices should dial: the IPv4 source of the default route when it is
/// reportable, otherwise the best reportable interface address.
pub(crate) fn preferred_invitation_address() -> Result<IpAddr, String> {
    choose_invitation_address(routed_ipv4_source(), &interface_addresses())
        .ok_or_else(|| NO_LOCAL_NETWORK_ADDRESS.to_string())
}

/// The IPv4 address this desktop reports for the Linux firewall rules, whose scopes are IPv4
/// only. `None` when no reportable IPv4 address exists.
#[cfg(target_os = "linux")]
pub(crate) fn preferred_ipv4_address() -> Option<Ipv4Addr> {
    match preferred_invitation_address() {
        Ok(IpAddr::V4(address)) => Some(address),
        _ => None,
    }
}

/// Picks the invitation address from the routed IPv4 source and the interface addresses.
///
/// A reportable routed source wins. Otherwise a private IPv4 address comes before an IPv4
/// link-local or overlay one, and a unique local IPv6 address is used only when no IPv4 address
/// is reportable. Global IPv6 is left out because a receiver accepts it only on its own prefix.
fn choose_invitation_address(routed: Option<IpAddr>, candidates: &[IpAddr]) -> Option<IpAddr> {
    if let Some(routed) = routed.filter(|address| address.is_ipv4() && is_reportable(*address)) {
        return Some(routed);
    }
    let reportable_ipv4 = || {
        candidates
            .iter()
            .copied()
            .filter(|address| address.is_ipv4() && is_reportable(*address))
    };
    reportable_ipv4()
        .find(|address| is_private_lan(*address) && !is_ipv4_link_local(*address))
        .or_else(|| reportable_ipv4().next())
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .find(|address| is_unique_local(*address) && is_reportable(*address))
        })
}

/// The listener endpoints this coordinator reports to linked devices in sync states, at most
/// [`MAX_COORDINATOR_ENDPOINTS`], best first.
pub(crate) fn reported_endpoints(port: u16) -> Vec<String> {
    order_reported_addresses(routed_ipv4_source(), &interface_addresses())
        .into_iter()
        .map(|address| SocketAddr::new(address, port).to_string())
        .collect()
}

/// Orders the reportable addresses for sync states: private IPv4, IPv4 link-local, unique local
/// IPv6, global IPv6, then overlay, because a LAN path is faster when both work. The routed
/// source leads its class, so in the common case the first entry is the invitation address.
/// Duplicates and unreportable addresses are dropped, and the list is capped.
fn order_reported_addresses(routed: Option<IpAddr>, candidates: &[IpAddr]) -> Vec<IpAddr> {
    let mut ordered: Vec<(u8, IpAddr)> = Vec::new();
    for address in routed.into_iter().chain(candidates.iter().copied()) {
        let address = address.to_canonical();
        let Some(rank) = report_rank(address) else {
            continue;
        };
        if ordered.iter().all(|(_, known)| *known != address) {
            ordered.push((rank, address));
        }
    }
    ordered.sort_by_key(|(rank, _)| *rank);
    ordered
        .into_iter()
        .map(|(_, address)| address)
        .take(MAX_COORDINATOR_ENDPOINTS)
        .collect()
}

/// Where a reportable address goes in the reported list, or `None` when it is not reportable.
fn report_rank(address: IpAddr) -> Option<u8> {
    match reportable_address(address).ok()? {
        DialAddressClass::PrivateLan if is_ipv4_link_local(address) => Some(1),
        DialAddressClass::PrivateLan if address.is_ipv4() => Some(0),
        DialAddressClass::PrivateLan => Some(2),
        DialAddressClass::GlobalIpv6 => Some(3),
        DialAddressClass::Overlay => Some(4),
        DialAddressClass::Loopback => None,
    }
}

/// Keeps the reported endpoints for [`REPORTED_ENDPOINTS_LIFETIME`], so a burst of sync requests
/// enumerates the interfaces once.
#[derive(Default)]
pub(crate) struct ReportedEndpointCache {
    entry: Mutex<Option<(Instant, Vec<String>)>>,
}

impl ReportedEndpointCache {
    /// The cached endpoints when they are fresh at `now`, otherwise the result of `gather`.
    pub(crate) fn get_or_refresh(
        &self,
        now: Instant,
        gather: impl FnOnce() -> Vec<String>,
    ) -> Vec<String> {
        let mut entry = match self.entry.lock() {
            Ok(entry) => entry,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((gathered_at, endpoints)) = entry.as_ref()
            && now.saturating_duration_since(*gathered_at) < REPORTED_ENDPOINTS_LIFETIME
        {
            return endpoints.clone();
        }
        let endpoints = gather();
        *entry = Some((now, endpoints.clone()));
        endpoints
    }
}

fn is_reportable(address: IpAddr) -> bool {
    reportable_address(address).is_ok()
}

fn is_private_lan(address: IpAddr) -> bool {
    matches!(
        reportable_address(address),
        Ok(DialAddressClass::PrivateLan)
    )
}

fn is_ipv4_link_local(address: IpAddr) -> bool {
    matches!(address, IpAddr::V4(address) if address.is_link_local())
}

fn is_unique_local(address: IpAddr) -> bool {
    matches!(address, IpAddr::V6(address) if address.is_unique_local())
}

/// The IPv4 source address the system routes toward [`ROUTE_PROBE_TARGET`], if any route exists.
fn routed_ipv4_source() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(ROUTE_PROBE_TARGET).ok()?;
    socket.local_addr().ok().map(|address| address.ip())
}

/// Addresses of the interfaces that are not known to be down. Enumeration failures are logged
/// and yield no addresses, which leaves the routed source as the only candidate.
fn interface_addresses() -> Vec<IpAddr> {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces
            .into_iter()
            .filter(|interface| !interface.is_loopback() && !is_down(&interface.oper_status))
            .map(|interface| interface.ip())
            .collect(),
        Err(error) => {
            eprintln!("list local network interfaces: {error}");
            Vec::new()
        }
    }
}

fn is_down(status: &if_addrs::IfOperStatus) -> bool {
    matches!(
        status,
        if_addrs::IfOperStatus::Down
            | if_addrs::IfOperStatus::NotPresent
            | if_addrs::IfOperStatus::LowerLayerDown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(address: &str) -> IpAddr {
        address.parse().expect("address")
    }

    fn addresses(values: &[&str]) -> Vec<IpAddr> {
        values.iter().map(|value| ip(value)).collect()
    }

    #[test]
    fn a_reportable_routed_source_wins() {
        let candidates = addresses(&["10.0.0.5", "fd00::5"]);
        assert_eq!(
            choose_invitation_address(Some(ip("192.168.1.20")), &candidates),
            Some(ip("192.168.1.20"))
        );
        assert_eq!(
            choose_invitation_address(Some(ip("100.80.0.4")), &candidates),
            Some(ip("100.80.0.4")),
            "an overlay route is reportable"
        );
    }

    #[test]
    fn an_unreportable_routed_source_falls_back_to_interfaces() {
        let candidates = addresses(&["fe80::1", "fd00::5", "100.80.0.4", "192.168.1.20"]);
        for routed in [None, Some(ip("127.0.0.1")), Some(ip("203.0.113.9"))] {
            assert_eq!(
                choose_invitation_address(routed, &candidates),
                Some(ip("192.168.1.20")),
                "routed source {routed:?}"
            );
        }
    }

    #[test]
    fn private_ipv4_comes_before_link_local_and_overlay_ipv4() {
        assert_eq!(
            choose_invitation_address(
                None,
                &addresses(&["169.254.3.4", "100.80.0.4", "172.16.0.9"])
            ),
            Some(ip("172.16.0.9"))
        );
        assert_eq!(
            choose_invitation_address(None, &addresses(&["fd00::5", "100.80.0.4"])),
            Some(ip("100.80.0.4")),
            "any reportable IPv4 address comes before unique local IPv6"
        );
    }

    #[test]
    fn unique_local_ipv6_is_used_only_without_ipv4() {
        assert_eq!(
            choose_invitation_address(
                None,
                &addresses(&["fe80::1", "2001:db8::5", "fd12:3456::7"])
            ),
            Some(ip("fd12:3456::7"))
        );
    }

    #[test]
    fn loopback_link_local_ipv6_and_public_addresses_are_never_chosen() {
        assert_eq!(
            choose_invitation_address(
                Some(ip("8.8.8.8")),
                &addresses(&["127.0.0.1", "::1", "fe80::1", "203.0.113.9", "2001:db8::5"])
            ),
            None
        );
        assert_eq!(choose_invitation_address(None, &[]), None);
    }

    #[test]
    fn reported_addresses_rank_lan_before_global_and_overlay_with_the_route_leading_its_class() {
        let candidates = addresses(&[
            "100.80.0.4",
            "2a00:1450::5",
            "fd00::5",
            "169.254.3.4",
            "10.0.0.5",
            "192.168.1.20",
            "fe80::1",
            "127.0.0.1",
        ]);
        assert_eq!(
            order_reported_addresses(Some(ip("192.168.1.20")), &candidates),
            addresses(&[
                "192.168.1.20",
                "10.0.0.5",
                "169.254.3.4",
                "fd00::5",
                "2a00:1450::5",
                "100.80.0.4",
            ])
        );
    }

    #[test]
    fn reported_addresses_drop_duplicates_unreportable_routes_and_extra_entries() {
        assert_eq!(
            order_reported_addresses(
                Some(ip("203.0.113.9")),
                &addresses(&["::ffff:10.0.0.5", "10.0.0.5", "::1"])
            ),
            addresses(&["10.0.0.5"]),
            "a mapped address is the same IPv4 address"
        );
        let many: Vec<IpAddr> = (1..=20)
            .map(|host| IpAddr::V4(Ipv4Addr::new(10, 0, 0, host)))
            .collect();
        let ordered = order_reported_addresses(None, &many);
        assert_eq!(ordered.len(), MAX_COORDINATOR_ENDPOINTS);
        assert_eq!(ordered, many[..MAX_COORDINATOR_ENDPOINTS]);
    }

    #[test]
    fn reported_endpoint_cache_reuses_entries_until_they_expire() {
        let cache = ReportedEndpointCache::default();
        let start = Instant::now();
        let first = cache.get_or_refresh(start, || vec!["192.168.1.20:1".to_string()]);
        let cached = cache.get_or_refresh(start + REPORTED_ENDPOINTS_LIFETIME / 2, || {
            panic!("a fresh entry is reused")
        });
        assert_eq!(cached, first);
        let refreshed = cache.get_or_refresh(start + REPORTED_ENDPOINTS_LIFETIME, || {
            vec!["10.0.0.5:1".to_string()]
        });
        assert_eq!(refreshed, vec!["10.0.0.5:1".to_string()]);
    }
}
