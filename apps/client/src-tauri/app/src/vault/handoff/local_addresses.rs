//! Local addresses this desktop names to other devices.
//!
//! The coordinator listens on every local address, so the address it hands out in invitations,
//! contact cards, and the Linux firewall rules is chosen separately from the bind.

use ganbaru_handoff::dial::{DialAddressClass, reportable_address};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// Documentation address whose route reveals the source address the system prefers. No packet
/// is sent: connecting a UDP socket only selects a route.
const ROUTE_PROBE_TARGET: (Ipv4Addr, u16) = (Ipv4Addr::new(192, 0, 2, 1), 9);

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
}
