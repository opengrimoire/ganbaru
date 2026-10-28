//! LAN peer exchange between the members of a vault, independent of the transport: which
//! devices a member listener admits, the device-local peer book of member endpoints, the plan
//! of which peers a pass exchanges with, and the pin of the current anchor.

pub mod admission;
pub mod anchor_pin;
pub mod book;
pub mod plan;
