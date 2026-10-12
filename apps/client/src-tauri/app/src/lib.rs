#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(desktop)]
mod benchmark;
mod calendar;
mod chat;
mod contacts;
mod db;
mod distractions;
#[cfg(all(test, desktop))]
mod first_use_contracts;
mod music;
mod notes;
mod notifications;
mod pomodoro;
mod profile_images;
mod projects;
mod quick_notes;
#[cfg(desktop)]
mod sound_effects;
mod sync;
#[cfg(desktop)]
mod system_command;
mod themes;
#[cfg(desktop)]
mod tray;
#[cfg(desktop)]
mod updates;
mod vault;

fn install_default_tls_crypto_provider() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("the default TLS crypto provider must be installed only once during startup");
}

mod runtime;

pub use runtime::run;

#[cfg(target_os = "linux")]
pub fn run_privileged_helper_if_requested() -> Option<Result<(), String>> {
    vault::handoff::run_privileged_helper_if_requested()
}
