#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(desktop)]
mod app_windows;
#[cfg(desktop)]
mod benchmark;
mod calendar;
mod chat;
mod contacts;
mod db;
mod distractions;
#[cfg(desktop)]
mod file_drops;
#[cfg(all(test, desktop))]
mod first_use_contracts;
mod music;
#[cfg(desktop)]
mod navigation_guard;
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
