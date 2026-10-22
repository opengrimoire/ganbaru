//! Network admission for the coordinator listener.
//!
//! On Linux the firewall rules for linking match an interface, address, and subnet, so another
//! network that hands out the same ones would reach the coordinator through rules the user
//! allowed elsewhere. The listener therefore asks whether the network behind the address a
//! connection arrived on was confirmed before serving a LAN peer, and closes the connection
//! before the TLS handshake when it was not. The wildcard listener serves several local
//! addresses at once, so each keeps its own decision, cached briefly because reading it touches
//! the routing tables and the firewall.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// How long an admission decision is reused before the network is read again.
const ADMISSION_RECHECK: Duration = Duration::from_secs(5);

/// Local addresses whose decisions are cached at once. A host rarely has more; past the cap the
/// oldest decision is read again on its next use.
const CACHED_LOCAL_ADDRESSES: usize = 8;

type Decide = dyn Fn(SocketAddr) -> bool + Send + Sync;

/// Decides whether the listener serves LAN peers that reach it on a local address. The default
/// admits every peer, for platforms without per-network firewall rules.
#[derive(Clone, Default)]
pub(crate) struct LanAdmission(Option<Arc<Gate>>);

struct Gate {
    decide: Arc<Decide>,
    cached: Mutex<DecisionCache>,
}

#[derive(Clone, Copy)]
struct Decision {
    admits: bool,
    at: Instant,
}

/// Recent decisions per local address, at most [`CACHED_LOCAL_ADDRESSES`] of them.
#[derive(Default)]
struct DecisionCache(Vec<(IpAddr, Decision)>);

impl DecisionCache {
    fn fresh(&self, local: IpAddr, now: Instant) -> Option<bool> {
        self.0
            .iter()
            .find(|(address, decision)| {
                *address == local && now.saturating_duration_since(decision.at) < ADMISSION_RECHECK
            })
            .map(|(_, decision)| decision.admits)
    }

    /// Stores `decision` for `local` and returns the decision it replaced.
    fn record(&mut self, local: IpAddr, decision: Decision) -> Option<Decision> {
        if let Some((_, cached)) = self.0.iter_mut().find(|(address, _)| *address == local) {
            return Some(std::mem::replace(cached, decision));
        }
        if self.0.len() >= CACHED_LOCAL_ADDRESSES
            && let Some(oldest) = self
                .0
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, decision))| decision.at)
                .map(|(index, _)| index)
        {
            self.0.swap_remove(oldest);
        }
        self.0.push((local, decision));
        None
    }
}

impl LanAdmission {
    /// Admits LAN peers only while the Linux firewall rules for the local address a peer
    /// reached and `port` are allowed on the current network.
    #[cfg(target_os = "linux")]
    pub(crate) fn linux_firewall(config_dir: std::path::PathBuf, port: u16) -> Self {
        Self::deciding(move |local| {
            ganbaru_handoff::network_access::admits_lan_peers(&config_dir, local.ip(), port)
        })
    }

    /// Admits LAN peers while `decide` returns true for the local address they reached.
    /// `decide` may block; it runs off the async runtime.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub(crate) fn deciding(decide: impl Fn(SocketAddr) -> bool + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(Gate {
            decide: Arc::new(decide),
            cached: Mutex::new(DecisionCache::default()),
        })))
    }

    /// Whether a LAN peer that reached the listener on `local` may be served now. A decision
    /// that cannot be read admits the peer, because refusing would break linking with no action
    /// that lifts it.
    pub(crate) async fn admits(&self, local: SocketAddr) -> bool {
        let Some(gate) = &self.0 else {
            return true;
        };
        let mut cached = gate.cached.lock().await;
        if let Some(admits) = cached.fresh(local.ip(), Instant::now()) {
            return admits;
        }
        let decide = gate.decide.clone();
        let admits = tokio::task::spawn_blocking(move || decide(local))
            .await
            .unwrap_or_else(|error| {
                eprintln!("vault handoff network admission failed: {error}");
                true
            });
        let previous = cached.record(
            local.ip(),
            Decision {
                admits,
                at: Instant::now(),
            },
        );
        if previous.is_none_or(|previous| previous.admits != admits) {
            if admits {
                eprintln!("vault handoff coordinator serves peers on this network");
            } else {
                eprintln!(
                    "vault handoff coordinator refuses peers until this network is confirmed"
                );
            }
        }
        admits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn local(address: &str) -> SocketAddr {
        SocketAddr::new(address.parse().expect("address"), 43_821)
    }

    #[tokio::test]
    async fn the_default_admits_and_a_gate_reuses_its_decision_briefly() {
        assert!(LanAdmission::default().admits(local("192.168.1.2")).await);

        let allowed = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let admission = LanAdmission::deciding({
            let allowed = allowed.clone();
            let reads = reads.clone();
            move |_| {
                reads.fetch_add(1, Ordering::SeqCst);
                allowed.load(Ordering::SeqCst)
            }
        });
        assert!(!admission.admits(local("192.168.1.2")).await);
        allowed.store(true, Ordering::SeqCst);
        assert!(!admission.admits(local("192.168.1.2")).await);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn each_local_address_keeps_its_own_decision() {
        let admission = LanAdmission::deciding(|local| local.is_ipv4());
        assert!(admission.admits(local("192.168.1.2")).await);
        assert!(!admission.admits(local("fd00::2")).await);
        assert!(
            admission.admits(local("192.168.1.2")).await,
            "a refusal on one address leaves the cached decision of another"
        );
        assert!(admission.admits(local("100.80.0.4")).await);
    }

    #[tokio::test]
    async fn a_failed_decision_admits() {
        let admission = LanAdmission::deciding(|_| panic!("the network could not be read"));
        assert!(admission.admits(local("192.168.1.2")).await);
    }

    #[test]
    fn the_cache_keeps_at_most_eight_addresses_and_evicts_the_oldest() {
        let start = Instant::now();
        let mut cache = DecisionCache::default();
        let address = |index: u8| IpAddr::from([10, 0, 0, index]);
        for index in 0..=8 {
            let decision = Decision {
                admits: true,
                at: start + Duration::from_millis(u64::from(index)),
            };
            assert!(cache.record(address(index), decision).is_none());
        }
        assert_eq!(cache.0.len(), CACHED_LOCAL_ADDRESSES);
        let now = start + Duration::from_secs(1);
        assert_eq!(
            cache.fresh(address(0), now),
            None,
            "the oldest entry was evicted"
        );
        assert_eq!(cache.fresh(address(8), now), Some(true));

        let replaced = cache.record(
            address(8),
            Decision {
                admits: false,
                at: now,
            },
        );
        assert!(replaced.is_some_and(|previous| previous.admits));
        assert_eq!(cache.0.len(), CACHED_LOCAL_ADDRESSES);
        assert_eq!(cache.fresh(address(8), now), Some(false));
        assert_eq!(
            cache.fresh(address(8), now + ADMISSION_RECHECK),
            None,
            "a decision is read again after the recheck interval"
        );
    }
}
