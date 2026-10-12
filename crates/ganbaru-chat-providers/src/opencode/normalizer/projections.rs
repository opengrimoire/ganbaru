use super::value::*;
use super::*;

impl OpenCodeEventNormalizer {
    pub(super) fn todo_updated(
        &self,
        state: &OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let todos = properties
            .get("todos")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("todo list"))?;
        if todos.len() > MAX_SAFE_COLLECTION_ITEMS {
            return Err(protocol_error("todo list size"));
        }
        let mut steps = Vec::new();
        for (index, todo) in todos.iter().enumerate() {
            let todo = todo.as_object().ok_or_else(|| protocol_error("todo"))?;
            let content = text(todo, "content")
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| protocol_error("todo content"))?;
            steps.push(PlanStep {
                id: text(todo, "id")
                    .map(|value| bounded_text(value, 256))
                    .unwrap_or_else(|| format!("opencode-todo-{index}")),
                text: bounded_text(content, 4096),
                status: activity_status(text(todo, "status")),
            });
        }
        let markdown = steps
            .iter()
            .map(|step| {
                format!(
                    "- [{}] {}",
                    if step.status == ActivityStatus::Completed {
                        "x"
                    } else {
                        " "
                    },
                    step.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(vec![self.event(
            state,
            "todo.updated",
            None,
            None,
            CanonicalEvent::PlanUpdated(PlanUpdatedEvent { markdown, steps }),
        )?])
    }

    pub(super) fn mcp_status(
        &self,
        state: &OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let server_id = text(properties, "name")
            .or_else(|| text(properties, "id"))
            .ok_or_else(|| protocol_error("MCP server ID"))?;
        validate_identifier(server_id, "MCP server ID")?;
        Ok(vec![self.event(
            state,
            "mcp.status",
            None,
            None,
            CanonicalEvent::McpStatus(McpStatusEvent {
                server_id: server_id.to_string(),
                status: activity_status(text(properties, "status")),
                detail: text(properties, "message").map(|value| bounded_text(value, 4096)),
            }),
        )?])
    }
}
