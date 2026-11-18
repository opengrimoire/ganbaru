//! The built-in agents of each provider family, and what each family's provider does on this
//! device.
//!
//! Every device that detects a provider family creates the built-in agent of each model line the
//! family offers, such as Opus, and the agent of each model its catalog offers, such as Opus 5.5,
//! under ids derived from the family and the line or model, so devices converge on the same
//! agents. Which provider installation runs an agent is a device-local choice made
//! here.

use super::agent_kinds::{family_model_lines, line_display_name};
use super::common::new_id;
use super::reads::LocalProviderInstances;
use super::workflow::insert_policy_revision;
use super::{identifier_error, now_timestamp, persistence_error};
use crate::chat::device_state::read_active_device_scope;
use ganbaru_chat::conversion::{catalog_agent_id, line_agent_id};
use ganbaru_chat::coordination::contracts::ChatAgentPolicyInput;
use ganbaru_chat_contracts::models::*;
use ganbaru_chat_providers::model_recommendation::is_model_line;
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::BTreeMap;

/// The provider installation that runs one family's agent on this device.
#[derive(Clone, Debug)]
pub(crate) struct DeviceProvider {
    pub(crate) instance_id: ProviderInstanceId,
    /// What the provider enforces here, or no support when it cannot start.
    pub(crate) support: ProviderAuthoritySupport,
    /// Whether the provider can start here: enabled, located, and approved on this device.
    pub(crate) ready: bool,
}

/// The provider of each family configured on this device.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeviceProviders {
    by_family: BTreeMap<String, DeviceProvider>,
    /// The agent of each model the family's catalog offers on this device.
    catalog_agents: BTreeMap<String, Vec<CatalogAgent>>,
}

impl DeviceProviders {
    /// Reads the providers of this device from its Chat settings.
    pub(crate) fn read(app: &tauri::AppHandle) -> ChatResult<Self> {
        let config = crate::chat::configuration::read_chat_config(app)?;
        let scope =
            read_active_device_scope(app).map_err(crate::chat::settings::device_state_error)?;
        let mut candidates: BTreeMap<String, Vec<ProviderInstanceId>> = BTreeMap::new();
        for provider in config.providers.iter().filter(|provider| provider.enabled) {
            candidates
                .entry(provider.family_id.as_str().to_string())
                .or_default()
                .push(provider.instance_id.clone());
        }
        let mut by_family = BTreeMap::new();
        let mut catalog_agents = BTreeMap::new();
        for (family, instances) in candidates {
            let Some(instance_id) = select_local_instance(&family, &instances) else {
                continue;
            };
            let Some(portable) = config
                .providers
                .iter()
                .find(|provider| provider.instance_id == instance_id)
            else {
                continue;
            };
            let read =
                crate::chat::settings::provider_instance_read(portable, &scope.provider_instances);
            let models = read
                .model_catalog
                .as_ref()
                .map(|catalog| catalog.models.as_slice())
                .unwrap_or_default();
            catalog_agents.insert(
                family.clone(),
                catalog_agents_of(models, &read.configuration.visible_model_ids),
            );
            let driver = ProviderDriverRegistry.create_driver(read.configuration.clone());
            let ready = driver.is_ok()
                && !read.configuration.executable.trim().is_empty()
                && read.runtime_review.is_empty();
            let support = match driver {
                Ok(driver) if ready => driver.authority_support(),
                _ => ProviderAuthoritySupport::default(),
            };
            by_family.insert(
                family,
                DeviceProvider {
                    instance_id,
                    support,
                    ready,
                },
            );
        }
        Ok(Self {
            by_family,
            catalog_agents,
        })
    }

    /// Providers of this device built from `entries`, for tests that need no Chat settings.
    #[cfg(test)]
    pub(crate) fn from_entries(
        entries: impl IntoIterator<Item = (String, DeviceProvider)>,
    ) -> Self {
        Self {
            by_family: entries.into_iter().collect(),
            catalog_agents: BTreeMap::new(),
        }
    }

    /// The provider of `family` on this device, if one is configured.
    pub(crate) fn get(&self, family: &str) -> Option<&DeviceProvider> {
        self.by_family.get(family)
    }

