//! Connection budgets, frame memory, and timeouts. A connection holds a pending permit from
//! accept until it authenticates, then an authenticated permit, so unauthenticated peers cannot
//! starve authenticated ones and one source cannot take a whole budget. Authenticated requests
//! also reserve their frame bytes from one global budget, so the relay's buffer memory stays
//! bounded however many connections are open.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Unauthenticated connections the relay holds at once. Each holds at most one small frame, so
/// the whole budget costs little memory.
pub const MAX_PENDING_CONNECTIONS: usize = 64;
/// Unauthenticated connections from one source. Sixteen sources are needed to fill the pending
/// budget, while a household behind one address still connects several devices at once.
pub const MAX_PENDING_CONNECTIONS_PER_SOURCE: usize = 4;
/// Bytes that authenticated requests and their responses may hold at once. With requests and
/// responses of up to 1 MiB each, this admits 16 requests in flight, which a personal relay
/// never needs more of, and keeps buffers far below the memory of a small VPS.
pub const MAX_IN_FLIGHT_FRAME_BYTES: usize = 32 * 1024 * 1024;
/// Longest an authenticated request waits for frame memory before the relay answers busy.
pub const FRAME_MEMORY_WAIT: Duration = Duration::from_secs(10);
/// Longest the body of an authenticated frame may take once its length has arrived.
pub const FRAME_BODY_TIMEOUT: Duration = Duration::from_secs(30);
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

/// A global budget of frame bytes held by authenticated requests. Reservations are granted in
/// arrival order, so large requests are not starved by small ones.
#[derive(Clone, Debug)]
pub struct FrameMemory {
    semaphore: Arc<Semaphore>,
    total: usize,
}

impl FrameMemory {
    /// A budget of `total` bytes.
    pub fn new(total: usize) -> Self {
        let total = total.min(Semaphore::MAX_PERMITS);
        Self {
            semaphore: Arc::new(Semaphore::new(total)),
            total,
        }
    }

    /// Reserves `bytes`, waiting up to `wait` for other requests to release theirs. Returns
    /// `None` when the budget stays short for that long or can never hold `bytes`.
    pub async fn reserve(&self, bytes: usize, wait: Duration) -> Option<FrameReservation> {
        let permits = self.permits(bytes)?;
        let semaphore = Arc::clone(&self.semaphore);
        match tokio::time::timeout(wait, semaphore.acquire_many_owned(permits)).await {
            Ok(Ok(permit)) => Some(FrameReservation { _permit: permit }),
            _ => None,
        }
    }

    /// Reserves `bytes` only if they are free now.
    pub fn try_reserve(&self, bytes: usize) -> Option<FrameReservation> {
        let permits = self.permits(bytes)?;
        Arc::clone(&self.semaphore)
            .try_acquire_many_owned(permits)
            .ok()
            .map(|permit| FrameReservation { _permit: permit })
    }

    /// Bytes not reserved now.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    fn permits(&self, bytes: usize) -> Option<u32> {
        if bytes > self.total {
            return None;
        }
        u32::try_from(bytes.max(1)).ok()
    }
}

/// Frame bytes reserved from a [`FrameMemory`], released on drop.
#[derive(Debug)]
pub struct FrameReservation {
    _permit: OwnedSemaphorePermit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[tokio::test]
    async fn frame_memory_bounds_reservations_and_releases_on_drop() {
        let memory = FrameMemory::new(100);
        let first = memory.reserve(60, Duration::from_millis(10)).await.unwrap();
        assert_eq!(memory.available(), 40);
        // A request larger than what is free waits, then gives up.
        assert!(
            memory
                .reserve(50, Duration::from_millis(20))
                .await
                .is_none()
        );
        assert!(memory.try_reserve(50).is_none());
        // More than the whole budget is never granted.
        assert!(memory.reserve(101, Duration::from_secs(5)).await.is_none());
        let small = memory.try_reserve(40).unwrap();
        assert_eq!(memory.available(), 0);
        drop(first);
        drop(small);
        assert_eq!(memory.available(), 100);
        assert!(memory.try_reserve(100).is_some());
    }

    #[tokio::test]
    async fn a_waiting_reservation_is_granted_once_memory_is_released() {
        let memory = FrameMemory::new(10);
        let held = memory.try_reserve(10).unwrap();
        let waiter = {
            let memory = memory.clone();
            tokio::spawn(async move { memory.reserve(8, Duration::from_secs(5)).await.is_some() })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(held);
        assert!(waiter.await.unwrap());
    }

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
