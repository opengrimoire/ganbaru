//! Keys of Chat standing push approvals.
//!
//! Sync admission and the Chat commands both read these, so an approval two devices make for
//! one agent, folder, push URL, and destination branch is one row.

/// Prefix of push approval keys, over the agent, working folder, push URL, and destination
/// branch.
pub const PUSH_APPROVAL_KEY_PREFIX: &str = "push-approval";
/// Longest push URL an approval names, in bytes, as long as a push request may record.
pub const MAX_PUSH_APPROVAL_URL_BYTES: usize = 2_048;
/// Longest destination branch an approval names, in bytes, as long as Git branch names Chat
/// accepts.
pub const MAX_PUSH_APPROVAL_BRANCH_BYTES: usize = 1_024;
