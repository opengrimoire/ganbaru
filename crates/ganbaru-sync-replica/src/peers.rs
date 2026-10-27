//! LAN peer exchange between the members of a vault, independent of the transport: which
//! devices a member listener admits, the device-local peer book of member endpoints, and the
//! plan of which peers a pass exchanges with.

pub mod admission;
pub mod book;
pub mod plan;
