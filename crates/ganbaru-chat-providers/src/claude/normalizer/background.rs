//! Claude background tasks that keep running after the turn that started them.

use super::text;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Most task IDs the roster keeps; a CLI that reports more is treated as busy until it reports
/// a shorter roster.
const MAX_TRACKED_TASKS: usize = 256;
/// Claude task type of a background shell command, which reports its start before any roster.
const BACKGROUND_COMMAND_TASK_TYPE: &str = "local_bash";

/// Background commands and agents the Claude CLI reports as still running.
///
/// The CLI's `background_tasks_changed` roster is authoritative and replaces what is known. A
/// background command's `task_started` adds it before the next roster arrives, and a
/// `task_notification` removes a task once it ends.
#[derive(Clone, Debug, Default)]
pub(super) struct ClaudeBackgroundTasks {
    running: BTreeSet<String>,
}

impl ClaudeBackgroundTasks {
    /// Whether any reported background task is still running.
    pub(super) fn any_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Applies one system message, returning whether it was a roster, which carries nothing
    /// else to show.
    pub(super) fn observe(&mut self, subtype: &str, object: &Map<String, Value>) -> bool {
        match subtype {
            "background_tasks_changed" => {
                if let Some(tasks) = object.get("tasks").and_then(Value::as_array) {
                    self.running = tasks
                        .iter()
                        .filter_map(|task| task.as_object().and_then(task_id))
                        .take(MAX_TRACKED_TASKS)
                        .map(str::to_string)
                        .collect();
                }
                true
            }
            "task_started" => {
                if text(object, "task_type") == Some(BACKGROUND_COMMAND_TASK_TYPE)
                    && self.running.len() < MAX_TRACKED_TASKS
                {
                    if let Some(id) = task_id(object) {
                        self.running.insert(id.to_string());
                    }
                }
                false
            }
            "task_notification" => {
                if let Some(id) = task_id(object) {
                    self.running.remove(id);
                }
                false
            }
            _ => false,
        }
    }
}

fn task_id(object: &Map<String, Value>) -> Option<&str> {
    text(object, "task_id").or_else(|| text(object, "taskId"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn observe(tasks: &mut ClaudeBackgroundTasks, message: Value) -> bool {
        let object = message.as_object().expect("message is an object");
        let subtype = text(object, "subtype").expect("message has a subtype");
        tasks.observe(subtype, object)
    }

    #[test]
    fn a_background_command_runs_until_its_notification() {
        let mut tasks = ClaudeBackgroundTasks::default();
        observe(
            &mut tasks,
            json!({"subtype": "task_started", "task_id": "b1", "task_type": "local_bash"}),
        );
        assert!(tasks.any_running());
        observe(
            &mut tasks,
            json!({"subtype": "task_notification", "task_id": "b1", "status": "completed"}),
        );
        assert!(!tasks.any_running());
    }

    #[test]
    fn a_foreground_subagent_start_is_not_background_work() {
        let mut tasks = ClaudeBackgroundTasks::default();
        observe(
            &mut tasks,
            json!({"subtype": "task_started", "task_id": "a1", "task_type": "local_agent"}),
        );
        assert!(!tasks.any_running());
    }

    #[test]
    fn the_roster_replaces_known_tasks_and_an_empty_roster_clears_them() {
        let mut tasks = ClaudeBackgroundTasks::default();
        observe(
            &mut tasks,
            json!({"subtype": "task_started", "task_id": "b1", "task_type": "local_bash"}),
        );
        let roster = json!({
            "subtype": "background_tasks_changed",
            "tasks": [{"task_id": "a2", "task_type": "local_agent"}]
        });
        assert!(observe(&mut tasks, roster));
        assert_eq!(tasks.running, BTreeSet::from(["a2".to_string()]));
        observe(
            &mut tasks,
            json!({"subtype": "background_tasks_changed", "tasks": []}),
        );
        assert!(!tasks.any_running());
    }

    #[test]
    fn a_malformed_roster_keeps_what_is_known() {
        let mut tasks = ClaudeBackgroundTasks::default();
        observe(
            &mut tasks,
            json!({"subtype": "task_started", "task_id": "b1", "task_type": "local_bash"}),
        );
        observe(
            &mut tasks,
            json!({"subtype": "background_tasks_changed", "tasks": "broken"}),
        );
        assert!(tasks.any_running());
    }
}
