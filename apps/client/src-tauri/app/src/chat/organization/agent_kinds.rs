//! What each AI agent stands for: a whole provider family, a built-in model line, or an agent a
//! person named. The kind is read from how the agent's id was made, so it needs no stored
//! column and every device reads it alike.

use ganbaru_chat::conversion::{model_agent_id, provider_agent_id};
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
        .find(|line| model_agent_id(family_id, line).is_ok_and(|id| id == agent_id))
}

/// The kind of the agent `agent_id` of the provider family `family_id`.
pub(super) fn agent_kind(family_id: &str, agent_id: &str) -> ChatAgentKind {
    if provider_agent_id(family_id).is_ok_and(|id| id == agent_id) {
        ChatAgentKind::Provider
    } else if model_agent_line(family_id, agent_id).is_some() {
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
        let opus = model_agent_id("claude", "opus").unwrap();
        assert_eq!(agent_kind("claude", &opus), ChatAgentKind::Model);
        assert_eq!(model_agent_line("claude", &opus), Some("opus"));
        assert_eq!(model_agent_line("codex", &opus), None);
        assert_eq!(
            agent_kind("claude", &provider_agent_id("claude").unwrap()),
            ChatAgentKind::Provider
        );
        assert_eq!(agent_kind("claude", "agent:custom"), ChatAgentKind::Custom);
        assert_eq!(family_model_lines("cursor"), &[] as &[&str]);
        assert_eq!(line_display_name("sol"), "Sol");
    }
}
