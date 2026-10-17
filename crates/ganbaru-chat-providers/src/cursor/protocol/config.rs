//! ACP mode, model, and session configuration validation.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

const MAX_CONFIG_OPTIONS: usize = 128;
const MAX_MODE_OPTIONS: usize = 32;
const MAX_SELECT_CHOICES: usize = 128;

pub(in crate::cursor) fn resolve_configuration_updates(
    config_options: &[AcpConfigOption],
    model_id: Option<&ModelId>,
    selections: &[ModelOptionSelection],
) -> ChatResult<Vec<ResolvedConfigUpdate>> {
    validate_config_options(config_options)?;
    let mut requested = BTreeMap::new();
    for selection in selections {
        if requested
            .insert(selection.key.as_str(), selection)
            .is_some()
        {
            return Err(ChatError::validation(
                "modelOptions",
                "Cursor model option keys must be unique",
            ));
        }
    }
    let mut updates = Vec::new();
    if let Some(model) = model_id {
        let option = model_config_option(config_options).ok_or_else(|| {
            ChatError::validation("modelId", "Cursor did not advertise model selection")
        })?;
        ensure_select_value(option, model.as_str())?;
        push_update(
            &mut updates,
            option,
            Value::String(model.as_str().to_string()),
        );
    }
    for (key, selection) in requested {
        let option = config_options
            .iter()
            .find(|option| canonical_option_key(option) == Some(key))
            .ok_or_else(|| {
                ChatError::validation(
                    "modelOptions",
                    format!("Cursor did not advertise the {key} model option"),
                )
            })?;
        let value = match (&selection.value, option.option_type.as_str()) {
            (ModelOptionValue::Boolean(value), "boolean") => Value::Bool(*value),
            (ModelOptionValue::Choice(value), "select") => {
                ensure_select_value(option, value)?;
                Value::String(value.clone())
            }
            _ => {
                return Err(ChatError::validation(
                    "modelOptions",
                    format!("Cursor model option {key} has the wrong value type"),
                ));
            }
        };
        push_update(&mut updates, option, value);
    }
    Ok(updates)
}

pub(super) fn model_config_option(options: &[AcpConfigOption]) -> Option<&AcpConfigOption> {
    options.iter().find(|option| {
        option.category.as_deref() == Some("model") || option.id.eq_ignore_ascii_case("model")
    })
}

pub(in crate::cursor) fn find_mode(
    modes: &AcpModeState,
    interaction: InteractionMode,
) -> Option<&AcpMode> {
    let aliases: &[&str] = match interaction {
        InteractionMode::Plan => &["plan", "architect"],
        InteractionMode::Build => &["code", "agent", "default", "chat", "implement", "ask"],
    };
    aliases.iter().find_map(|alias| {
        modes.available_modes.iter().find(|mode| {
            mode.id.eq_ignore_ascii_case(alias) || mode.name.eq_ignore_ascii_case(alias)
        })
    })
}

pub(in crate::cursor) fn confirmed_session_not_found(code: i64, message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    matches!(code, -32001 | -32004 | -32602)
        && (message.contains("session") || message.contains("conversation"))
        && (message.contains("not found")
            || message.contains("does not exist")
            || message.contains("unknown"))
}

pub(super) fn model_option_definitions(options: &[AcpConfigOption]) -> Vec<ModelOptionDefinition> {
    options
        .iter()
        .filter_map(|option| {
            let key = canonical_option_key(option)?.to_string();
            let label = option.name.trim().to_string();
            match option.option_type.as_str() {
                "boolean" => Some(ModelOptionDefinition::Boolean {
                    key,
                    label,
                    description: option.description.clone(),
                    default_value: option.current_value.as_bool(),
                }),
                "select" => Some(ModelOptionDefinition::Choice {
                    key,
                    label,
                    description: option.description.clone(),
                    options: flattened_select_options(option),
                    default_value: option.current_value.as_str().map(str::to_string),
                }),
                _ => None,
            }
        })
        .collect()
}

fn canonical_option_key(option: &AcpConfigOption) -> Option<&'static str> {
    let id = option.id.to_ascii_lowercase();
    let name = option.name.to_ascii_lowercase();
    if ["effort", "reasoning"].contains(&id.as_str())
        || name.contains("effort")
        || name.contains("reasoning")
    {
        Some("reasoning")
    } else if ["context", "context_size"].contains(&id.as_str()) || name.contains("context") {
        Some("contextWindow")
    } else if id == "fast" || name.contains("fast mode") {
        Some("fastMode")
    } else if id == "thinking" || name.contains("thinking") {
        Some("thinking")
    } else {
        None
    }
}

