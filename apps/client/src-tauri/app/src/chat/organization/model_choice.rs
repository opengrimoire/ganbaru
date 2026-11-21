//! The `/model` command a message that starts work may carry, and the model and instructions
//! new work freezes when it is created.

use super::persistence_error;
use super::workflow::parse_model_selection;
use ganbaru_chat::coordination::agent_settings::read_channel_instructions;
use ganbaru_chat_contracts::models::*;
use sqlx::SqliteConnection;

/// The word that starts a model command.
const MODEL_COMMAND: &str = "/model";

/// A `/model` command found in message text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelCommand<'a> {
    /// The model the command names.
    model: &'a str,
    /// Byte range of the command and its argument in the text.
    start: usize,
    end: usize,
}

/// Finds the first `/model <model>` command in `text`: the word `/model` at the start of the
/// text or after whitespace, followed by whitespace and one word that names the model.
fn find_model_command(text: &str) -> Option<ModelCommand<'_>> {
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find(MODEL_COMMAND) {
        let start = search_from + offset;
        let after_command = start + MODEL_COMMAND.len();
        search_from = after_command;
        let starts_word = text[..start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let rest = &text[after_command..];
        let argument = rest.trim_start();
        let separated = argument.len() < rest.len();
        if !starts_word || !separated {
            continue;
        }
        let argument_start = after_command + (rest.len() - argument.len());
        let model_length = argument.find(char::is_whitespace).unwrap_or(argument.len());
        if model_length == 0 {
            continue;
        }
        return Some(ModelCommand {
            model: &argument[..model_length],
            start,
            end: argument_start + model_length,
        });
    }
    None
}

/// The model the first `/model` command of `text` names, if it has one.
pub(super) fn requested_model(text: &str) -> Option<&str> {
    find_model_command(text).map(|command| command.model)
}

/// `text` without its first `/model` command, so the agent reads only what it was asked. The
/// whitespace around the command collapses to one space, or to nothing at either end.
#[cfg(desktop)]
pub(super) fn strip_model_command(text: &str) -> String {
    let Some(command) = find_model_command(text) else {
        return text.to_string();
    };
    let before = text[..command.start].trim_end_matches([' ', '\t']);
    let after = text[command.end..].trim_start_matches([' ', '\t']);
    match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_string(),
        (_, true) => before.to_string(),
        _ if before.ends_with('\n') || after.starts_with('\n') => format!("{before}{after}"),
        _ => format!("{before} {after}"),
    }
}

/// The model and instructions an assignment starts with, frozen on its row.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FrozenWorkSettings {
    pub(super) model_selection: ChatModelSelection,
    pub(super) instructions: String,
}

/// Resolves the settings new work of `agent_id` in `conversation_id` freezes. Work that
/// continues `previous_assignment_id` keeps its predecessor's model and instructions unless the
/// trigger names a model with `/model`. Otherwise the model is the one `/model` names, else the
/// agent's own model, else the provider default, and the instructions are the vault, project,
/// and channel instructions joined. A built-in model line agent freezes its line, such as `opus`,
/// and dispatch runs the line's newest model; a catalog or named agent freezes the model it was
/// made with.
///
/// The model a command names is frozen as named, since the device that creates work may not
/// know the executing device's models; dispatch falls back to the provider default when the
/// executing device's provider does not offer it.
pub(super) async fn resolve_frozen_work_settings(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    agent_id: &str,
    trigger_text: &str,
    previous_assignment_id: Option<&str>,
) -> ChatResult<FrozenWorkSettings> {
    let requested = requested_model(trigger_text)
        .and_then(|model| ModelId::new(model).ok())
        .map(|model_id| ChatModelSelection {
            provider_managed_model: false,
            model_id: Some(model_id),
            model_options: Vec::new(),
        });
    if let Some(previous_assignment_id) = previous_assignment_id
        && let Some((model_data, instructions)) =
            read_assignment_settings(connection, previous_assignment_id).await?
    {
        return Ok(FrozenWorkSettings {
            model_selection: match requested {
                Some(requested) => requested,
                None => parse_model_selection(model_data)?,
            },
            instructions,
        });
    }
    let family = read_agent_family(connection, agent_id).await?;
    let line = family
        .as_deref()
        .and_then(|family| super::agent_kinds::model_agent_line(family, agent_id));
    let model_selection = match (requested, line) {
        (Some(requested), _) => requested,
        (None, Some(line)) => ChatModelSelection {
            provider_managed_model: false,
            model_id: Some(ModelId::new(line).map_err(|_| {
                ChatError::validation("modelId", "The model line is not a valid model id")
            })?),
            model_options: Vec::new(),
        },
        (None, None) => read_agent_default_model(connection, agent_id)
            .await?
            .unwrap_or_else(ChatModelSelection::provider_default),
    };
    Ok(FrozenWorkSettings {
        model_selection,
        instructions: read_channel_instructions(connection, conversation_id).await?,
    })
}

