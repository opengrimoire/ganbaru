//! Deterministic Music session models, queue and observation policy, bounded queue loading, and
//! device-scoped checkpoint persistence. Callers own the session runtime, playback backends,
//! subscriptions, and device-local root bindings.

pub mod models;
pub mod persistence;
pub mod policy;
pub mod queue;

#[cfg(test)]
mod tests;
