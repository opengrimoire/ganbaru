//! Validated nonsecret Chat preferences stored in the active vault config.

use crate::models::{
    ChatError, ChatResult, CredentialReferenceId, InteractionMode, ModelId, ModelOptionSelection,
    ProjectWorkingFolderId, ProviderFamilyId, ProviderInstanceId, SafetyMode, VersionedJson,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CHAT_VAULT_CONFIG_SCHEMA_VERSION: u32 = 1;
const MAX_PROVIDERS: usize = 64;
const MAX_ARGUMENTS: usize = 64;
const MAX_ENVIRONMENT_ROWS: usize = 64;
const MAX_MODELS_PER_SET: usize = 512;
const MAX_REMEMBERED_SELECTIONS: usize = 256;
const MAX_LABEL_BYTES: usize = 160;
const MAX_ARGUMENT_BYTES: usize = 4_096;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 16_384;
const MIN_INSPECTOR_WIDTH_PX: u32 = 240;
const MAX_INSPECTOR_WIDTH_PX: u32 = 960;
const MIN_TERMINAL_SCROLLBACK_LINES: u32 = 1_000;
const MAX_TERMINAL_SCROLLBACK_LINES: u32 = 100_000;
const MIN_IDLE_SESSION_TIMEOUT_SECONDS: u32 = 60;
const MAX_IDLE_SESSION_TIMEOUT_SECONDS: u32 = 7_200;

fn default_inspector_width_px() -> u32 {
    520
}

fn default_terminal_scrollback_lines() -> u32 {
    10_000
}

fn default_idle_session_timeout_seconds() -> u32 {
    900
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSendKey {
    #[default]
    Enter,
    ModEnter,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPortableProviderConfig {
    pub schema_version: u32,
    pub instance_id: ProviderInstanceId,
    pub family_id: ProviderFamilyId,
    pub label: String,
    pub enabled: bool,
    pub launch_arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub credential_references: BTreeMap<String, CredentialReferenceId>,
    pub visible_model_ids: Vec<ModelId>,
    pub favorite_model_ids: Vec<ModelId>,
    pub provider_config: VersionedJson,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberedComposerSelection {
    pub working_folder_id: ProjectWorkingFolderId,
    pub provider_instance_id: ProviderInstanceId,
    #[serde(deserialize_with = "required_nullable")]
    pub model_id: Option<ModelId>,
    pub provider_managed_model: bool,
    pub model_options: Vec<ModelOptionSelection>,
    pub safety_mode: SafetyMode,
    pub interaction_mode: InteractionMode,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPanelPreferences {
    pub inspector_width_px: u32,
}

impl Default for ChatPanelPreferences {
    fn default() -> Self {
        Self {
            inspector_width_px: default_inspector_width_px(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBehaviorPreferences {
    pub send_key: ChatSendKey,
    pub restore_last_selected_thread: bool,
    pub show_reasoning_summaries: bool,
    pub automatically_fold_settled_work: bool,
    pub terminal_scrollback_lines: u32,
    pub idle_session_timeout_seconds: u32,
    pub confirm_multiline_terminal_paste: bool,
}

impl Default for ChatBehaviorPreferences {
    fn default() -> Self {
        Self {
            send_key: ChatSendKey::default(),
            restore_last_selected_thread: true,
            show_reasoning_summaries: true,
            automatically_fold_settled_work: true,
            terminal_scrollback_lines: default_terminal_scrollback_lines(),
            idle_session_timeout_seconds: default_idle_session_timeout_seconds(),
            confirm_multiline_terminal_paste: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatVaultConfig {
    pub schema_version: u32,
    pub providers: Vec<ChatPortableProviderConfig>,
    pub automatic_provider_setup_disabled: BTreeSet<ProviderFamilyId>,
    pub remembered_selections: Vec<RememberedComposerSelection>,
    pub working_folder_provider_preferences: BTreeMap<ProjectWorkingFolderId, ProviderInstanceId>,
    pub panels: ChatPanelPreferences,
    pub behavior: ChatBehaviorPreferences,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, Value>,
}

impl Default for ChatVaultConfig {
    fn default() -> Self {
        Self {
            schema_version: CHAT_VAULT_CONFIG_SCHEMA_VERSION,
            providers: Vec::new(),
            automatic_provider_setup_disabled: BTreeSet::new(),
            remembered_selections: Vec::new(),
            working_folder_provider_preferences: BTreeMap::new(),
            panels: ChatPanelPreferences::default(),
            behavior: ChatBehaviorPreferences::default(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

impl ChatVaultConfig {
    pub fn validate(&self) -> ChatResult<()> {
        if self.schema_version != CHAT_VAULT_CONFIG_SCHEMA_VERSION {
            return Err(ChatError::validation(
                "chat.schemaVersion",
                "Chat config schema is unsupported",
            ));
        }
        if self.providers.len() > MAX_PROVIDERS {
            return Err(ChatError::validation(
                "chat.providers",
                format!("Chat config supports at most {MAX_PROVIDERS} provider instances"),
            ));
        }
        if self.automatic_provider_setup_disabled.len() > MAX_PROVIDERS {
            return Err(ChatError::validation(
                "chat.automaticProviderSetupDisabled",
                format!("Chat config supports at most {MAX_PROVIDERS} disabled provider families"),
            ));
        }
        let mut instance_ids = std::collections::BTreeSet::new();
        for (index, provider) in self.providers.iter().enumerate() {
            validate_provider(provider, index)?;
            if !instance_ids.insert(provider.instance_id.as_str()) {
                return Err(ChatError::validation(
                    format!("chat.providers[{index}].instanceId"),
                    "provider instance ID must be unique",
                ));
            }
        }
        if self.remembered_selections.len() > MAX_REMEMBERED_SELECTIONS {
            return Err(ChatError::validation(
                "chat.rememberedSelections",
                format!(
                    "Chat config supports at most {MAX_REMEMBERED_SELECTIONS} remembered selections"
                ),
            ));
        }
        for (index, selection) in self.remembered_selections.iter().enumerate() {
            if selection.model_id.is_none() && !selection.provider_managed_model {
                return Err(ChatError::validation(
                    format!("chat.rememberedSelections[{index}].modelId"),
                    "a model ID or provider-managed model state is required",
                ));
            }
        }
        for provider_instance_id in self.working_folder_provider_preferences.values() {
            if !instance_ids.contains(provider_instance_id.as_str()) {
                return Err(ChatError::validation(
                    "chat.workingFolderProviderPreferences",
                    "working-folder provider preference references an unknown provider instance",
                ));
            }
        }
        validate_range(
            self.panels.inspector_width_px,
            MIN_INSPECTOR_WIDTH_PX,
            MAX_INSPECTOR_WIDTH_PX,
            "chat.panels.inspectorWidthPx",
        )?;
        validate_range(
            self.behavior.terminal_scrollback_lines,
            MIN_TERMINAL_SCROLLBACK_LINES,
            MAX_TERMINAL_SCROLLBACK_LINES,
            "chat.behavior.terminalScrollbackLines",
        )?;
        validate_range(
            self.behavior.idle_session_timeout_seconds,
            MIN_IDLE_SESSION_TIMEOUT_SECONDS,
            MAX_IDLE_SESSION_TIMEOUT_SECONDS,
            "chat.behavior.idleSessionTimeoutSeconds",
        )
    }
}

pub fn parse_chat_config_branch(root: &Value) -> ChatResult<ChatVaultConfig> {
    let root = root
        .as_object()
        .ok_or_else(|| ChatError::validation("config", "vault config must be an object"))?;
    let Some(chat) = root.get("chat") else {
        return Ok(ChatVaultConfig::default());
    };
    let config: ChatVaultConfig = serde_json::from_value(chat.clone())
        .map_err(|_| ChatError::validation("chat", "Chat config has an invalid shape"))?;
    config.validate()?;
    Ok(config)
}

pub fn replace_chat_config_branch(root: &mut Value, config: ChatVaultConfig) -> ChatResult<()> {
    config.validate()?;
    let root = root
        .as_object_mut()
        .ok_or_else(|| ChatError::validation("config", "vault config must be an object"))?;
    root.insert(
        "chat".to_string(),
        serde_json::to_value(config).map_err(|_| {
            ChatError::new(
                crate::models::ChatErrorCode::Internal,
                "serialize Chat config",
                false,
            )
        })?,
    );
    Ok(())
}

fn validate_provider(provider: &ChatPortableProviderConfig, index: usize) -> ChatResult<()> {
    validate_bounded_string(
        &provider.label,
        MAX_LABEL_BYTES,
        &format!("chat.providers[{index}].label"),
    )?;
    if provider.launch_arguments.len() > MAX_ARGUMENTS {
        return Err(ChatError::validation(
            format!("chat.providers[{index}].launchArguments"),
            format!("provider supports at most {MAX_ARGUMENTS} launch arguments"),
        ));
    }
    for (argument_index, argument) in provider.launch_arguments.iter().enumerate() {
        validate_max_bytes(
            argument,
            MAX_ARGUMENT_BYTES,
            &format!("chat.providers[{index}].launchArguments[{argument_index}]"),
        )?;
    }
    if provider.environment.len() + provider.credential_references.len() > MAX_ENVIRONMENT_ROWS {
        return Err(ChatError::validation(
            format!("chat.providers[{index}].environment"),
            format!("provider supports at most {MAX_ENVIRONMENT_ROWS} environment rows"),
        ));
    }
    for (name, value) in &provider.environment {
        if !is_valid_environment_name(name) {
            return Err(ChatError::validation(
                format!("chat.providers[{index}].environment"),
                "environment variable names use ASCII letters, digits, and underscores",
            ));
        }
        validate_max_bytes(
            value,
            MAX_ENVIRONMENT_VALUE_BYTES,
            &format!("chat.providers[{index}].environment.{name}"),
        )?;
    }
    for name in provider.credential_references.keys() {
        if !is_valid_environment_name(name) || provider.environment.contains_key(name) {
            return Err(ChatError::validation(
                format!("chat.providers[{index}].credentialReferences"),
                "credential environment names must be valid and have one source",
            ));
        }
    }
    if provider.visible_model_ids.len() > MAX_MODELS_PER_SET
        || provider.favorite_model_ids.len() > MAX_MODELS_PER_SET
    {
        return Err(ChatError::validation(
            format!("chat.providers[{index}].visibleModelIds"),
            format!("model preference sets support at most {MAX_MODELS_PER_SET} entries"),
        ));
    }
    if ["executable", "providerHome", "canonicalPath", "lastProbe"]
        .iter()
        .any(|field| provider.unknown_fields.contains_key(*field))
    {
        return Err(ChatError::validation(
            format!("chat.providers[{index}]"),
            "machine-specific provider paths and probes belong in device state",
        ));
    }
    Ok(())
}

fn is_valid_environment_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn validate_bounded_string(value: &str, maximum: usize, field: &str) -> ChatResult<()> {
    if value.trim().is_empty() {
        return Err(ChatError::validation(field, "value is required"));
    }
    validate_max_bytes(value, maximum, field)
}

fn validate_max_bytes(value: &str, maximum: usize, field: &str) -> ChatResult<()> {
    if value.len() > maximum {
        return Err(ChatError::validation(
            field,
            format!("value exceeds the {maximum} byte limit"),
        ));
    }
    Ok(())
}

fn validate_range(value: u32, minimum: u32, maximum: u32, field: &str) -> ChatResult<()> {
    if !(minimum..=maximum).contains(&value) {
        return Err(ChatError::validation(
            field,
            format!("value must be between {minimum} and {maximum}"),
        ));
    }
    Ok(())
}
