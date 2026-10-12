//! Tauri authorization adapter for core Chat checkpoints.

mod cleanup;

pub(crate) use cleanup::run_checkpoint_cleanup;
