//! Deterministic adaptive policy. Local time facts and canonical evidence are
//! explicit inputs; presentation processes do not choose accepted durations.

pub mod decision;
pub mod experiments;
pub mod features;
mod history;
pub mod models;
pub mod policy;
pub mod replay;
pub(crate) mod snapshots;
pub mod state;
pub mod statistics;

#[cfg(test)]
mod tests;
