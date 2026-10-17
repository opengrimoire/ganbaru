//! LAN handoff contracts and device state independent of Tauri: the bounded control protocol
//! that linked devices and the desktop coordinator exchange, invitation and contact QR encoding,
//! staged bundle validation, the device-private pairing identity, membership, and transfer
//! progress, and on Linux the explicit firewall authorization for the coordinator port with its
//! privileged helper.

#[cfg(target_os = "linux")]
pub mod network_access;
pub mod pairing;
pub mod protocol;