    /// What the provider of `family` enforces here; no support when it is absent or not ready,
    /// so the agent cannot work on this device.
    pub(crate) fn support(&self, family: &str) -> ProviderAuthoritySupport {
        self.get(family)
            .filter(|provider| provider.ready)
            .map(|provider| provider.support)
            .unwrap_or_default()
    }

    /// The provider instance of each configured family.
    pub(crate) fn local_instances(&self) -> LocalProviderInstances {
        self.by_family
            .iter()
            .map(|(family, provider)| (family.clone(), provider.instance_id.clone()))
            .collect()
    }
}

/// The providers of this device, or none when its Chat settings cannot be read, so no agent
/// works here until they can.
pub(crate) fn read_device_providers(app: &tauri::AppHandle) -> DeviceProviders {
    DeviceProviders::read(app).unwrap_or_else(|error| {
        eprintln!("Chat could not read this device's providers: {error}");
        DeviceProviders::default()
    })
}

/// Picks the instance that runs `family` on this device: the instance whose id is the family
/// itself, which automatic setup creates, else the lowest id, so the choice is stable.
pub(crate) fn select_local_instance(
    family: &str,
    instances: &[ProviderInstanceId],
) -> Option<ProviderInstanceId> {
    instances
        .iter()
        .find(|instance| instance.as_str() == family)
        .or_else(|| {
            instances
                .iter()
                .min_by(|left, right| left.as_str().cmp(right.as_str()))
        })
        .cloned()
}

/// Creates the built-in agents of each provider family configured on this device that the vault
/// lacks.
pub(crate) async fn ensure_device_provider_agents(
    pool: &SqlitePool,
    providers: &DeviceProviders,
) -> ChatResult<()> {
    let families: BTreeMap<String, Vec<CatalogAgent>> = providers
        .by_family
        .keys()
        .map(|family| {
            let agents = providers
                .catalog_agents
                .get(family)
                .cloned()
                .unwrap_or_default();
            (family.clone(), agents)
        })
        .collect();
    ensure_provider_agents(pool, &families).await
}

/// The agent one catalog model gets: the model it always runs, the name it is tagged by, and
/// the options it starts with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CatalogAgent {
    pub(crate) model_id: ModelId,
    pub(crate) display_name: String,
    pub(crate) model_options: Vec<ModelOptionSelection>,
    /// Whether the agent starts turned off, because the model is hidden on this device.
    pub(crate) archived: bool,
}

/// The agent of each model in `models` that can run: every model but a model line alias such as
/// `opus`, which its line's agent already stands for, and those the provider reports
/// unavailable. A model outside `visible_model_ids` starts turned off, unless that list is
/// empty, which shows every model.
pub(crate) fn catalog_agents_of(
    models: &[ProviderModel],
    visible_model_ids: &[ModelId],
) -> Vec<CatalogAgent> {
    models
        .iter()
        .filter(|model| {
            !is_model_line(model.id.as_str())
                && !matches!(
                    model.availability,
                    ModelAvailability::Unavailable | ModelAvailability::Deprecated
                )
        })
        .filter_map(|model| {
            let display_name = agent_model_name(&model.display_name);
            (!display_name.is_empty() && !display_name.contains('@')).then(|| CatalogAgent {
                model_id: model.id.clone(),
                display_name,
                model_options: default_effort(&model.options).into_iter().collect(),
                archived: !visible_model_ids.is_empty() && !visible_model_ids.contains(&model.id),
            })
        })
        .collect()
}

