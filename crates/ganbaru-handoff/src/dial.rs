//! Address rules for outgoing handoff and contact connections.
//!
//! Invitations and contact cards arrive from untrusted QR codes or pasted text, so the address
//! they carry decides where this device connects. Linking is a LAN feature: only private,
//! link-local, and overlay addresses are dialed. Global IPv6 is dialed only when it shares the
//! on-link prefix of the source address this device would use to reach it. Linking across the
//! internet belongs to the planned relay.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Prefix length treated as on-link for global IPv6. SLAAC and DHCPv6 LANs assign a /64.
const ON_LINK_IPV6_PREFIX_BITS: u32 = 64;

/// Address class of a dial target that passed the shape check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialAddressClass {
    /// Private IPv4, IPv4 link-local, IPv6 unique local, or IPv6 link-local.
    PrivateLan,
    /// Shared address space `100.64.0.0/10`, used by Tailscale and carrier-grade NAT overlays.
    Overlay,
    /// Loopback, dialed only by development builds.
    Loopback,
    /// Global unicast IPv6, dialed only when it is on a local link.
    GlobalIpv6,
}

/// Whether loopback targets may be dialed. Production builds reject them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopbackPolicy {
    Allow,
    Reject,
}

/// Reason a dial target is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialTargetError {
    Unspecified,
    Loopback,
    Multicast,
    Broadcast,
    Documentation,
    Public,
    OffLinkIpv6,
    InvalidPort,
}

impl fmt::Display for DialTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unspecified => "the device address is unspecified",
            Self::Loopback => "the device address points at this device",
            Self::Multicast => "the device address is a multicast address",
            Self::Broadcast => "the device address is a broadcast address",
            Self::Documentation => "the device address is reserved for documentation",
            Self::Public => "the device address is not on a local network",
            Self::OffLinkIpv6 => "the device address is not on a local network link",
            Self::InvalidPort => "the device port is invalid",
        })
    }
}

impl std::error::Error for DialTargetError {}

/// Classifies a dial target address, refusing every class that is never a LAN peer.
pub fn classify_dial_address(address: IpAddr) -> Result<DialAddressClass, DialTargetError> {
    match address {
        IpAddr::V4(address) => classify_ipv4(address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(mapped) => classify_ipv4(mapped),
            None => classify_ipv6(address),
        },
    }
}

/// Checks the shape of a dial target carried by an invitation or card: a nonzero port and an
/// address class that may be a LAN peer. Loopback and on-link checks need device context and
/// run in [`authorize_dial_target`] before connecting.
pub fn validate_dial_endpoint(endpoint: SocketAddr) -> Result<DialAddressClass, DialTargetError> {
    if endpoint.port() == 0 {
        return Err(DialTargetError::InvalidPort);
    }
    classify_dial_address(endpoint.ip())
}

/// Decides whether this device may connect to `endpoint`.
///
/// `on_link_source` is the IPv6 source address the operating system selects to reach a global
/// IPv6 target, or `None` when no route exists. The target is dialed only when both share the
/// on-link prefix.
pub fn authorize_dial_target(
    endpoint: SocketAddr,
    loopback: LoopbackPolicy,
    on_link_source: Option<Ipv6Addr>,
) -> Result<(), DialTargetError> {
    match validate_dial_endpoint(endpoint)? {
        DialAddressClass::PrivateLan | DialAddressClass::Overlay => Ok(()),
        DialAddressClass::Loopback => match loopback {
            LoopbackPolicy::Allow => Ok(()),
            LoopbackPolicy::Reject => Err(DialTargetError::Loopback),
        },
        DialAddressClass::GlobalIpv6 => {
            let IpAddr::V6(target) = endpoint.ip() else {
                return Err(DialTargetError::Public);
            };
            match on_link_source {
                Some(source) if shares_on_link_prefix(target, source) => Ok(()),
                _ => Err(DialTargetError::OffLinkIpv6),
            }
        }
    }
}

/// Whether `target` lies in the on-link prefix of the global `source` address.
pub fn shares_on_link_prefix(target: Ipv6Addr, source: Ipv6Addr) -> bool {
    if !is_global_unicast(source) || target == source {
        return false;
    }
    let mask = u128::MAX << (128 - ON_LINK_IPV6_PREFIX_BITS);
    (u128::from(target) & mask) == (u128::from(source) & mask)
}

/// Private and link-local IPv4 blocks, as network and prefix length.
const PRIVATE_IPV4_BLOCKS: [(Ipv4Addr, u32); 4] = [
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
    (Ipv4Addr::new(169, 254, 0, 0), 16),
];
/// Shared address space used by overlays, as network and prefix length.
const OVERLAY_IPV4_BLOCK: (Ipv4Addr, u32) = (Ipv4Addr::new(100, 64, 0, 0), 10);

/// Network mask of an IPv4 prefix length of at most 32.
pub fn ipv4_mask(prefix: u32) -> u32 {
    u32::MAX.checked_shl(32 - prefix.min(32)).unwrap_or(0)
}

fn block_contains((network, prefix): (Ipv4Addr, u32), address: Ipv4Addr) -> bool {
    u32::from(address) & ipv4_mask(prefix) == u32::from(network)
}

/// The private, link-local, or overlay IPv4 block that holds `address`, as network and prefix
/// length. Every IPv4 LAN peer address lies in one of these blocks.
pub fn ipv4_lan_block(address: Ipv4Addr) -> Option<(Ipv4Addr, u32)> {
    PRIVATE_IPV4_BLOCKS
        .into_iter()
        .chain([OVERLAY_IPV4_BLOCK])
        .find(|block| block_contains(*block, address))
}

