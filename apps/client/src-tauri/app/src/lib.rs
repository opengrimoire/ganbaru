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
mod native_dialogs;
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

/// Makes ring the process TLS provider. `install_default` fails only when a provider is already
/// set, and that one is ring as well: it is the only provider the workspace enables, and rustls
/// config builders set it on first use. Android reaches this after that point, because a
/// background sync pass can run before the first activity and each new activity starts the app
/// again in the same process.
fn install_default_tls_crypto_provider() {
    drop(rustls::crypto::ring::default_provider().install_default());
}

mod runtime;

pub use runtime::run;

#[cfg(test)]
mod tests {
    #[test]
    fn tls_provider_install_tolerates_an_installed_provider() {
        super::install_default_tls_crypto_provider();
        super::install_default_tls_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