/// The frozen model data and instructions of `assignment_id`, if it exists.
async fn read_assignment_settings(
    connection: &mut SqliteConnection,
    assignment_id: &str,
) -> ChatResult<Option<(String, String)>> {
    sqlx::query_as(
        "SELECT model_selection_data, instructions FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)
}

/// The provider family of the agent `agent_id`, if it is an agent.
async fn read_agent_family(
    connection: &mut SqliteConnection,
    agent_id: &str,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar("SELECT provider_family_id FROM chat_agents WHERE participant_id = ?")
        .bind(agent_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// The default model of `agent_id`, set in Settings > Agents, if it has one.
async fn read_agent_default_model(
    connection: &mut SqliteConnection,
    agent_id: &str,
) -> ChatResult<Option<ChatModelSelection>> {
    let data: Option<String> = sqlx::query_scalar(
        "SELECT policy.model_selection_data
         FROM chat_agents agent
         JOIN chat_agent_policy_revisions policy
           ON policy.id = agent.current_policy_revision_id
          AND policy.agent_id = agent.participant_id
         WHERE agent.participant_id = ?",
    )
    .bind(agent_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    data.map(parse_model_selection).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_command_names_the_word_after_it() {
        assert_eq!(requested_model("/model gpt-5 fix the build"), Some("gpt-5"));
        assert_eq!(
            requested_model("@Codex please /model  o4-mini\nfix it"),
            Some("o4-mini")
        );
        assert_eq!(requested_model("/model\topus"), Some("opus"));
    }

    #[test]
    fn text_without_a_standalone_command_names_no_model() {
        assert_eq!(requested_model("fix the build"), None);
        assert_eq!(requested_model("/model"), None);
        assert_eq!(requested_model("/model   "), None);
        assert_eq!(requested_model("see a/model gpt-5"), None);
        assert_eq!(requested_model("/modelgpt-5"), None);
    }

    #[test]
    fn only_the_first_command_counts() {
        assert_eq!(
            requested_model("/model first then /model second"),
            Some("first")
        );
        assert_eq!(
            strip_model_command("/model first then /model second"),
            "then /model second"
        );
    }

    #[test]
    fn stripping_removes_the_command_and_keeps_the_request() {
        assert_eq!(
            strip_model_command("/model gpt-5 fix the build"),
            "fix the build"
        );
        assert_eq!(
            strip_model_command("@Codex /model gpt-5 fix it"),
            "@Codex fix it"
        );
        assert_eq!(strip_model_command("fix it /model gpt-5"), "fix it");
        assert_eq!(strip_model_command("/model gpt-5\nfix it"), "\nfix it");
        assert_eq!(strip_model_command("fix it\n/model gpt-5"), "fix it\n");
        assert_eq!(strip_model_command("no command here"), "no command here");
        assert_eq!(strip_model_command("/model gpt-5"), "");
    }
}
