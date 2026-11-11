//! Bidirectional provider replay: recorded or authored transcripts of every frame between a
//! driver and its CLI, played back against the real driver.
//!
//! A transcript is JSON Lines under `src/<provider>/fixtures/scenarios/<scenario>.jsonl`. The
//! runner installs a duplex pair through the provider's test connection factory, checks each
//! frame the driver writes against the next `expect_outbound` entry, and writes each
//! `emit_inbound` frame back on the paused tokio clock. A script of driver calls runs at the
//! same time, and the calls and canonical events it produced are compared with the
//! `<scenario>.events.json` snapshot.

pub(crate) mod transcript;

mod diff;
pub(crate) mod runner;
pub(crate) mod scripts;

mod record;

mod tests;
