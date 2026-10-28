//! The vault manifest: the registered domains, flattened once.

use std::sync::LazyLock;

use super::{DomainManifest, Manifest};

pub mod calendar;
pub mod chat;
pub mod contacts;
pub mod distractions;
pub mod focus;
pub mod music;
pub mod notes;
pub mod preferences;
pub mod projects;
pub mod quick_notes;
pub mod themes;

/// Current vault manifest version.
pub const VAULT_MANIFEST_VERSION: u16 = 20;

/// Registered vault domains in dependency order, one line per domain.
pub const VAULT_DOMAINS: &[&DomainManifest] = &[
    &quick_notes::DOMAIN,
    &preferences::DOMAIN,
    &themes::DOMAIN,
    &contacts::DOMAIN,
    &music::DOMAIN,
    &projects::DOMAIN,
    &calendar::DOMAIN,
    &focus::DOMAIN,
    &distractions::DOMAIN,
    &notes::DOMAIN,
    &chat::DOMAIN,
];

/// The vault manifest.
pub static VAULT_MANIFEST: LazyLock<Manifest> =
    LazyLock::new(|| Manifest::from_domains(VAULT_MANIFEST_VERSION, VAULT_DOMAINS));