pub(super) fn validate_config_options(options: &[AcpConfigOption]) -> ChatResult<()> {
    if options.len() > MAX_CONFIG_OPTIONS {
        return Err(protocol_error("session configuration options"));
    }
    let mut ids = BTreeSet::new();
    for option in options {
        if !is_valid_identifier(&option.id, 256)
            || option.name.trim().is_empty()
            || option.name.len() > 256
            || option.name.chars().any(char::is_control)
            || option.description.as_ref().is_some_and(|description| {
                description.len() > 2_048 || description.chars().any(char::is_control)
            })
            || !matches!(option.option_type.as_str(), "select" | "boolean")
            || !ids.insert(option.id.clone())
        {
            return Err(protocol_error("session configuration option"));
        }
        if option.option_type == "boolean" {
            if !option.current_value.is_boolean() {
                return Err(protocol_error("boolean configuration option"));
            }
            continue;
        }
        let declared_count = declared_select_choice_count(option)
            .ok_or_else(|| protocol_error("select configuration option"))?;
        let choices = flattened_select_options(option);
        let current = option
            .current_value
            .as_str()
            .ok_or_else(|| protocol_error("selected configuration value"))?;
        let mut values = BTreeSet::new();
        if declared_count == 0
            || declared_count > MAX_SELECT_CHOICES
            || choices.len() != declared_count
            || choices
                .iter()
                .any(|choice| !values.insert(choice.value.as_str()))
            || !choices.iter().any(|choice| choice.value == current)
        {
            return Err(protocol_error("select configuration option"));
        }
    }
    Ok(())
}

pub(super) fn validate_modes(modes: &AcpModeState) -> ChatResult<()> {
    if modes.available_modes.is_empty()
        || modes.available_modes.len() > MAX_MODE_OPTIONS
        || !is_valid_identifier(&modes.current_mode_id, 256)
        || modes.available_modes.iter().any(|mode| {
            !is_valid_identifier(&mode.id, 256)
                || mode.name.trim().is_empty()
                || mode.name.len() > 256
        })
    {
        return Err(protocol_error("session modes"));
    }
    Ok(())
}

pub(super) fn derive_modes_from_config(options: &[AcpConfigOption]) -> Option<AcpModeState> {
    let option = options.iter().find(|option| {
        option.id.eq_ignore_ascii_case("mode") || option.category.as_deref() == Some("mode")
    })?;
    let current_mode_id = option.current_value.as_str()?.to_string();
    let available_modes = flattened_select_options(option)
        .into_iter()
        .map(|choice| AcpMode {
            id: choice.value,
            name: choice.label,
            description: choice.description,
        })
        .collect::<Vec<_>>();
    (!available_modes.is_empty()).then_some(AcpModeState {
        current_mode_id,
        available_modes,
    })
}

fn flattened_select_options(option: &AcpConfigOption) -> Vec<ModelChoiceOption> {
    option
        .options
        .iter()
        .flat_map(|entry| {
            if let Some(object) = entry.as_object() {
                if let (Some(value), Some(name)) = (
                    object.get("value").and_then(Value::as_str),
                    object.get("name").and_then(Value::as_str),
                ) {
                    return vec![choice(value, name, object.get("description"))];
                }
                if let Some(group) = object.get("options").and_then(Value::as_array) {
                    return group
                        .iter()
                        .filter_map(|nested| {
                            let nested = nested.as_object()?;
                            Some(choice(
                                nested.get("value")?.as_str()?,
                                nested.get("name")?.as_str()?,
                                nested.get("description"),
                            ))
                        })
                        .collect();
                }
            }
            Vec::new()
        })
        .filter(|entry| {
            is_valid_identifier(&entry.value, 256)
                && !entry.label.trim().is_empty()
                && entry.label.len() <= 256
                && !entry.label.chars().any(char::is_control)
                && entry.description.as_ref().is_none_or(|description| {
                    description.len() <= 2_048 && !description.chars().any(char::is_control)
                })
        })
        .take(MAX_SELECT_CHOICES + 1)
        .collect()
}

fn declared_select_choice_count(option: &AcpConfigOption) -> Option<usize> {
    let mut count = 0_usize;
    for entry in &option.options {
        let object = entry.as_object()?;
        if object.get("value").and_then(Value::as_str).is_some()
            && object.get("name").and_then(Value::as_str).is_some()
        {
            count = count.checked_add(1)?;
            continue;
        }
        let group = object.get("options").and_then(Value::as_array)?;
        if group.iter().any(|nested| {
            nested.as_object().is_none_or(|nested| {
                nested.get("value").and_then(Value::as_str).is_none()
                    || nested.get("name").and_then(Value::as_str).is_none()
            })
        }) {
            return None;
        }
        count = count.checked_add(group.len())?;
        if count > MAX_SELECT_CHOICES {
            return Some(count);
        }
    }
    Some(count)
}

fn choice(value: &str, name: &str, description: Option<&Value>) -> ModelChoiceOption {
    ModelChoiceOption {
        value: value.to_string(),
        label: name.to_string(),
        description: description.and_then(Value::as_str).map(str::to_string),
    }
}

pub(super) fn ensure_select_value(option: &AcpConfigOption, value: &str) -> ChatResult<()> {
    if flattened_select_options(option)
        .iter()
        .any(|candidate| candidate.value == value)
    {
        Ok(())
    } else {
        Err(ChatError::validation(
            "modelOptions",
            format!("Cursor did not offer value {value} for {}", option.id),
        ))
    }
}

fn push_update(updates: &mut Vec<ResolvedConfigUpdate>, option: &AcpConfigOption, value: Value) {
    if option.current_value != value {
        updates.push(ResolvedConfigUpdate {
            config_id: option.id.clone(),
            value,
            previous_value: option.current_value.clone(),
        });
    }
}
