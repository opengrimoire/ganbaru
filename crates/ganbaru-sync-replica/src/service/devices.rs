//! Device names for versions and recovery offers, from the linked devices this device knows.

use ganbaru_handoff::pairing::PairingManager;
use ganbaru_sync::DeviceNames;
use std::collections::HashMap;

/// The names of device `own` and of the linked peers and pinned coordinator `pairing` knows.
pub fn read_with(own: String, pairing: &PairingManager) -> DeviceNames {
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
