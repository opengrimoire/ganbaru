//! Network admission for the coordinator listener.
//!
//! On Linux the firewall rules for linking match an interface, address, and subnet, so another
//! network that hands out the same ones would reach the coordinator through rules the user
//! allowed elsewhere. The listener therefore asks whether the current network was confirmed
//! before serving a LAN peer, and closes the connection before the TLS handshake when it was not.
//! The answer is cached briefly because reading it touches the routing tables and the firewall.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// How long an admission decision is reused before the network is read again.
const ADMISSION_RECHECK: Duration = Duration::from_secs(5);

type Decide = dyn Fn() -> bool + Send + Sync;

/// Decides whether the listener serves LAN peers on the current network. The default admits
/// every peer, for platforms and addresses without per-network firewall rules.
#[derive(Clone, Default)]
pub(crate) struct LanAdmission(Option<Arc<Gate>>);

struct Gate {
    decide: Arc<Decide>,
    cached: Mutex<Option<Decision>>,
}

#[derive(Clone, Copy)]
struct Decision {
    admits: bool,
    at: Instant,
}

impl LanAdmission {
    /// Admits LAN peers only while the Linux firewall rules for `local_address` and `port` are
    /// allowed on the current network.
    #[cfg(target_os = "linux")]
    pub(crate) fn linux_firewall(
        config_dir: std::path::PathBuf,
        local_address: std::net::Ipv4Addr,
        port: u16,
    ) -> Self {
        Self::deciding(move || {
            ganbaru_handoff::network_access::admits_lan_peers(&config_dir, local_address, port)
        })
    }

    /// Admits LAN peers while `decide` returns true. `decide` may block; it runs off the async
    /// runtime.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub(crate) fn deciding(decide: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(Gate {
            decide: Arc::new(decide),
            cached: Mutex::new(None),
        })))
    }

    /// Whether a LAN peer may be served now. A decision that cannot be read admits the peer,
    /// because refusing would break linking with no action that lifts it.
    pub(crate) async fn admits(&self) -> bool {
        let Some(gate) = &self.0 else {
            return true;
        };
        let mut cached = gate.cached.lock().await;
        if let Some(decision) = *cached
            && decision.at.elapsed() < ADMISSION_RECHECK
        {
            return decision.admits;
        }
        let decide = gate.decide.clone();
        let admits = tokio::task::spawn_blocking(move || decide())
            .await
            .unwrap_or_else(|error| {
                eprintln!("vault handoff network admission failed: {error}");
                true
            });
        if cached.is_none_or(|previous| previous.admits != admits) {
            if admits {
                eprintln!("vault handoff coordinator serves peers on this network");
            } else {
                eprintln!(
                    "vault handoff coordinator refuses peers until this network is confirmed"
                );
            }
        }
        *cached = Some(Decision {
            admits,
            at: Instant::now(),
        });
        admits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[tokio::test]
    async fn the_default_admits_and_a_gate_reuses_its_decision_briefly() {
        assert!(LanAdmission::default().admits().await);

        let allowed = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let admission = LanAdmission::deciding({
            let allowed = allowed.clone();
            let reads = reads.clone();
            move || {
                reads.fetch_add(1, Ordering::SeqCst);
                allowed.load(Ordering::SeqCst)
            }
        });
        assert!(!admission.admits().await);
        allowed.store(true, Ordering::SeqCst);
        assert!(!admission.admits().await);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_decision_admits() {
        let admission = LanAdmission::deciding(|| panic!("the network could not be read"));
        assert!(admission.admits().await);
    }
}
