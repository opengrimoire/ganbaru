//! LAN handoff contracts and device state independent of Tauri: the bounded control protocol
//! that linked devices and the desktop coordinator exchange, invitation and contact QR encoding,
//! staged bundle validation, the device-private pairing identity, membership, and transfer
//! progress, the LAN-only rule for outgoing connection targets, the DNS-SD discovery records
//! with their hourly member tags, and on Linux the explicit firewall authorization for the
//! coordinator port and LAN discovery with its privileged helper.

pub mod dial;
pub mod discovery;
#[cfg(target_os = "linux")]
pub mod network_access;
pub mod pairing;
pub mod protocol;
