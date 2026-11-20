//! Live execution reads between linked devices. A desktop answers them for the work assignments
//! it executes, and a phone reads them for the work it follows while the reply thread is open.
//! A phone also answers the approvals that work holds open, which the executing desktop resolves.

#[cfg(any(test, mobile))]
pub mod answer;
#[cfg(desktop)]
mod approvals;
#[cfg(any(test, mobile))]
pub mod follow;
#[cfg(desktop)]
mod serve;

#[cfg(desktop)]
pub(crate) use approvals::RemoteApprovals;
#[cfg(desktop)]
pub(crate) use serve::{ChatLiveChanges, chat_live_routes, wake};
