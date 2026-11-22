//! What the coordinator dials of this app run found, so the sync status can name the network
//! it reached the coordinator over and why it could not. Kept in memory only.

use ganbaru_handoff::pairing::CoordinatorPin;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Mutex;

/// Coordinator dial outcomes of this run, one per coordinator dialed.
static DIAL_REPORTS: Mutex<DialReports> = Mutex::new(DialReports::new());

/// Why a dial reached no endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FailedDial {
    /// The system refused at least one connection with a permission error, as Android does
    /// when local network access is denied.
    pub local_network_denied: bool,
    /// A browse of the local network found no endpoint.
    pub discovery_found_nothing: bool,
}

/// What the dials of one coordinator found in this run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CoordinatorDialReport {
    /// No dial ended yet.
    #[default]
    NotDialed,
    /// The latest dial reached this endpoint.
    Reached(SocketAddr),
    /// The latest dial reached no endpoint. `last_reached` keeps the endpoint an earlier dial of
    /// this run reached, which the status still names next to its state.
    Failed {
        last_reached: Option<SocketAddr>,
        failure: FailedDial,
    },
}

impl CoordinatorDialReport {
    /// The report after a dial ended with `outcome`. A failure keeps the endpoint an earlier
    /// dial reached, and a success clears every problem.
    pub fn after(self, outcome: DialOutcome) -> Self {
        match outcome {
            DialOutcome::Reached(endpoint) => Self::Reached(endpoint),
            DialOutcome::Failed(failure) => Self::Failed {
                last_reached: self.last_reached(),
                failure,
            },
        }
    }

    /// The endpoint the latest successful dial of this run reached.
    pub fn last_reached(&self) -> Option<SocketAddr> {
        match *self {
            Self::NotDialed => None,
            Self::Reached(endpoint) => Some(endpoint),
            Self::Failed { last_reached, .. } => last_reached,
        }
    }

    /// Why the latest dial failed, or `None` when it reached an endpoint or none ended yet.
    pub fn failure(&self) -> Option<FailedDial> {
        match *self {
            Self::Failed { failure, .. } => Some(failure),
            Self::NotDialed | Self::Reached(_) => None,
        }
    }
}

/// How one dial ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialOutcome {
    Reached(SocketAddr),
    Failed(FailedDial),
}

/// The coordinator a report belongs to.
type DialTarget = (String, String);

/// Reports by coordinator vault and certificate fingerprint.
#[derive(Debug)]
pub struct DialReports(BTreeMap<DialTarget, CoordinatorDialReport>);

impl DialReports {
    const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Folds `outcome` into the report of the coordinator of `vault_id` with `fingerprint`.
    pub fn record(&mut self, vault_id: &str, fingerprint: &str, outcome: DialOutcome) {
        let report = self
            .0
            .entry((vault_id.to_string(), fingerprint.to_string()))
            .or_default();
        *report = report.after(outcome);
    }

    /// The report of the coordinator of `vault_id` with `fingerprint`, empty before any dial.
    pub fn report(&self, vault_id: &str, fingerprint: &str) -> CoordinatorDialReport {
        self.0
            .get(&(vault_id.to_string(), fingerprint.to_string()))
            .copied()
            .unwrap_or_default()
    }
}

/// Records how a dial of `coordinator` ended.
pub fn record_dial(coordinator: &CoordinatorPin, outcome: DialOutcome) {
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
pub fn coordinator_dial_report(coordinator: &CoordinatorPin) -> CoordinatorDialReport {
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

    const DENIED: FailedDial = FailedDial {
        local_network_denied: true,
        discovery_found_nothing: true,
    };
    const NOTHING_FOUND: FailedDial = FailedDial {
        local_network_denied: false,
        discovery_found_nothing: true,
    };

    #[test]
    fn a_failed_dial_keeps_the_endpoint_an_earlier_dial_reached_and_a_success_clears_problems() {
        let mut reports = DialReports::new();
        assert_eq!(
            reports.report("vault", "a"),
            CoordinatorDialReport::NotDialed
        );
        reports.record(
            "vault",
            "a",
            DialOutcome::Reached(endpoint("192.168.1.20:43821")),
        );
        reports.record("vault", "a", DialOutcome::Failed(DENIED));
        assert_eq!(
            reports.report("vault", "a"),
            CoordinatorDialReport::Failed {
                last_reached: Some(endpoint("192.168.1.20:43821")),
                failure: DENIED,
            }
        );
        reports.record("vault", "a", DialOutcome::Failed(NOTHING_FOUND));
        assert_eq!(reports.report("vault", "a").failure(), Some(NOTHING_FOUND));
        reports.record(
            "vault",
            "a",
            DialOutcome::Reached(endpoint("100.64.0.7:43821")),
        );
        assert_eq!(
            reports.report("vault", "a"),
            CoordinatorDialReport::Reached(endpoint("100.64.0.7:43821"))
        );
    }

    #[test]
    fn every_report_moves_to_the_outcome_of_the_latest_dial() {
        let lan = endpoint("192.168.1.20:43821");
        let overlay = endpoint("100.64.0.7:43821");
        let reports = [
            CoordinatorDialReport::NotDialed,
            CoordinatorDialReport::Reached(lan),
            CoordinatorDialReport::Failed {
                last_reached: None,
                failure: DENIED,
            },
            CoordinatorDialReport::Failed {
                last_reached: Some(lan),
                failure: NOTHING_FOUND,
            },
        ];
        for report in reports {
            assert_eq!(
                report.after(DialOutcome::Reached(overlay)),
                CoordinatorDialReport::Reached(overlay)
            );
            let failed = report.after(DialOutcome::Failed(DENIED));
            assert_eq!(failed.failure(), Some(DENIED));
            assert_eq!(failed.last_reached(), report.last_reached());
        }
        assert_eq!(CoordinatorDialReport::NotDialed.failure(), None);
        assert_eq!(CoordinatorDialReport::Reached(lan).failure(), None);
        assert_eq!(CoordinatorDialReport::NotDialed.last_reached(), None);
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
