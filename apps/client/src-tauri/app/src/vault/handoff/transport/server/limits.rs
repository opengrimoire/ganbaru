//! Connection admission bounds for the coordinator listener.
//!
//! A connection is pending from accept until its TLS handshake shows a linked, unrevoked device
//! certificate. Pending connections (handshakes, enrollment, contact requests, revoked devices)
//! have a small global budget and a smaller per-source budget, so one LAN host cannot exhaust
//! file descriptors or crowd out linked devices. Authenticated connections draw from their own
//! larger budget. Contact messages, which reach the vault database without authentication, also
//! spend a per-source token bucket. Every bound sits far above what a household of linked devices
//! uses.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};
#[cfg(desktop)]
use std::time::{Duration, Instant};

/// Connections still in the handshake or without a linked certificate, across all sources.
pub(in super::super) const MAX_PENDING_CONNECTIONS: usize = 32;
/// Pending connections from one IPv4 address or one IPv6 /64 prefix.
pub(in super::super) const MAX_PENDING_CONNECTIONS_PER_SOURCE: usize = 8;
/// Concurrent connections from linked devices, which are capped at 32 per vault.
pub(in super::super) const MAX_AUTHENTICATED_CONNECTIONS: usize = 64;
/// Contact messages one source may send at once. A requester sends one request and then polls
/// about once a minute, so even a household behind one address stays far below it.
#[cfg(desktop)]
pub(in super::super) const CONTACT_MESSAGE_BURST: u32 = 16;
/// Time in which one source earns another contact message.
#[cfg(desktop)]
pub(in super::super) const CONTACT_MESSAGE_REFILL: Duration = Duration::from_secs(1);
/// Sources whose contact budget is tracked at once. Sources with a full budget are forgotten
/// first, so the map only fills when that many sources are sending at the same time.
#[cfg(desktop)]
const MAX_CONTACT_SOURCES: usize = 1_024;

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

/// Token bucket for the contact messages of one source.
#[cfg(desktop)]
#[derive(Clone, Copy, Debug)]
struct ContactBudget {
    tokens: u32,
    refilled_at: Instant,
}

#[cfg(desktop)]
impl ContactBudget {
    fn full(now: Instant) -> Self {
        Self {
            tokens: CONTACT_MESSAGE_BURST,
            refilled_at: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.refilled_at);
        let earned = elapsed.as_nanos() / CONTACT_MESSAGE_REFILL.as_nanos();
        if earned == 0 {
            return;
        }
        let tokens = u128::from(self.tokens) + earned;
        match u32::try_from(tokens) {
            Ok(tokens) if tokens < CONTACT_MESSAGE_BURST => {
                self.tokens = tokens;
                // `earned` is below the burst here, so it fits a u32.
                self.refilled_at += CONTACT_MESSAGE_REFILL * u32::try_from(earned).unwrap_or(0);
            }
            _ => *self = Self::full(now),
        }
    }

    fn is_full(&self) -> bool {
        self.tokens >= CONTACT_MESSAGE_BURST
    }
}

#[derive(Default)]
struct Counts {
    pending: usize,
    pending_by_source: HashMap<SourceKey, usize>,
    authenticated: usize,
    #[cfg(desktop)]
    contact_budgets: HashMap<SourceKey, ContactBudget>,
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

    /// Spends one contact message from the budget of `source`. Returns false when the source
    /// sent too many recently, or when too many sources are sending at once, and the requester
    /// should be told to retry later.
    #[cfg(desktop)]
    pub(in super::super) fn take_contact_message(&self, source: IpAddr, now: Instant) -> bool {
        let source = SourceKey::from_address(source);
        let mut counts = self.lock();
        let budgets = &mut counts.contact_budgets;
        if !budgets.contains_key(&source) && budgets.len() >= MAX_CONTACT_SOURCES {
            budgets.retain(|_, budget| {
                budget.refill(now);
                !budget.is_full()
            });
            if budgets.len() >= MAX_CONTACT_SOURCES {
                return false;
            }
        }
        let budget = budgets
            .entry(source)
            .or_insert_with(|| ContactBudget::full(now));
        budget.refill(now);
        if budget.tokens == 0 {
            return false;
        }
        budget.tokens -= 1;
        true
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
    fn contact_messages_spend_a_per_source_budget_that_refills() {
        let limits = ConnectionLimits::default();
        let start = Instant::now();
        for _ in 0..CONTACT_MESSAGE_BURST {
            assert!(limits.take_contact_message(v4(10), start));
        }
        assert!(!limits.take_contact_message(v4(10), start), "burst spent");
        assert!(
            limits.take_contact_message(v4(11), start),
            "other sources keep their own budget"
        );
        let almost = start + CONTACT_MESSAGE_REFILL - Duration::from_millis(1);
        assert!(!limits.take_contact_message(v4(10), almost));
        let refilled = start + CONTACT_MESSAGE_REFILL;
        assert!(limits.take_contact_message(v4(10), refilled));
        assert!(!limits.take_contact_message(v4(10), refilled), "one earned");
        let rested = refilled + CONTACT_MESSAGE_REFILL * (CONTACT_MESSAGE_BURST * 4);
        for _ in 0..CONTACT_MESSAGE_BURST {
            assert!(limits.take_contact_message(v4(10), rested));
        }
        assert!(
            !limits.take_contact_message(v4(10), rested),
            "a long rest refills only to the burst"
        );
    }

    #[test]
    fn contact_budgets_forget_rested_sources_when_many_are_tracked() {
        let limits = ConnectionLimits::default();
        let start = Instant::now();
        let source = |index: usize| {
            let [.., high, low] = u32::try_from(index).expect("index").to_be_bytes();
            IpAddr::V4(Ipv4Addr::new(10, 1, high, low))
        };
        for index in 0..MAX_CONTACT_SOURCES {
            assert!(limits.take_contact_message(source(index), start));
        }
        let newcomer = source(MAX_CONTACT_SOURCES);
        assert!(
            !limits.take_contact_message(newcomer, start),
            "every tracked source is still active"
        );
        assert!(
            limits.take_contact_message(source(0), start),
            "tracked sources keep sending"
        );
        let rested = start + CONTACT_MESSAGE_REFILL;
        assert!(limits.take_contact_message(newcomer, rested));
        assert!(limits.lock().contact_budgets.len() <= MAX_CONTACT_SOURCES);
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
