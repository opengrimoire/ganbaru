//! Bounds and keys of Chat channel notes.
//!
//! Sync admission, the Chat commands, and the agent proposal tool all read these, so a note
//! one replica accepts is a note every replica can apply.

/// Characters of a channel note body after trimming. A note holds a fact, not a document.
pub const MAX_CHANNEL_NOTE_CHARS: usize = 1000;
/// Prefix of the note id derived from an accepted or dismissed proposal, over the proposal
/// item, so devices that settle one proposal concurrently write one row.
pub const CHANNEL_NOTE_PROPOSAL_KEY_PREFIX: &str = "channel-note-proposal";
