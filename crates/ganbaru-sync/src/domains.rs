//! Built-in domain adapters and materialization hooks.

use crate::adapter::Adapters;
use crate::hooks::Hooks;
use crate::manifest::vault::preferences::PREFERENCES_TABLE;

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

/// The adapters the vault manifest names, one line per domain.
pub fn vault_adapters() -> Adapters {
    Adapters::new()
        .with(Box::new(quick_notes::QuickNotesAdapter))
        .with(Box::new(preferences::PreferencesAdapter))
        .with(Box::new(themes::ThemesAdapter))
        .with(Box::new(contacts::ContactsAdapter))
        .with(Box::new(music::MusicAdapter))
        .with(Box::new(projects::ProjectsAdapter))
        .with(Box::new(calendar::CalendarAdapter))
        .with(Box::new(focus::FocusAdapter))
        .with(Box::new(distractions::DistractionsAdapter))
        .with(Box::new(notes::NotesAdapter))
        .with(Box::new(chat::ChatAdapter))
}

/// The hooks of the vault manifest's tables, one line per hooked table or domain.
pub fn vault_hooks() -> Hooks {
    focus::with_hooks(distractions::with_hooks(projects::with_hooks(
        Hooks::new().with(PREFERENCES_TABLE, Box::new(preferences::PreferencesHook)),
    )))
}