fn classify_ipv4(address: Ipv4Addr) -> Result<DialAddressClass, DialTargetError> {
    if address.octets()[0] == 0 {
        return Err(DialTargetError::Unspecified);
    }
    if address.is_loopback() {
        return Ok(DialAddressClass::Loopback);
    }
    if address.is_broadcast() {
        return Err(DialTargetError::Broadcast);
    }
    if address.is_multicast() {
        return Err(DialTargetError::Multicast);
    }
    if address.is_documentation() {
        return Err(DialTargetError::Documentation);
    }
    match ipv4_lan_block(address) {
        Some(OVERLAY_IPV4_BLOCK) => Ok(DialAddressClass::Overlay),
        Some(_) => Ok(DialAddressClass::PrivateLan),
        None => Err(DialTargetError::Public),
    }
}

fn classify_ipv6(address: Ipv6Addr) -> Result<DialAddressClass, DialTargetError> {
    let first = address.segments()[0];
    if address.is_unspecified() {
        return Err(DialTargetError::Unspecified);
    }
    if address.is_loopback() {
        return Ok(DialAddressClass::Loopback);
    }
    if address.is_multicast() {
        return Err(DialTargetError::Multicast);
    }
    if (first & 0xfe00) == 0xfc00 || address.is_unicast_link_local() {
        return Ok(DialAddressClass::PrivateLan);
    }
    if first == 0x2001 && address.segments()[1] == 0x0db8 {
        return Err(DialTargetError::Documentation);
    }
    if is_global_unicast(address) {
        return Ok(DialAddressClass::GlobalIpv6);
    }
    Err(DialTargetError::Public)
}

fn is_global_unicast(address: Ipv6Addr) -> bool {
    (address.segments()[0] & 0xe000) == 0x2000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(text: &str) -> Result<DialAddressClass, DialTargetError> {
        classify_dial_address(text.parse().expect("address"))
    }

    fn endpoint(text: &str) -> SocketAddr {
        text.parse().expect("endpoint")
    }

    #[test]
    fn private_link_local_and_overlay_addresses_are_lan_peers() {
        for address in [
            "10.0.0.4",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.20",
            "169.254.10.2",
            "fd12:3456::1",
            "fc00::1",
            "fe80::1",
            "::ffff:192.168.1.20",
        ] {
            assert_eq!(
                class(address),
                Ok(DialAddressClass::PrivateLan),
                "{address}"
            );
        }
        for address in ["100.64.0.1", "100.100.100.100", "100.127.255.254"] {
            assert_eq!(class(address), Ok(DialAddressClass::Overlay), "{address}");
        }
    }

    #[test]
    fn unroutable_and_public_addresses_are_refused() {
        let cases = [
            ("0.0.0.0", DialTargetError::Unspecified),
            ("0.1.2.3", DialTargetError::Unspecified),
            ("::", DialTargetError::Unspecified),
            ("224.0.0.251", DialTargetError::Multicast),
            ("ff02::1", DialTargetError::Multicast),
            ("255.255.255.255", DialTargetError::Broadcast),
            ("192.0.2.1", DialTargetError::Documentation),
            ("198.51.100.7", DialTargetError::Documentation),
            ("203.0.113.5", DialTargetError::Documentation),
            ("2001:db8::1", DialTargetError::Documentation),
            ("8.8.8.8", DialTargetError::Public),
            ("172.32.0.1", DialTargetError::Public),
            ("100.128.0.1", DialTargetError::Public),
            ("100.63.255.255", DialTargetError::Public),
            ("::ffff:8.8.8.8", DialTargetError::Public),
            ("64:ff9b::808:808", DialTargetError::Public),
        ];
        for (address, expected) in cases {
            assert_eq!(class(address), Err(expected), "{address}");
        }
        assert_eq!(class("127.0.0.1"), Ok(DialAddressClass::Loopback));
        assert_eq!(class("::1"), Ok(DialAddressClass::Loopback));
        assert_eq!(class("2a01:4f8::1"), Ok(DialAddressClass::GlobalIpv6));
    }

    #[test]
    fn loopback_is_dialed_only_when_allowed() {
        let target = endpoint("127.0.0.1:43821");
        assert_eq!(
            authorize_dial_target(target, LoopbackPolicy::Allow, None),
            Ok(())
        );
        assert_eq!(
            authorize_dial_target(target, LoopbackPolicy::Reject, None),
            Err(DialTargetError::Loopback)
        );
    }

    #[test]
    fn global_ipv6_is_dialed_only_inside_the_on_link_prefix() {
        let target = endpoint("[2a01:4f8:10:20::5]:43821");
        let on_link = "2a01:4f8:10:20::9".parse().expect("source");
        let off_link = "2a01:4f8:10:21::9".parse().expect("source");
        assert_eq!(
            authorize_dial_target(target, LoopbackPolicy::Reject, Some(on_link)),
            Ok(())
        );
        for source in [
            Some(off_link),
            None,
            Some("fd00::9".parse().expect("source")),
        ] {
            assert_eq!(
                authorize_dial_target(target, LoopbackPolicy::Reject, source),
                Err(DialTargetError::OffLinkIpv6)
            );
        }
        assert_eq!(
            authorize_dial_target(
                endpoint("[2a01:4f8:10:20::9]:43821"),
                LoopbackPolicy::Reject,
                Some(on_link)
            ),
            Err(DialTargetError::OffLinkIpv6)
        );
    }

    #[test]
    fn lan_endpoints_need_a_port() {
        assert_eq!(
            validate_dial_endpoint(endpoint("192.168.1.20:0")),
            Err(DialTargetError::InvalidPort)
        );
        assert_eq!(
            authorize_dial_target(endpoint("100.64.3.4:43821"), LoopbackPolicy::Reject, None),
            Ok(())
        );
    }
}
