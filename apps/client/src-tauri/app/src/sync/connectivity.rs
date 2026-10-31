//! Why connectivity is degraded on this device beyond what the latest coordinator dial found:
//! the platform probes of the Tauri app, which are the Linux firewall state and the Android
//! background restrictions.

#[cfg(target_os = "linux")]
use super::status::SyncRole;
#[cfg(any(target_os = "linux", target_os = "android"))]
use super::status::{DegradedReasons, SyncDegradedReason};
#[cfg(target_os = "linux")]
use crate::vault::handoff::transport::CoordinatorDialReport;

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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

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
