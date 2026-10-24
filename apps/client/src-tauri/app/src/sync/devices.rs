//! Device names for versions and recovery offers, from the linked devices this device knows.

use crate::vault::handoff::pairing::PairingManager;
use ganbaru_sync::DeviceNames;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, Runtime};

/// Reads the own device id and the labels of linked peers and the pinned coordinator. A device
/// without pairing state knows no labels.
pub(crate) fn read<R: Runtime>(app: &AppHandle<R>) -> Result<DeviceNames, String> {
    let own = stable_device_id(app)?;
    Ok(read_with(own, &app.state::<PairingManager>()))
}

/// The stable identifier of this installation, which keys device-scoped records such as usage
/// totals.
///
/// It is the device id in the app config directory's `app-state.json`, so it keeps the same value
/// across restarts and app updates for one installation and changes only when that state is
/// removed. It is never the sync writer id, which rotates with writer keys, and it is never
/// derived from keys or other secrets.
pub(crate) fn stable_device_id<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    crate::vault::ensure_device_id(app)
}

/// The names of device `own` and of the linked peers and pinned coordinator `pairing` knows.
pub(crate) fn read_with(own: String, pairing: &PairingManager) -> DeviceNames {
    let mut labels: HashMap<String, String> = pairing
        .linked_peers()
        .unwrap_or_default()
        .into_iter()
        .map(|peer| (peer.device_id, peer.device_label))
        .collect();
    if let Ok(Some(pin)) = pairing.coordinator_pin() {
        if let Some(label) = pin.device_label {
            labels.insert(pin.device_id, label);
        }
    }
    DeviceNames::new(own, labels)
}
