//! Live execution reads between linked devices. A desktop answers them for the work assignments
//! it executes, and a phone reads them for the work it follows while the reply thread is open.

#[cfg(desktop)]
mod serve;

#[cfg(desktop)]
pub(crate) use serve::{ChatLiveChanges, chat_live_routes, wake};
