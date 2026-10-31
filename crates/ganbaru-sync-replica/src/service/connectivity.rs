//! What the latest coordinator dial says about connectivity on this device.

use super::dial_report::{CoordinatorDialReport, coordinator_dial_report};
use super::status::{DegradedReasons, SyncConnectionView, SyncDegradedReason, SyncRole};
use ganbaru_handoff::pairing::PairingManager;

/// What the latest dial of this device's coordinator found, or `None` on a hub or before the
/// device is linked.
pub fn dial_report(role: SyncRole, pairing: &PairingManager) -> Option<CoordinatorDialReport> {
    if role != SyncRole::Client {
        return None;
    }
    match pairing.coordinator_pin() {
        Ok(pin) => pin.as_ref().map(coordinator_dial_report),
        Err(error) => {
            eprintln!("read the coordinator for the sync status: {error}");
            None
        }
    }
}

/// The connection the latest successful dial reached.
pub fn connection(dial: Option<&CoordinatorDialReport>) -> Option<SyncConnectionView> {
    dial?.reached.and_then(SyncConnectionView::reached)
}

/// Degraded reasons the latest dial shows on its own.
pub fn dial_reasons(dial: Option<&CoordinatorDialReport>, reasons: &mut DegradedReasons) {
    reasons.note(
        SyncDegradedReason::LocalNetworkDenied,
        dial.is_some_and(|report| report.local_network_denied),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::status::SyncConnectionNetwork;

    fn report(reached: Option<&str>, denied: bool) -> CoordinatorDialReport {
        CoordinatorDialReport {
            reached: reached.map(|endpoint| endpoint.parse().expect("endpoint")),
            local_network_denied: denied,
            discovery_found_nothing: false,
        }
    }

    #[test]
    fn a_connection_names_the_network_class_of_the_endpoint_reached() {
        let lan = report(Some("192.168.1.20:43821"), false);
        assert_eq!(
            connection(Some(&lan)),
            Some(SyncConnectionView {
                network: SyncConnectionNetwork::Lan,
                address: "192.168.1.20:43821".to_string(),
            })
        );
        let overlay = report(Some("100.64.0.7:43821"), false);
        assert_eq!(
            connection(Some(&overlay)).map(|view| view.network),
            Some(SyncConnectionNetwork::Overlay)
        );
        let ula = report(Some("[fd00::5]:43821"), false);
        assert_eq!(
            connection(Some(&ula)).map(|view| view.address),
            Some("[fd00::5]:43821".to_string())
        );
        assert_eq!(connection(Some(&report(None, false))), None);
        assert_eq!(connection(None), None);
        assert_eq!(
            connection(Some(&report(Some("8.8.8.8:43821"), false))),
            None
        );
    }

    #[test]
    fn a_dial_refused_by_a_permission_error_reports_local_network_denial() {
        let mut reasons = DegradedReasons::default();
        dial_reasons(Some(&report(None, false)), &mut reasons);
        dial_reasons(None, &mut reasons);
        assert!(reasons.clone().into_vec().is_empty());
        dial_reasons(Some(&report(None, true)), &mut reasons);
        assert_eq!(
            reasons.into_vec(),
            vec![SyncDegradedReason::LocalNetworkDenied]
        );
    }
}
