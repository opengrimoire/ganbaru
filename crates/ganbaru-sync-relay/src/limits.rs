//! Connection budgets and timeouts. A connection holds a pending permit from accept until it
//! authenticates, then an authenticated permit, so unauthenticated peers cannot starve
//! authenticated ones and one source cannot take a whole budget.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Unauthenticated connections the relay holds at once.
pub const MAX_PENDING_CONNECTIONS: usize = 32;
/// Unauthenticated connections from one source.
pub const MAX_PENDING_CONNECTIONS_PER_SOURCE: usize = 8;
/// Authenticated connections the relay holds at once.
pub const MAX_AUTHENTICATED_CONNECTIONS: usize = 256;
/// Authenticated connections from one source.
pub const MAX_AUTHENTICATED_CONNECTIONS_PER_SOURCE: usize = 16;
/// Longest a TLS handshake may take.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest wait for one frame before the connection authenticates.
pub const PRE_AUTH_FRAME_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest an unauthenticated connection stays open.
pub const PRE_AUTH_DEADLINE: Duration = Duration::from_secs(30);
/// Longest an authenticated connection may stay silent; above the relay wait timeout so a
/// device can keep one wait outstanding.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest a response write may take.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// The unit a per-source budget counts: an IPv4 address or an IPv6 /64.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKey {
    /// An IPv4 address, including an IPv4-mapped IPv6 address.
    V4([u8; 4]),
    /// The first 64 bits of an IPv6 address.
    V6([u8; 8]),
}

impl SourceKey {
    /// The budget unit of a peer address.
    pub fn of(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(address) => Self::V4(address.octets()),
            IpAddr::V6(address) => match address.to_ipv4_mapped() {
                Some(address) => Self::V4(address.octets()),
                None => {
                    let mut prefix = [0u8; 8];
                    prefix.copy_from_slice(&address.octets()[..8]);
                    Self::V6(prefix)
                }
            },
        }
    }
}

#[derive(Debug, Default)]
struct BudgetState {
    total: usize,
    per_source: HashMap<SourceKey, usize>,
}

/// A connection budget with a global and a per-source limit.
#[derive(Clone, Debug)]
pub struct ConnectionBudget {
    state: Arc<Mutex<BudgetState>>,
    max_total: usize,
    max_per_source: usize,
}

impl ConnectionBudget {
    /// A budget of `max_total` connections and `max_per_source` per source.
    pub fn new(max_total: usize, max_per_source: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(BudgetState::default())),
            max_total,
            max_per_source,
        }
    }

    /// Takes one slot for `source`, or `None` when either limit is reached.
    pub fn try_acquire(&self, source: SourceKey) -> Option<BudgetPermit> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let held = state.per_source.get(&source).copied().unwrap_or(0);
        if state.total >= self.max_total || held >= self.max_per_source {
            return None;
        }
        state.total += 1;
        state.per_source.insert(source, held + 1);
        Some(BudgetPermit {
            state: Arc::clone(&self.state),
            source,
        })
    }

    /// Slots in use.
    pub fn in_use(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .total
    }
}

/// One slot of a [`ConnectionBudget`], released on drop.
#[derive(Debug)]
pub struct BudgetPermit {
    state: Arc<Mutex<BudgetState>>,
    source: SourceKey,
}

impl Drop for BudgetPermit {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total = state.total.saturating_sub(1);
        if let Some(held) = state.per_source.get_mut(&self.source) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                state.per_source.remove(&self.source);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn budgets_limit_each_source_and_the_total_and_release_on_drop() {
        let budget = ConnectionBudget::new(3, 2);
        let first = SourceKey::of(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)));
        let second = SourceKey::of(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)));
        let a = budget.try_acquire(first).unwrap();
        let _b = budget.try_acquire(first).unwrap();
        assert!(budget.try_acquire(first).is_none());
        let _c = budget.try_acquire(second).unwrap();
        assert!(budget.try_acquire(second).is_none());
        drop(a);
        assert_eq!(budget.in_use(), 2);
        assert!(budget.try_acquire(first).is_some());
    }

    #[test]
    fn ipv6_sources_share_a_slash_64_and_mapped_ipv4_counts_as_ipv4() {
        let one: Ipv6Addr = "2001:db8:1:2::1".parse().unwrap();
        let two: Ipv6Addr = "2001:db8:1:2:ffff::9".parse().unwrap();
        assert_eq!(SourceKey::of(one.into()), SourceKey::of(two.into()));
        let mapped = Ipv4Addr::new(198, 51, 100, 7).to_ipv6_mapped();
        assert_eq!(
            SourceKey::of(mapped.into()),
            SourceKey::of(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)))
        );
    }
}
