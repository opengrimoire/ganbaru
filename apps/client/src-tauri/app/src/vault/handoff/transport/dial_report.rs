//! What the coordinator dials of this app run found, so the sync status can name the network
//! it reached the coordinator over and why it could not. Kept in memory only.

use super::super::pairing::CoordinatorPin;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Mutex;

/// Coordinator dial outcomes of this run, one per coordinator dialed.
static DIAL_REPORTS: Mutex<DialReports> = Mutex::new(DialReports::new());

/// What the dials of one coordinator found in this run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoordinatorDialReport {
    /// The endpoint the latest successful dial reached.
    pub(crate) reached: Option<SocketAddr>,
    /// The latest dial reached no endpoint, and the system refused at least one connection
    /// with a permission error, as Android does when local network access is denied.
    pub(crate) local_network_denied: bool,
    /// The latest dial reached no endpoint after a browse of the local network found none.
    pub(crate) discovery_found_nothing: bool,
}

/// How one dial ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DialOutcome {
    Reached(SocketAddr),
    Failed {
        local_network_denied: bool,
        discovery_found_nothing: bool,
    },
}

/// The coordinator a report belongs to.
type DialTarget = (String, String);

/// Reports by coordinator vault and certificate fingerprint.
#[derive(Debug)]
pub(super) struct DialReports(BTreeMap<DialTarget, CoordinatorDialReport>);

impl DialReports {
    const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Folds `outcome` into the report of the coordinator of `vault_id` with `fingerprint`. A
    /// failure keeps the endpoint an earlier dial reached.
    pub(super) fn record(&mut self, vault_id: &str, fingerprint: &str, outcome: DialOutcome) {
        let report = self
            .0
            .entry((vault_id.to_string(), fingerprint.to_string()))
            .or_default();
        match outcome {
            DialOutcome::Reached(endpoint) => {
                *report = CoordinatorDialReport {
                    reached: Some(endpoint),
                    ..CoordinatorDialReport::default()
                };
            }
            DialOutcome::Failed {
                local_network_denied,
                discovery_found_nothing,
            } => {
                report.local_network_denied = local_network_denied;
                report.discovery_found_nothing = discovery_found_nothing;
            }
        }
    }

    /// The report of the coordinator of `vault_id` with `fingerprint`, empty before any dial.
    pub(super) fn report(&self, vault_id: &str, fingerprint: &str) -> CoordinatorDialReport {
        self.0
            .get(&(vault_id.to_string(), fingerprint.to_string()))
            .cloned()
            .unwrap_or_default()
    }
}

/// Records how a dial of `coordinator` ended.
pub(super) fn record_dial(coordinator: &CoordinatorPin, outcome: DialOutcome) {
    match DIAL_REPORTS.lock() {
        Ok(mut reports) => reports.record(
            &coordinator.vault_id,
            &coordinator.certificate_fingerprint,
            outcome,
        ),
        Err(_) => eprintln!("record the coordinator dial: the dial report lock is unavailable"),
    }
}

/// What the dials of `coordinator` found in this run.
pub(crate) fn coordinator_dial_report(coordinator: &CoordinatorPin) -> CoordinatorDialReport {
    DIAL_REPORTS
        .lock()
        .map(|reports| reports.report(&coordinator.vault_id, &coordinator.certificate_fingerprint))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(text: &str) -> SocketAddr {
        text.parse().expect("endpoint")
    }

    #[test]
    fn a_failed_dial_keeps_the_endpoint_an_earlier_dial_reached_and_a_success_clears_problems() {
        let mut reports = DialReports::new();
        assert_eq!(
            reports.report("vault", "a"),
            CoordinatorDialReport::default()
        );
        reports.record(
            "vault",
            "a",
            DialOutcome::Reached(endpoint("192.168.1.20:43821")),
        );
        reports.record(
            "vault",
            "a",
            DialOutcome::Failed {
                local_network_denied: true,
                discovery_found_nothing: true,
            },
        );
        assert_eq!(
            reports.report("vault", "a"),
            CoordinatorDialReport {
                reached: Some(endpoint("192.168.1.20:43821")),
                local_network_denied: true,
                discovery_found_nothing: true,
            }
        );
        reports.record(
            "vault",
            "a",
            DialOutcome::Failed {
                local_network_denied: false,
                discovery_found_nothing: true,
            },
        );
        assert!(!reports.report("vault", "a").local_network_denied);
        reports.record(
            "vault",
            "a",
            DialOutcome::Reached(endpoint("100.64.0.7:43821")),
        );
        assert_eq!(
            reports.report("vault", "a"),
            CoordinatorDialReport {
                reached: Some(endpoint("100.64.0.7:43821")),
                ..CoordinatorDialReport::default()
            }
        );
    }

    #[test]
    fn dial_reports_belong_to_one_coordinator_of_one_vault() {
        let mut reports = DialReports::new();
        reports.record(
            "vault",
            "a",
            DialOutcome::Reached(endpoint("192.168.1.20:43821")),
        );
        assert_eq!(
            reports.report("vault", "b"),
            CoordinatorDialReport::default()
        );
        assert_eq!(
            reports.report("other", "a"),
            CoordinatorDialReport::default()
        );
    }
}
