//! Device names for conflict versions and recovery offers.

use std::collections::HashMap;

use serde::Serialize;

/// A device that wrote a version, as the frontend shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRef {
    pub device_id: String,
    /// Label from pairing, when this device knows the writer's device.
    pub device_label: Option<String>,
    /// Whether the version was written on this device.
    pub own_device: bool,
}

/// Labels of the devices this device is linked with, and its own id.
#[derive(Clone, Debug)]
pub struct DeviceNames {
    own: String,
    labels: HashMap<String, String>,
}

impl DeviceNames {
    /// Names from the own device id and the labels of known devices, keyed by device id.
    pub fn new(own: impl Into<String>, labels: HashMap<String, String>) -> Self {
        Self {
            own: own.into(),
            labels,
        }
    }

    /// The device reference for a device id.
    pub fn describe(&self, device_id: &str) -> DeviceRef {
        DeviceRef {
            device_id: device_id.to_string(),
            device_label: self.labels.get(device_id).cloned(),
            own_device: device_id == self.own,
        }
    }
}
