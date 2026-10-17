//! Chat command adapters. Desktop hosts native provider execution and workspace integrations;
//! mobile exposes provider-free channels, coordination, and preferences.

pub mod channels;
#[cfg(desktop)]
pub mod checkpoint_commands;
#[cfg(desktop)]
pub mod checkpoints;
#[cfg(desktop)]
pub(crate) mod command_support;
#[cfg(desktop)]
pub mod credentials;
pub mod device_state;
#[cfg(desktop)]
pub mod diagnostics_commands;
#[cfg(desktop)]
pub mod drafts;
#[cfg(desktop)]
pub mod execution_environment;
#[cfg(desktop)]
pub mod git;
#[cfg(desktop)]
pub mod ingestion;
#[cfg(desktop)]
pub(crate) mod interaction;
#[cfg(desktop)]
pub mod interaction_commands;
#[cfg(desktop)]
pub mod internal_mcp;
#[cfg(desktop)]
mod internal_mcp_tools;
pub mod organization;
#[cfg(desktop)]
pub mod preview;
#[cfg(desktop)]
pub mod provider_files;
#[cfg(desktop)]
pub mod restore_commands;
#[cfg(desktop)]
pub mod review;
#[cfg(desktop)]
pub(crate) mod revocation;
#[cfg(desktop)]
pub(crate) mod scratch;
#[cfg(desktop)]
pub(crate) mod send;
#[cfg(desktop)]
pub mod send_commands;
#[cfg(desktop)]
pub(crate) mod settings;
#[cfg(desktop)]
pub mod settings_commands;
#[cfg(desktop)]
pub mod source_control;
#[cfg(desktop)]
pub mod terminal;
#[cfg(desktop)]
pub mod terminal_commands;
#[cfg(desktop)]
pub mod threads;
#[cfg(desktop)]
pub(crate) mod turns;
#[cfg(desktop)]
pub mod workspace;
// Mobile registers provider-free settings under the same command path as desktop.
#[cfg(mobile)]
pub(crate) mod settings_commands_mobile;
#[cfg(mobile)]
pub(crate) use settings_commands_mobile as settings_commands;

#[cfg(all(test, desktop))]
mod tests;