/// The name a model is tagged by, from the `display_name` its catalog gives it: hyphens become
/// spaces, and an OpenAI name such as `GPT-6-Luna` puts its line first, as `Luna 6`, since
/// every OpenAI model is a GPT.
pub(crate) fn agent_model_name(display_name: &str) -> String {
    let spaced = display_name
        .replace('-', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut words = spaced.split(' ');
    let (Some(first), Some(version)) = (words.next(), words.next()) else {
        return spaced;
    };
    let rest: Vec<&str> = words.collect();
    let is_version = version.starts_with(|character: char| character.is_ascii_digit())
        && version
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.');
    if first.eq_ignore_ascii_case("gpt") && is_version && !rest.is_empty() {
        format!("{} {version}", rest.join(" "))
    } else {
        spaced
    }
}

/// The effort a new agent of a model starts with: `medium` when the model offers it, else its
/// highest effort, or nothing when the model has no effort option.
pub(crate) fn default_effort(options: &[ModelOptionDefinition]) -> Option<ModelOptionSelection> {
    options.iter().find_map(|definition| {
        let ModelOptionDefinition::Choice {
            key,
            label,
            options,
            ..
        } = definition
        else {
            return None;
        };
        let identity = format!("{key} {label}").to_lowercase();
        if !identity.contains("effort") && !identity.contains("reasoning") {
            return None;
        }
        let choice = options
            .iter()
            .find(|option| option.value.eq_ignore_ascii_case("medium"))
            .or_else(|| options.last())?;
        Some(ModelOptionSelection {
            key: key.clone(),
            value: ModelOptionValue::Choice(choice.value.clone()),
        })
    })
}

/// Creates, for each family of `families`, the built-in agent of each model line the family
/// offers, such as Opus or Sol, under the id derived from the family and line, and the agent of
/// each of its catalog models, such as Opus 5.5, under the id derived from the family and model.
/// A catalog agent always runs its model and starts with its effort, and it is skipped when
/// another agent already holds its name, so each `@` mention names one agent. An existing agent,
/// archived or not, is left as it is, so running it again changes nothing and two devices that
/// detect one family write the same agents.
pub(crate) async fn ensure_provider_agents(
    pool: &SqlitePool,
    families: &BTreeMap<String, Vec<CatalogAgent>>,
) -> ChatResult<()> {
    if families.is_empty() {
        return Ok(());
    }
    let now = now_timestamp()?;
    let invalid_family =
        |_| ChatError::validation("providerFamilyId", "The provider family is not valid");
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    for (family, catalog_agents) in families {
        for line in family_model_lines(family) {
            let agent_id = line_agent_id(family, line).map_err(invalid_family)?;
            let display_name = line_display_name(line);
            insert_agent(&mut transaction, &agent_id, family, &display_name, &now).await?;
        }
        for agent in catalog_agents {
            let agent_id =
                catalog_agent_id(family, agent.model_id.as_str()).map_err(invalid_family)?;
            insert_catalog_agent(&mut transaction, &agent_id, family, agent, &now).await?;
        }
    }
    transaction.commit().await.map_err(persistence_error)
}

/// Inserts the catalog agent `agent_id` of `family` with its model as policy, turned off when
/// `agent` starts archived, unless the vault has it or another agent holds its name.
async fn insert_catalog_agent(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &str,
    family: &str,
    agent: &CatalogAgent,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let taken: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM chat_participants
         WHERE participant_kind = 'ai_agent' AND lower(display_name) = lower(?) AND id <> ?",
    )
    .bind(&agent.display_name)
    .bind(agent_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if taken.is_some() {
        return Ok(());
    }
    if !insert_agent(transaction, agent_id, family, &agent.display_name, now).await? {
        return Ok(());
    }
    let participant_id = ChatParticipantId::new(agent_id).map_err(identifier_error)?;
    let policy = ChatAgentPolicyInput {
        provider_managed_model: false,
        model_id: Some(agent.model_id.clone()),
        model_options: agent.model_options.clone(),
        provider_options: VersionedJson {
            schema_version: 1,
            value: serde_json::json!({}),
        },
        interaction_mode: InteractionMode::Build,
    };
    insert_policy_revision(transaction, &participant_id, &policy, now).await?;
    if agent.archived {
        sqlx::query("UPDATE chat_participants SET archived_at = ?, updated_at = ? WHERE id = ?")
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(agent_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
    }
    Ok(())
}

/// The longest name, in characters, a person can give an agent.
const MAX_AGENT_NAME_CHARACTERS: usize = 32;

/// The name a person gave a new agent, trimmed, or a validation error when it is empty, too long,
/// or holds characters that would break an `@` mention: whitespace, `@`, or control characters.
pub(super) fn validate_agent_name(display_name: &str) -> ChatResult<&str> {
    let name = display_name.trim();
    let invalid = |message: &str| ChatError::validation("displayName", message);
    if name.is_empty() {
        return Err(invalid("Name the agent"));
    }
    if name.chars().count() > MAX_AGENT_NAME_CHARACTERS {
        return Err(invalid("The agent name is too long"));
    }
    if name
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '@')
    {
        return Err(invalid("The agent name cannot hold spaces or @"));
    }
    Ok(name)
}

/// Creates an agent of `family` named `display_name` that runs `policy`, under a random id, and
/// returns its id. The name must differ, ignoring case, from every agent's name, archived ones
/// included, from every built-in model line, and from every provider family, so each `@` mention names
/// one agent.
pub(super) async fn create_custom_agent(
    pool: &SqlitePool,
    family: &ProviderFamilyId,
    display_name: &str,
    policy: &ChatAgentPolicyInput,
) -> ChatResult<ChatParticipantId> {
    let name = validate_agent_name(display_name)?;
    if is_reserved_agent_name(name) {
        return Err(name_taken());
    }
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let taken: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM chat_participants
         WHERE participant_kind = 'ai_agent' AND lower(display_name) = lower(?)",
    )
    .bind(name)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if taken.is_some() {
        return Err(name_taken());
    }
    let agent_id = ChatParticipantId::new(new_id("agent")?).map_err(identifier_error)?;
    insert_agent(
        &mut transaction,
        agent_id.as_str(),
        family.as_str(),
        name,
        &now,
    )
    .await?;
    insert_policy_revision(&mut transaction, &agent_id, policy, &now).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(agent_id)
}

