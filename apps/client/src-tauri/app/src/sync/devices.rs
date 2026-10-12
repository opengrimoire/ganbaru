//! Device names for versions and recovery offers, from the linked devices this device knows.

use crate::vault::handoff::pairing::PairingManager;
use ganbaru_sync::DeviceNames;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, Runtime};

/// Reads the own device id and the labels of linked peers and the pinned coordinator. A device
/// without pairing state knows no labels.
pub(crate) fn read<R: Runtime>(app: &AppHandle<R>) -> Result<DeviceNames, String> {
    let own = crate::vault::ensure_device_id(app)?;
    let manager = app.state::<PairingManager>();
    let mut labels: HashMap<String, String> = manager
        .linked_peers()
        .unwrap_or_default()
        .into_iter()
        .map(|peer| (peer.device_id, peer.device_label))
        .collect();
    if let Ok(Some(pin)) = manager.coordinator_pin() {
        if let Some(label) = pin.device_label {
            labels.insert(pin.device_id, label);
        }
    }
    Ok(DeviceNames::new(own, labels))
}
