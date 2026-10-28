//! Why connectivity is degraded on this device: what the latest coordinator dial found, and the
//! platform probes of the Tauri app, which are the Linux firewall state and the Android
//! background restrictions.

use super::status::{DegradedReasons, SyncConnectionView, SyncDegradedReason, SyncRole};
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::transport::{CoordinatorDialReport, coordinator_dial_report};

/// What the latest dial of this device's coordinator found, or `None` on a hub or before the
/// device is linked.
pub(crate) fn dial_report(
    role: SyncRole,
    pairing: &PairingManager,
) -> Option<CoordinatorDialReport> {
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
pub(crate) fn connection(dial: Option<&CoordinatorDialReport>) -> Option<SyncConnectionView> {
    dial?.reached.and_then(SyncConnectionView::reached)
}

/// Degraded reasons the latest dial shows on its own.
pub(crate) fn dial_reasons(dial: Option<&CoordinatorDialReport>, reasons: &mut DegradedReasons) {
    reasons.note(
        SyncDegradedReason::LocalNetworkDenied,
        dial.is_some_and(|report| report.local_network_denied),
    );
}

/// Linux firewall probes, cached because reading firewalld state starts processes.
#[cfg(target_os = "linux")]
pub(crate) mod firewall {
    use super::{CoordinatorDialReport, DegradedReasons, SyncDegradedReason, SyncRole};
    use ganbaru_handoff::network_access::{self, NetworkAccessState};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Manager, Runtime};

    /// How long a firewall probe stays current. Granting or revoking access clears it.
    const PROBE_TTL: Duration = Duration::from_secs(60);

    /// The latest probe of the coordinator listener's firewall state.
    static LISTENER: Mutex<Option<Probe<NetworkAccessState>>> = Mutex::new(None);
    /// The latest probe of the member listener's firewall state.
    static MEMBER_LISTENER: Mutex<Option<Probe<NetworkAccessState>>> = Mutex::new(None);
    /// The latest probe of whether a firewall is active.
    static ACTIVE: Mutex<Option<Probe<bool>>> = Mutex::new(None);

    #[derive(Clone, Copy)]
    struct Probe<T> {
        at: Instant,
        value: T,
    }

    /// The cached value in `slot` while it is current, otherwise a fresh one from `read`.
    fn cached<T: Copy>(slot: &Mutex<Option<Probe<T>>>, read: impl FnOnce() -> T) -> T {
        let now = Instant::now();
        if let Ok(slot) = slot.lock()
            && let Some(probe) = *slot
            && now.duration_since(probe.at) < PROBE_TTL
        {
            return probe.value;
        }
        let value = read();
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(Probe { at: now, value });
        }
        value
    }

    /// Forgets the cached probes after the firewall access changed and runs a pass, so the
    /// status shows the new state.
    pub(crate) fn changed<R: Runtime>(app: &AppHandle<R>) {
        let listener = LISTENER.lock().map(|mut slot| *slot = None);
        let member = MEMBER_LISTENER.lock().map(|mut slot| *slot = None);
        let active = ACTIVE.lock().map(|mut slot| *slot = None);
        if listener.is_err() || member.is_err() || active.is_err() {
            eprintln!("forget the firewall probes: a probe lock is unavailable");
        }
        if let Some(runtime) = app.try_state::<crate::sync::SyncRuntime>() {
            runtime.sync_now();
        }
    }

    /// Whether the firewall keeps peers from reaching the coordinator listener until the
    /// person acts in Settings.
    pub(crate) fn listener_blocked(state: NetworkAccessState) -> bool {
        matches!(
            state,
            NetworkAccessState::AuthorizationRequired
                | NetworkAccessState::NetworkConfirmationRequired
                | NetworkAccessState::ManualActionRequired
        )
    }

    /// Adds the firewall reason: on the hub when its listener is not allowed through the
    /// firewall, and on a client when its latest dial failed after a browse that found nothing
    /// while a firewall is active.
    pub(crate) async fn note<R: Runtime>(
        app: &AppHandle<R>,
        role: SyncRole,
        dial: Option<&CoordinatorDialReport>,
        reasons: &mut DegradedReasons,
    ) {
        let blocked = match role {
            SyncRole::Hub => {
                let app = app.clone();
                let state = tauri::async_runtime::spawn_blocking(move || {
                    cached(&LISTENER, || {
                        crate::vault::handoff::desktop_network_access_status(&app)
                            .map(|status| status.state())
                            .unwrap_or_else(|error| {
                                eprintln!("read the coordinator firewall state: {error}");
                                NetworkAccessState::NotRequired
                            })
                    })
                })
                .await;
                state.is_ok_and(listener_blocked)
            }
            SyncRole::Client => {
                note_member_listener(app, reasons).await;
                if !dial.is_some_and(|report| report.discovery_found_nothing) {
                    return;
                }
                tauri::async_runtime::spawn_blocking(|| {
                    cached(&ACTIVE, network_access::has_active_firewall)
                })
                .await
                .unwrap_or(false)
            }
        };
        reasons.note(SyncDegradedReason::FirewallAuthorizationRequired, blocked);
    }

    /// Adds the member listener reason while the firewall keeps other members from reaching
    /// this member desktop's listener.
    async fn note_member_listener<R: Runtime>(app: &AppHandle<R>, reasons: &mut DegradedReasons) {
        let app = app.clone();
        let state = tauri::async_runtime::spawn_blocking(move || {
            cached(&MEMBER_LISTENER, || {
                crate::vault::handoff::member_network_access_state(&app)
                    .unwrap_or_else(|error| {
                        eprintln!("read the member listener firewall state: {error}");
                        None
                    })
                    .unwrap_or(NetworkAccessState::NotRequired)
            })
        })
        .await;
        reasons.note(
            SyncDegradedReason::FirewallPeerListenerBlocked,
            state.is_ok_and(listener_blocked),
        );
    }
}

/// Adds the Android background restrictions the plugin reports.
#[cfg(target_os = "android")]
pub(crate) async fn note_android<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    reasons: &mut DegradedReasons,
) {
    use ganbaru_mobile_sync::MobileSyncExt;
    let app = app.clone();
    let probed = tauri::async_runtime::spawn_blocking(move || {
        app.mobile_sync()
            .ok_or_else(|| "the background sync plugin is not registered".to_string())?
            .connectivity_status()
    })
    .await
    .map_err(|error| format!("read background sync connectivity status: {error}"))
    .and_then(|status| status);
    match probed {
        Ok(status) => {
            reasons.note(
                SyncDegradedReason::BackgroundRestricted,
                status.background_restricted,
            );
            reasons.note(
                SyncDegradedReason::StandbyRestricted,
                status.standby_restricted,
            );
        }
        Err(error) => eprintln!("{error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::status::SyncConnectionNetwork;

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

    #[cfg(target_os = "linux")]
    #[test]
    fn the_listener_counts_as_blocked_only_while_the_firewall_waits_on_the_person() {
        use ganbaru_handoff::network_access::NetworkAccessState;
        assert!(firewall::listener_blocked(
            NetworkAccessState::AuthorizationRequired
        ));
        assert!(firewall::listener_blocked(
            NetworkAccessState::NetworkConfirmationRequired
        ));
        assert!(firewall::listener_blocked(
            NetworkAccessState::ManualActionRequired
        ));
        assert!(!firewall::listener_blocked(NetworkAccessState::Granted));
        assert!(!firewall::listener_blocked(NetworkAccessState::NotRequired));
    }
}
