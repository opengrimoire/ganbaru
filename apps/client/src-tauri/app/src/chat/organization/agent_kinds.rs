//! What each AI agent stands for: a built-in model line, one catalog model, or an agent a person
//! named. The kind is read from how the agent's id was made, so it needs no stored column and
//! every device reads it alike.

use ganbaru_chat::conversion::{catalog_agent_id, line_agent_id};
use ganbaru_chat_contracts::models::ChatAgentKind;
use ganbaru_chat_providers::model_recommendation::{family_native_company, model_lines};

/// The model lines the provider family `family_id` offers as built-in agents.
pub(super) fn family_model_lines(family_id: &str) -> &'static [&'static str] {
    model_lines(family_native_company(family_id))
}

/// The model line `agent_id` runs when it is the built-in agent of a line of `family_id`.
pub(super) fn model_agent_line(family_id: &str, agent_id: &str) -> Option<&'static str> {
    family_model_lines(family_id)
        .iter()
        .copied()
        .find(|line| line_agent_id(family_id, line).is_ok_and(|id| id == agent_id))
}

/// The kind of the agent `agent_id` of the provider family `family_id`, whose policy names
/// `model_id`. A catalog agent's id is derived from the model it runs, so an agent whose id
/// matches neither a line nor its own model was named by a person.
pub(super) fn agent_kind(family_id: &str, agent_id: &str, model_id: Option<&str>) -> ChatAgentKind {
    if model_agent_line(family_id, agent_id).is_some() {
        ChatAgentKind::Line
    } else if model_id.is_some_and(|model_id| {
        catalog_agent_id(family_id, model_id).is_ok_and(|id| id == agent_id)
    }) {
        ChatAgentKind::Model
    } else {
        ChatAgentKind::Custom
    }
}

/// The name a built-in model line agent shows, such as `Opus` for `opus`.
#[cfg(desktop)]
pub(super) fn line_display_name(line: &str) -> String {
    let mut characters = line.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_follows_how_the_id_was_made() {
        let opus = line_agent_id("claude", "opus").unwrap();
        assert_eq!(
            agent_kind("claude", &opus, Some("opus")),
            ChatAgentKind::Line
        );
        assert_eq!(model_agent_line("claude", &opus), Some("opus"));
        assert_eq!(model_agent_line("codex", &opus), None);
        let pinned = catalog_agent_id("claude", "claude-opus-4-8").unwrap();
        assert_eq!(
            agent_kind("claude", &pinned, Some("claude-opus-4-8")),
            ChatAgentKind::Model
        );
        assert_eq!(
            agent_kind("claude", &pinned, Some("claude-opus-4-7")),
            ChatAgentKind::Custom
        );
        assert_eq!(
            agent_kind("claude", "agent:custom", Some("opus")),
            ChatAgentKind::Custom
        );
        assert_eq!(family_model_lines("cursor"), &[] as &[&str]);
        assert_eq!(line_display_name("sol"), "Sol");
    }
}
