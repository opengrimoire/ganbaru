//! Connection admission bounds for the coordinator listener.
//!
//! A connection is pending from accept until its TLS handshake shows a linked, unrevoked device
//! certificate. Pending connections (handshakes, enrollment, contact requests, revoked devices)
//! have a small global budget and a smaller per-source budget, so one LAN host cannot exhaust
//! file descriptors or crowd out linked devices. Authenticated connections draw from their own
//! larger budget. Every bound sits far above what a household of linked devices uses.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};

/// Connections still in the handshake or without a linked certificate, across all sources.
pub(in super::super) const MAX_PENDING_CONNECTIONS: usize = 32;
/// Pending connections from one IPv4 address or one IPv6 /64 prefix.
pub(in super::super) const MAX_PENDING_CONNECTIONS_PER_SOURCE: usize = 8;
/// Concurrent connections from linked devices, which are capped at 32 per vault.
pub(in super::super) const MAX_AUTHENTICATED_CONNECTIONS: usize = 64;

/// Groups addresses one host can cheaply rotate through: IPv6 hosts pick any address in their
/// /64, and IPv4-mapped IPv6 addresses count as the IPv4 host.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SourceKey {
    V4([u8; 4]),
    V6Prefix([u8; 8]),
}

impl SourceKey {
    fn from_address(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(address) => Self::V4(address.octets()),
            IpAddr::V6(address) => match address.to_ipv4_mapped() {
                Some(mapped) => Self::V4(mapped.octets()),
                None => {
                    let mut prefix = [0_u8; 8];
                    prefix.copy_from_slice(&address.octets()[..8]);
                    Self::V6Prefix(prefix)
                }
            },
        }
    }
}

#[derive(Default)]
struct Counts {
    pending: usize,
    pending_by_source: HashMap<SourceKey, usize>,
    authenticated: usize,
}

/// Shared admission state for one listener.
#[derive(Clone)]
pub(in super::super) struct ConnectionLimits {
    counts: Arc<Mutex<Counts>>,
    max_pending: usize,
    max_pending_per_source: usize,
    max_authenticated: usize,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self::new(
            MAX_PENDING_CONNECTIONS,
            MAX_PENDING_CONNECTIONS_PER_SOURCE,
            MAX_AUTHENTICATED_CONNECTIONS,
        )
    }
}

impl ConnectionLimits {
    pub(in super::super) fn new(
        max_pending: usize,
        max_pending_per_source: usize,
        max_authenticated: usize,
    ) -> Self {
        Self {
            counts: Arc::default(),
            max_pending,
            max_pending_per_source,
            max_authenticated,
        }
    }

    /// Admits a newly accepted connection as pending, or returns `None` when the global or
    /// per-source pending budget is full and the socket should be closed.
    pub(in super::super) fn admit(&self, source: IpAddr) -> Option<PendingPermit> {
        let source = SourceKey::from_address(source);
        let mut counts = self.lock();
        let from_source = counts.pending_by_source.get(&source).copied().unwrap_or(0);
        if counts.pending >= self.max_pending || from_source >= self.max_pending_per_source {
            return None;
        }
        counts.pending += 1;
        counts.pending_by_source.insert(source, from_source + 1);
        Some(PendingPermit {
            limits: self.clone(),
            source,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Counts> {
        // Counts stay consistent under poisoning because every update is a single step.
        self.counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Holds a pending slot until dropped or exchanged for an authenticated slot.
pub(in super::super) struct PendingPermit {
    limits: ConnectionLimits,
    source: SourceKey,
}

impl PendingPermit {
    /// Moves the connection to the authenticated budget and releases its pending slot. Returns
    /// `None` when the authenticated budget is full; the pending slot is released either way.
    pub(in super::super) fn authenticate(self) -> Option<AuthenticatedPermit> {
        let limits = self.limits.clone();
        let mut counts = limits.lock();
        let admitted = counts.authenticated < limits.max_authenticated;
        if admitted {
            counts.authenticated += 1;
        }
        // Release the lock before `self` drops, because releasing the pending slot locks again.
        drop(counts);
        // Build the permit only when admitted, since dropping one releases an authenticated slot.
        if admitted {
            Some(AuthenticatedPermit { limits })
        } else {
            None
        }
    }
}

impl Drop for PendingPermit {
    fn drop(&mut self) {
        let mut counts = self.limits.lock();
        counts.pending = counts.pending.saturating_sub(1);
        if let Some(from_source) = counts.pending_by_source.get_mut(&self.source) {
            *from_source = from_source.saturating_sub(1);
            if *from_source == 0 {
                counts.pending_by_source.remove(&self.source);
            }
        }
    }
}

/// Holds an authenticated slot until dropped.
pub(in super::super) struct AuthenticatedPermit {
    limits: ConnectionLimits,
}

impl Drop for AuthenticatedPermit {
    fn drop(&mut self) {
        let mut counts = self.limits.lock();
        counts.authenticated = counts.authenticated.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
    }

    #[test]
    fn pending_connections_are_bounded_per_source_and_globally() {
        let limits = ConnectionLimits::new(5, 2, 4);
        let first = limits.admit(v4(10)).expect("first from source");
        let _second = limits.admit(v4(10)).expect("second from source");
        assert!(limits.admit(v4(10)).is_none(), "per-source bound");
        let _third = limits.admit(v4(11)).expect("other source");
        let _fourth = limits.admit(v4(11)).expect("other source");
        let _fifth = limits.admit(v4(12)).expect("third source");
        assert!(limits.admit(v4(13)).is_none(), "global bound");
        drop(first);
        assert!(limits.admit(v4(10)).is_some(), "released slot is reusable");
    }

    #[test]
    fn ipv6_sources_are_grouped_by_prefix_and_mapped_ipv4_by_host() {
        let limits = ConnectionLimits::new(10, 1, 4);
        let _held = limits
            .admit("fd00:1:2:3::1".parse().expect("address"))
            .expect("first in prefix");
        assert!(
            limits
                .admit("fd00:1:2:3:ffff::9".parse().expect("address"))
                .is_none(),
            "another address in the same /64"
        );
        assert!(
            limits
                .admit("fd00:1:2:4::1".parse().expect("address"))
                .is_some()
        );
        let _mapped = limits
            .admit(IpAddr::V6(Ipv4Addr::new(10, 0, 0, 5).to_ipv6_mapped()))
            .expect("mapped IPv4");
        assert!(
            limits
                .admit(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)))
                .is_none()
        );
        assert!(limits.admit(IpAddr::V6(Ipv6Addr::LOCALHOST)).is_some());
    }

    #[test]
    fn authentication_frees_the_pending_slot_and_respects_its_own_bound() {
        let limits = ConnectionLimits::new(1, 1, 1);
        let authenticated = limits
            .admit(v4(10))
            .expect("pending")
            .authenticate()
            .expect("authenticated slot");
        let pending = limits.admit(v4(10)).expect("pending slot was released");
        assert!(pending.authenticate().is_none(), "authenticated bound");
        assert!(
            limits.admit(v4(10)).is_some(),
            "rejected upgrade releases its slot"
        );
        drop(authenticated);
        assert!(
            limits
                .admit(v4(10))
                .and_then(PendingPermit::authenticate)
                .is_some()
        );
    }
}