/// Whether `name` belongs, ignoring case, to a built-in model line or to a provider family, by id or
/// display name, so a named agent is never mistaken for a whole provider.
fn is_reserved_agent_name(name: &str) -> bool {
    is_model_line(name)
        || ProviderDriverRegistry.list_metadata().iter().any(|family| {
            family.display_name.eq_ignore_ascii_case(name)
                || family.family_id.as_str().eq_ignore_ascii_case(name)
        })
}

fn name_taken() -> ChatError {
    ChatError::validation("displayName", "Another agent already has this name")
}

/// Inserts the agent `agent_id` of `family` named `display_name` unless the vault has it, and
/// returns whether it did.
pub(super) async fn insert_agent(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &str,
    family: &str,
    display_name: &str,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let inserted = sqlx::query(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES (?, 'ai_agent', ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(agent_id)
    .bind(display_name)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected()
        > 0;
    sqlx::query(
        "INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         SELECT ?1, ?2, ?3, ?3
         WHERE EXISTS (
             SELECT 1 FROM chat_participants
             WHERE id = ?1 AND participant_kind = 'ai_agent'
         )
         ON CONFLICT DO NOTHING",
    )
    .bind(agent_id)
    .bind(family)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(id: &str) -> ProviderInstanceId {
        ProviderInstanceId::new(id.to_string()).expect("instance id")
    }

    #[test]
    fn the_instance_named_after_the_family_runs_its_agent() {
        let instances = [
            instance("codex-work"),
            instance("codex"),
            instance("a-codex"),
        ];
        assert_eq!(
            select_local_instance("codex", &instances),
            Some(instance("codex"))
        );
    }

    #[test]
    fn the_lowest_instance_id_runs_the_agent_without_a_family_instance() {
        let instances = [instance("codex-work"), instance("codex-home")];
        assert_eq!(
            select_local_instance("codex", &instances),
            Some(instance("codex-home"))
        );
        assert_eq!(select_local_instance("codex", &[]), None);
    }

    #[test]
    fn a_provider_that_is_not_ready_supports_nothing() {
        let support = ProviderAuthoritySupport {
            isolated_conversation: true,
            writable_root: true,
            ..ProviderAuthoritySupport::default()
        };
        let providers = DeviceProviders::from_entries([
            (
                "codex".to_string(),
                DeviceProvider {
                    instance_id: instance("codex"),
                    support,
                    ready: true,
                },
            ),
            (
                "claude".to_string(),
                DeviceProvider {
                    instance_id: instance("claude"),
                    support,
                    ready: false,
                },
            ),
        ]);
        assert_eq!(providers.support("codex"), support);
        assert_eq!(
            providers.support("claude"),
            ProviderAuthoritySupport::default()
        );
        assert_eq!(
            providers.support("cursor"),
            ProviderAuthoritySupport::default()
        );
        assert!(
            providers
                .get("codex")
                .is_some_and(|provider| provider.ready)
        );
        assert!(
            !providers
                .get("claude")
                .is_some_and(|provider| provider.ready)
        );
        assert_eq!(providers.local_instances().len(), 2);
    }

    fn policy(model: &str) -> ChatAgentPolicyInput {
        ChatAgentPolicyInput {
            provider_managed_model: false,
            model_id: Some(ModelId::new(model).unwrap()),
            model_options: Vec::new(),
            provider_options: VersionedJson {
                schema_version: 1,
                value: serde_json::json!({}),
            },
            interaction_mode: InteractionMode::Build,
        }
    }

    #[test]
    fn a_named_agent_keeps_its_model_and_a_name_no_other_agent_or_line_holds() {
        tauri::async_runtime::block_on(async {
            let pool = super::super::tests::migrated_pool().await;
            ensure_provider_agents(&pool, &family("claude", Vec::new()))
                .await
                .unwrap();
            let claude = ProviderFamilyId::new("claude").unwrap();

            let reviewer = create_custom_agent(&pool, &claude, " Reviewer ", &policy("sonnet"))
                .await
                .unwrap();
            let read =
                super::super::reads::read_agent(&pool, &reviewer, &LocalProviderInstances::new())
                    .await
                    .unwrap();
            assert_eq!(read.participant.display_name, "Reviewer");
            assert_eq!(read.agent_kind, ChatAgentKind::Custom);
            assert_eq!(
                read.latest_policy.and_then(|policy| policy.model_id),
                Some(ModelId::new("sonnet").unwrap())
            );

            for taken in [
                "reviewer",
                "CLAUDE",
                "Opus",
                "sol",
                "Cursor",
                "opencode",
                "two words",
                "@me",
                "",
            ] {
                let error = create_custom_agent(&pool, &claude, taken, &policy("sonnet"))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ChatErrorCode::Validation, "{taken}");
            }
        });
    }

    fn family(name: &str, agents: Vec<CatalogAgent>) -> BTreeMap<String, Vec<CatalogAgent>> {
        BTreeMap::from([(name.to_string(), agents)])
    }

    fn catalog_model(id: &str, display_name: &str) -> ProviderModel {
        ProviderModel {
            id: ModelId::new(id).unwrap(),
            display_name: display_name.to_string(),
            description: None,
            context_limit: None,
            availability: ModelAvailability::Available,
            capabilities: Vec::new(),
            options: Vec::new(),
            custom: false,
        }
    }

    fn effort(values: &[&str]) -> ModelOptionDefinition {
        ModelOptionDefinition::Choice {
            key: "effort".to_string(),
            label: "Effort".to_string(),
            description: None,
            options: values
                .iter()
                .map(|value| ModelChoiceOption {
                    value: value.to_string(),
                    label: value.to_string(),
                    description: None,
                })
                .collect(),
            default_value: None,
        }
    }

    fn chosen(value: &str) -> Option<ModelOptionSelection> {
        Some(ModelOptionSelection {
            key: "effort".to_string(),
            value: ModelOptionValue::Choice(value.to_string()),
        })
    }

    #[test]
    fn an_openai_model_is_named_by_its_line_and_then_its_version() {
        assert_eq!(agent_model_name("GPT-6-Luna"), "Luna 6");
        assert_eq!(agent_model_name("GPT-5.6-Terra"), "Terra 5.6");
        assert_eq!(agent_model_name("gpt-5.5-codex-mini"), "codex mini 5.5");
        assert_eq!(agent_model_name("GPT-5"), "GPT 5");
        assert_eq!(agent_model_name("  Opus   5.5 "), "Opus 5.5");
        assert_eq!(agent_model_name("Fable-5"), "Fable 5");
    }

    #[test]
    fn a_new_model_agent_starts_at_medium_else_the_highest_effort() {
        assert_eq!(
            default_effort(&[effort(&["low", "medium", "high"])]),
            chosen("medium")
        );
        assert_eq!(default_effort(&[effort(&["low", "high"])]), chosen("high"));
        assert_eq!(default_effort(&[effort(&[])]), None);
        assert_eq!(default_effort(&[]), None);
        let speed = ModelOptionDefinition::Choice {
            key: "speed".to_string(),
            label: "Speed".to_string(),
            description: None,
            options: Vec::new(),
            default_value: None,
        };
        assert_eq!(default_effort(&[speed]), None);
    }

    #[test]
    fn every_runnable_catalog_model_gets_an_agent_off_when_hidden() {
        let mut gone = catalog_model("claude-opus-4-1", "Opus 4.1");
        gone.availability = ModelAvailability::Unavailable;
        let mut sonnet = catalog_model("claude-sonnet-5", "Sonnet 5");
        sonnet.options = vec![effort(&["low", "medium", "high"])];
        let models = [
            catalog_model("opus", "Opus 5.5"),
            catalog_model("claude-opus-4-8", "Opus 4.8"),
            sonnet,
            gone,
        ];

        let all = catalog_agents_of(&models, &[]);
        assert_eq!(
            all.iter()
                .map(|agent| (agent.display_name.as_str(), agent.archived))
                .collect::<Vec<_>>(),
            [("Opus 4.8", false), ("Sonnet 5", false)]
        );
        assert_eq!(
            all[1].model_options,
            chosen("medium").into_iter().collect::<Vec<_>>()
        );

        let visible = [ModelId::new("claude-sonnet-5").unwrap()];
        let archived: Vec<bool> = catalog_agents_of(&models, &visible)
            .iter()
            .map(|agent| agent.archived)
            .collect();
        assert_eq!(archived, [true, false]);
    }

    #[test]
    fn ensuring_a_family_again_converges_on_one_agent_with_its_derived_id() {
        tauri::async_runtime::block_on(async {
            let pool = super::super::tests::migrated_pool().await;
            let luna = CatalogAgent {
                model_id: ModelId::new("gpt-6-luna").unwrap(),
                display_name: "Luna 6".to_string(),
                model_options: chosen("medium").into_iter().collect(),
                archived: false,
            };
            let hidden = CatalogAgent {
                model_id: ModelId::new("gpt-5.6-terra").unwrap(),
                display_name: "Terra 5.6".to_string(),
                model_options: Vec::new(),
                archived: true,
            };
            let clash = CatalogAgent {
                model_id: ModelId::new("sol-preview").unwrap(),
                display_name: "sol".to_string(),
                model_options: Vec::new(),
                archived: false,
            };
            let agents = vec![luna.clone(), hidden.clone(), clash];

            ensure_provider_agents(&pool, &family("codex", agents.clone()))
                .await
                .unwrap();
            let mut renamed = agents.clone();
            renamed[0].display_name = "Luna 6 on another device".to_string();
            renamed[1].archived = false;
            ensure_provider_agents(&pool, &family("codex", renamed))
                .await
                .unwrap();

            let mut rows: Vec<(String, String, bool)> = sqlx::query_as(
                "SELECT agent.participant_id, participant.display_name,
                        participant.archived_at IS NOT NULL
                 FROM chat_agents agent
                 JOIN chat_participants participant ON participant.id = agent.participant_id
                 WHERE agent.provider_family_id = 'codex'",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            rows.sort();
            let mut expected = Vec::new();
            for (line, name) in [
                ("astra", "Astra"),
                ("sol", "Sol"),
                ("terra", "Terra"),
                ("luna", "Luna"),
            ] {
                expected.push((
                    line_agent_id("codex", line).unwrap(),
                    name.to_string(),
                    false,
                ));
            }
            for (agent, archived) in [(&luna, false), (&hidden, true)] {
                expected.push((
                    catalog_agent_id("codex", agent.model_id.as_str()).unwrap(),
                    agent.display_name.clone(),
                    archived,
                ));
            }
            expected.sort();
            assert_eq!(rows, expected);

            let luna_id =
                ChatParticipantId::new(catalog_agent_id("codex", "gpt-6-luna").unwrap()).unwrap();
            let read =
                super::super::reads::read_agent(&pool, &luna_id, &LocalProviderInstances::new())
                    .await
                    .unwrap();
            assert_eq!(read.agent_kind, ChatAgentKind::Model);
            let policy = read.latest_policy.unwrap();
            assert_eq!(policy.model_id, Some(luna.model_id));
            assert_eq!(policy.model_options, luna.model_options);
        });
    }
}
