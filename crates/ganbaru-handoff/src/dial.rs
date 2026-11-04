//! Address policy for every peer address of handoff, sync, and contact connections.
//!
//! Invitations and contact cards arrive from untrusted QR codes or pasted text, so the address
//! they carry decides where this device connects, and the listener accepts connections from any
//! host that can route to it. Linking works over local and overlay networks only: private,
//! link-local, and overlay addresses are dialed and admitted, and global IPv6 only when it
//! shares the on-link prefix of this device's own address on that link. One rule set decides
//! outgoing dials ([`authorize_dial_target`]), inbound peers before TLS
//! ([`admit_inbound_peer`]), and the addresses a device reports about itself
//! ([`reportable_address`]). Linking across the internet belongs to the planned relay.

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

/// Whether loopback peers may be dialed or admitted. Production builds reject them.
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
    LinkLocalIpv6,
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
            Self::LinkLocalIpv6 => "the device address is valid only on its own network link",
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
        DialAddressClass::Loopback => allow_loopback(loopback),
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

/// Decides whether this device may connect to `endpoint` of the sync server that the space's
/// sequenced, admin-signed server configuration names. It is the one exception to the LAN-only
/// rule of [`authorize_dial_target`]: public addresses and global IPv6 off the local link are
/// allowed, because the pinned certificate fingerprint decides the server's identity.
/// Unspecified, multicast, broadcast, and documentation addresses stay refused, and loopback
/// follows `loopback`. Callers pass only endpoints read from that configuration, never one
/// from an invitation, contact card, or peer report.
pub fn authorize_server_dial_target(
    endpoint: SocketAddr,
    loopback: LoopbackPolicy,
) -> Result<(), DialTargetError> {
    match validate_dial_endpoint(endpoint) {
        Ok(DialAddressClass::Loopback) => allow_loopback(loopback),
        Ok(_) | Err(DialTargetError::Public | DialTargetError::OffLinkIpv6) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Decides whether the listener may serve a connection from `peer` that arrived on `local`, the
/// accepted socket's own address. It runs right after accept, before any byte is read.
///
/// Inbound peers follow the dial classes. A global IPv6 peer is admitted only when it shares the
/// on-link prefix of `local`, so the decision needs no route lookup. IPv4-mapped IPv6 addresses
/// count as their IPv4 address on either side.
pub fn admit_inbound_peer(
    peer: IpAddr,
    local: IpAddr,
    loopback: LoopbackPolicy,
) -> Result<(), DialTargetError> {
    match classify_dial_address(peer)? {
        DialAddressClass::PrivateLan | DialAddressClass::Overlay => Ok(()),
        DialAddressClass::Loopback => allow_loopback(loopback),
        DialAddressClass::GlobalIpv6 => match (peer.to_canonical(), local.to_canonical()) {
            (IpAddr::V6(peer), IpAddr::V6(local)) if shares_on_link_prefix(peer, local) => Ok(()),
            _ => Err(DialTargetError::OffLinkIpv6),
        },
    }
}

/// Decides whether this device may name `address` as one of its own to other devices, in
/// invitations, reported endpoints, and discovery records.
///
/// Every dialable class is reportable except loopback, which names the receiving device, and
/// IPv6 link-local, whose scope identifier is meaningless on another host. Global IPv6 is
/// reportable because each receiver checks its own on-link prefix before dialing it.
pub fn reportable_address(address: IpAddr) -> Result<DialAddressClass, DialTargetError> {
    let class = classify_dial_address(address)?;
    match (class, address.to_canonical()) {
        (DialAddressClass::Loopback, _) => Err(DialTargetError::Loopback),
        (_, IpAddr::V6(address)) if address.is_unicast_link_local() => {
            Err(DialTargetError::LinkLocalIpv6)
        }
        _ => Ok(class),
    }
}

fn allow_loopback(policy: LoopbackPolicy) -> Result<(), DialTargetError> {
    match policy {
        LoopbackPolicy::Allow => Ok(()),
        LoopbackPolicy::Reject => Err(DialTargetError::Loopback),
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

/// Unique local IPv6 block, as network and prefix length.
const UNIQUE_LOCAL_IPV6_BLOCK: (Ipv6Addr, u32) = (Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7);
/// Link-local IPv6 block, as network and prefix length.
const LINK_LOCAL_IPV6_BLOCK: (Ipv6Addr, u32) = (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10);

/// Network mask of an IPv6 prefix length of at most 128.
pub fn ipv6_mask(prefix: u32) -> u128 {
    u128::MAX.checked_shl(128 - prefix.min(128)).unwrap_or(0)
}

/// The IPv6 range that holds `address` as a LAN peer, as network and prefix length: the unique
/// local or link-local block, or for global unicast its own on-link prefix, so a range built
/// from a global address never extends past the local link.
pub fn ipv6_lan_block(address: Ipv6Addr) -> Option<(Ipv6Addr, u32)> {
    match classify_ipv6(address).ok()? {
        DialAddressClass::PrivateLan if address.is_unicast_link_local() => {
            Some(LINK_LOCAL_IPV6_BLOCK)
        }
        DialAddressClass::PrivateLan => Some(UNIQUE_LOCAL_IPV6_BLOCK),
        DialAddressClass::GlobalIpv6 => {
            let mask = ipv6_mask(ON_LINK_IPV6_PREFIX_BITS);
            Some((
                Ipv6Addr::from(u128::from(address) & mask),
                ON_LINK_IPV6_PREFIX_BITS,
            ))
        }
        DialAddressClass::Overlay | DialAddressClass::Loopback => None,
    }
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

    fn address(text: &str) -> IpAddr {
        text.parse().expect("address")
    }

    fn admit(peer: &str, local: &str, loopback: LoopbackPolicy) -> Result<(), DialTargetError> {
        admit_inbound_peer(address(peer), address(local), loopback)
    }

    #[test]
    fn inbound_peers_follow_the_dial_classes() {
        for (peer, local) in [
            ("10.0.0.4", "10.0.0.2"),
            ("172.16.0.1", "172.16.0.2"),
            ("192.168.1.20", "192.168.1.2"),
            ("169.254.10.2", "169.254.10.3"),
            ("100.101.102.103", "100.64.0.1"),
            ("fd12:3456::1", "fd12:3456::2"),
            ("fe80::1", "fe80::2"),
            ("::ffff:192.168.1.20", "::ffff:192.168.1.2"),
            ("::ffff:100.64.0.9", "192.168.1.2"),
        ] {
            assert_eq!(
                admit(peer, local, LoopbackPolicy::Reject),
                Ok(()),
                "{peer} on {local}"
            );
        }
        let refused = [
            ("8.8.8.8", DialTargetError::Public),
            ("::ffff:8.8.8.8", DialTargetError::Public),
            ("100.128.0.1", DialTargetError::Public),
            ("64:ff9b::808:808", DialTargetError::Public),
            ("0.0.0.0", DialTargetError::Unspecified),
            ("::", DialTargetError::Unspecified),
            ("224.0.0.251", DialTargetError::Multicast),
            ("ff02::fb", DialTargetError::Multicast),
            ("255.255.255.255", DialTargetError::Broadcast),
            ("192.0.2.1", DialTargetError::Documentation),
            ("2001:db8::1", DialTargetError::Documentation),
        ];
        for (peer, expected) in refused {
            assert_eq!(
                admit(peer, "192.168.1.2", LoopbackPolicy::Allow),
                Err(expected),
                "{peer}"
            );
        }
    }

    #[test]
    fn loopback_peers_are_admitted_only_when_allowed() {
        for peer in ["127.0.0.1", "127.0.0.2", "::1", "::ffff:127.0.0.1"] {
            assert_eq!(admit(peer, "127.0.0.1", LoopbackPolicy::Allow), Ok(()));
            assert_eq!(
                admit(peer, "127.0.0.1", LoopbackPolicy::Reject),
                Err(DialTargetError::Loopback),
                "{peer}"
            );
        }
    }

    #[test]
    fn global_ipv6_peers_are_admitted_only_inside_the_local_prefix() {
        let peer = "2a01:4f8:10:20::5";
        assert_eq!(
            admit(peer, "2a01:4f8:10:20::9", LoopbackPolicy::Reject),
            Ok(())
        );
        for local in [
            "2a01:4f8:10:21::9",
            "fd00::9",
            "fe80::9",
            "192.168.1.2",
            "::ffff:192.168.1.2",
            // The same address on both ends is this device, not a peer on its link.
            "2a01:4f8:10:20::5",
        ] {
            assert_eq!(
                admit(peer, local, LoopbackPolicy::Allow),
                Err(DialTargetError::OffLinkIpv6),
                "{local}"
            );
        }
    }

    #[test]
    fn reported_addresses_exclude_loopback_and_ipv6_link_local() {
        let reportable = [
            ("10.0.0.4", DialAddressClass::PrivateLan),
            ("172.31.255.254", DialAddressClass::PrivateLan),
            ("192.168.1.20", DialAddressClass::PrivateLan),
            ("169.254.10.2", DialAddressClass::PrivateLan),
            ("::ffff:192.168.1.20", DialAddressClass::PrivateLan),
            ("fd12:3456::1", DialAddressClass::PrivateLan),
            ("100.100.100.100", DialAddressClass::Overlay),
            ("2a01:4f8:10:20::5", DialAddressClass::GlobalIpv6),
        ];
        for (text, expected) in reportable {
            assert_eq!(reportable_address(address(text)), Ok(expected), "{text}");
        }
        let refused = [
            ("127.0.0.1", DialTargetError::Loopback),
            ("::1", DialTargetError::Loopback),
            ("::ffff:127.0.0.1", DialTargetError::Loopback),
            ("fe80::1", DialTargetError::LinkLocalIpv6),
            ("fe80::1c2d:3eff:fe4f:5a6b", DialTargetError::LinkLocalIpv6),
            ("2001:db8::1", DialTargetError::Documentation),
            ("192.0.2.1", DialTargetError::Documentation),
            ("8.8.8.8", DialTargetError::Public),
            ("100.128.0.1", DialTargetError::Public),
            ("0.0.0.0", DialTargetError::Unspecified),
            ("::", DialTargetError::Unspecified),
            ("224.0.0.251", DialTargetError::Multicast),
            ("ff02::fb", DialTargetError::Multicast),
        ];
        for (text, expected) in refused {
            assert_eq!(reportable_address(address(text)), Err(expected), "{text}");
        }
    }

    #[test]
    fn ipv6_lan_blocks_stop_at_the_local_link_for_global_addresses() {
        let block = |text: &str| {
            ipv6_lan_block(text.parse().expect("address"))
                .map(|(network, prefix)| format!("{network}/{prefix}"))
        };
        assert_eq!(block("fd12:3456::1").as_deref(), Some("fc00::/7"));
        assert_eq!(
            block("fe80::1c2d:3eff:fe4f:5a6b").as_deref(),
            Some("fe80::/10")
        );
        assert_eq!(
            block("2806:101e:e:8970:4a89:e7ff:fecf:5d55").as_deref(),
            Some("2806:101e:e:8970::/64")
        );
        for refused in ["2001:db8::1", "::1", "ff02::fb", "::ffff:192.168.1.2", "::"] {
            assert_eq!(block(refused), None, "{refused}");
        }
        assert_eq!(ipv6_mask(0), 0);
        assert_eq!(ipv6_mask(128), u128::MAX);
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

    #[test]
    fn configured_server_dials_allow_public_addresses_that_peer_dials_refuse() {
        for text in [
            "203.0.114.7:43823",
            "8.8.8.8:43823",
            "[2606:4700::1111]:43823",
            "192.168.1.20:43823",
            "100.64.3.4:43823",
            "[fd00::20]:43823",
        ] {
            assert_eq!(
                authorize_server_dial_target(endpoint(text), LoopbackPolicy::Reject),
                Ok(()),
                "{text}"
            );
        }
        // The same public targets stay refused for every other dial.
        assert_eq!(
            authorize_dial_target(endpoint("8.8.8.8:43823"), LoopbackPolicy::Allow, None),
            Err(DialTargetError::Public)
        );
        assert_eq!(
            authorize_dial_target(
                endpoint("[2606:4700::1111]:43823"),
                LoopbackPolicy::Allow,
                None
            ),
            Err(DialTargetError::OffLinkIpv6)
        );
    }

    #[test]
    fn configured_server_dials_refuse_unroutable_targets_and_loopback_by_policy() {
        for (text, error) in [
            ("0.0.0.0:43823", DialTargetError::Unspecified),
            ("[::]:43823", DialTargetError::Unspecified),
            ("224.0.0.1:43823", DialTargetError::Multicast),
            ("[ff02::1]:43823", DialTargetError::Multicast),
            ("255.255.255.255:43823", DialTargetError::Broadcast),
            ("192.0.2.1:43823", DialTargetError::Documentation),
            ("[2001:db8::1]:43823", DialTargetError::Documentation),
            ("8.8.8.8:0", DialTargetError::InvalidPort),
            ("127.0.0.1:43823", DialTargetError::Loopback),
            ("[::1]:43823", DialTargetError::Loopback),
            ("[::ffff:127.0.0.1]:43823", DialTargetError::Loopback),
        ] {
            assert_eq!(
                authorize_server_dial_target(endpoint(text), LoopbackPolicy::Reject),
                Err(error),
                "{text}"
            );
        }
        assert_eq!(
            authorize_server_dial_target(endpoint("127.0.0.1:43823"), LoopbackPolicy::Allow),
            Ok(())
        );
    }
}
