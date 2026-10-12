//! Projects command adapters. Persistence, structure, tasks, history, dependency cascades,
//! reordering, scheduling, and validation live in `ganbaru-projects`; these commands authorize
//! the active vault before calling it. Icon assets and working-folder device state stay here.

pub(crate) mod custom_fields;
pub(crate) mod dependency_cascade;
pub(crate) mod emojis;
pub(crate) mod icons;
pub(crate) mod preferences;
pub(crate) mod project_commands;
pub(crate) mod relationship_commands;
pub(crate) mod reorder;
pub(crate) mod structure_commands;
pub(crate) mod task_bulk;
pub(crate) mod task_commands;
#[cfg(desktop)]
pub(crate) mod working_folders;
pub(crate) mod workspace;
