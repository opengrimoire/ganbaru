//! Derived row keys for Projects rows identified by other rows.
//!
//! Every device derives the same key from the same identity, so concurrent writes of one link, value, or preference
//! address one row.

use ganbaru_sync_contracts::derived_key;

/// Prefix of priority keys, over the project and priority id.
pub const PRIORITY_KEY_PREFIX: &str = "project-priority";
/// Prefix of dependency keys, over the blocking and blocked task.
pub const DEPENDENCY_KEY_PREFIX: &str = "dependency";
/// Prefix of task tag link keys, over the task and tag.
pub const TASK_TAG_KEY_PREFIX: &str = "task-tag";
/// Prefix of custom field value keys, over the task and field.
pub const CUSTOM_FIELD_VALUE_KEY_PREFIX: &str = "custom-field-value";
/// Prefix of view preference keys, over the view and preference key.
pub const VIEW_PREFERENCE_KEY_PREFIX: &str = "view-preference";

fn key(prefix: &str, parts: &[&str]) -> Result<String, String> {
    derived_key(prefix, parts)
        .map(|key| key.as_str().to_string())
        .map_err(|error| format!("derive {prefix} key: {error}"))
}

/// Key of a project priority.
pub fn priority_key(project_id: &str, priority_id: &str) -> Result<String, String> {
    key(PRIORITY_KEY_PREFIX, &[project_id, priority_id])
}

/// Key and id of a dependency between two tasks.
pub fn dependency_key(blocking_task_id: &str, blocked_task_id: &str) -> Result<String, String> {
    key(DEPENDENCY_KEY_PREFIX, &[blocking_task_id, blocked_task_id])
}

/// Key of a task tag link.
pub fn task_tag_key(task_id: &str, tag_id: &str) -> Result<String, String> {
    key(TASK_TAG_KEY_PREFIX, &[task_id, tag_id])
}

/// Key of a custom field value.
pub fn custom_field_value_key(task_id: &str, field_id: &str) -> Result<String, String> {
    key(CUSTOM_FIELD_VALUE_KEY_PREFIX, &[task_id, field_id])
}

/// Key of a view preference.
pub fn view_preference_key(view_id: &str, preference_key: &str) -> Result<String, String> {
    key(VIEW_PREFERENCE_KEY_PREFIX, &[view_id, preference_key])
}
