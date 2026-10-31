//! LAN handoff contracts and device state independent of Tauri: the bounded control protocol
//! that linked devices and the desktop coordinator exchange, invitation and contact QR encoding,
//! staged bundle validation, the device-bound protection of private key files, the
//! device-private pairing identity, membership, and transfer progress, the LAN-only rule for
//! outgoing connection targets, TLS certificate pins for both connection sides, the DNS-SD
//! discovery records with their hourly member tags, and on Linux the explicit firewall
//! authorization for the coordinator port and LAN discovery with its privileged helper.

pub mod dial;
pub mod discovery;
pub mod key_wrap;
#[cfg(target_os = "linux")]
pub mod network_access;
pub mod pairing;
pub mod protocol;
pub mod tls;
