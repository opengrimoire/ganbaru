//! Projects domain independent of Tauri and the WebView: groups, projects, sections, statuses,
//! priorities, tags, tasks, checklists, custom fields, relationships, task history, task date reminders, reviewed
//! dependency cascades, reordering, project views, workspace reads, and Calendar scheduling rows.
//!
//! Callers supply an authorized vault pool. Project creation also creates the managed working
//! folder under the supplied vault root.

pub mod custom_fields;
mod defaults;
pub mod dependency_cascade;
mod dependency_cascade_graph;
mod dependency_cascade_plan;
pub mod emojis;
mod history;
mod models;
mod mutations;
pub mod preferences;
pub mod project_commands;
pub mod relationship_commands;
pub mod reorder;
mod routine;
pub mod scheduling;
pub mod structure_commands;
pub mod task_bulk;
pub mod task_commands;
pub mod task_reminders;
pub mod task_summaries;
mod task_views;
pub mod validation;
pub mod views;
pub mod workspace;

pub use models::*;
#[cfg(test)]
use routine::BUILT_IN_ROUTINE_PROJECTS;
#[cfg(test)]
use routine::{ROUTINE_GROUP_ID, ensure_built_in_routine_defaults};
#[cfg(test)]
use task_views::{load_task_detail, load_task_view};
#[cfg(test)]
use validation::{
    MAX_TASK_CHANGE_REASON_LENGTH, normalize_optional_date_filter, sql_like_contains_pattern,
    validate_project_update, validate_task_update,
};
#[cfg(test)]
use workspace::load_project_custom_emojis;

#[cfg(test)]
mod tests;
