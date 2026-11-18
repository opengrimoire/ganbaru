//! The agent of each provider family, and what each family's provider does on this device.
//!
//! An agent is a provider: every device that detects a provider family creates its agent under
//! an id derived from the family, so devices converge on one agent per family. Which provider
//! installation runs an agent is a device-local choice made here.

use super::reads::LocalProviderInstances;
use super::{now_timestamp, persistence_error};
use crate::chat::device_state::read_active_device_scope;
use ganbaru_chat::conversion::provider_agent_id;
use ganbaru_chat_contracts::models::*;
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use sqlx::SqlitePool;
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
        Ok(Self { by_family })
    }

    /// Providers of this device built from `entries`, for tests that need no Chat settings.
    #[cfg(test)]
    pub(crate) fn from_entries(
        entries: impl IntoIterator<Item = (String, DeviceProvider)>,
    ) -> Self {
        Self {
            by_family: entries.into_iter().collect(),
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

    /// The configured families with the display name their provider gives them.
    fn families(&self) -> Vec<(String, String)> {
        self.by_family
            .keys()
            .map(|family| {
                let display_name = ProviderFamilyId::new(family.clone())
                    .map(|id| ProviderDriverRegistry.metadata(&id).display_name)
                    .unwrap_or_else(|_| family.clone());
                (family.clone(), display_name)
            })
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

/// Creates the agent of each provider family configured on this device that the vault lacks.
pub(crate) async fn ensure_device_provider_agents(
    pool: &SqlitePool,
    providers: &DeviceProviders,
) -> ChatResult<()> {
    ensure_provider_agents(pool, &providers.families()).await
}

/// Creates the agent of each `(family, display_name)` that the vault lacks, under the id
/// derived from the family. An existing agent, archived or not, is left as it is, so running it
/// again changes nothing and two devices that detect one family write the same agent.
pub(crate) async fn ensure_provider_agents(
    pool: &SqlitePool,
    families: &[(String, String)],
) -> ChatResult<()> {
    if families.is_empty() {
        return Ok(());
    }
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    for (family, display_name) in families {
        let agent_id = provider_agent_id(family).map_err(|_| {
            ChatError::validation("providerFamilyId", "The provider family is not valid")
        })?;
        let display_name = display_name.trim();
        let display_name = if display_name.is_empty() {
            family.as_str()
        } else {
            display_name
        };
        sqlx::query(
            "INSERT INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES (?, 'ai_agent', ?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(&agent_id)
        .bind(display_name)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
             SELECT ?1, ?2, ?3, ?3
             WHERE EXISTS (
                 SELECT 1 FROM chat_participants
                 WHERE id = ?1 AND participant_kind = 'ai_agent'
             )
             ON CONFLICT DO NOTHING",
        )
        .bind(&agent_id)
        .bind(family)
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)
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

    #[test]
    fn ensuring_a_family_again_converges_on_one_agent_with_its_derived_id() {
        tauri::async_runtime::block_on(async {
            let pool = super::super::tests::migrated_pool().await;
            let family = |name: &str| vec![("codex".to_string(), name.to_string())];

            ensure_provider_agents(&pool, &family("Codex"))
                .await
                .unwrap();
            ensure_provider_agents(&pool, &family("Codex on another device"))
                .await
                .unwrap();
            ensure_provider_agents(&pool, &family("  ")).await.unwrap();

            let agents: Vec<(String, String)> = sqlx::query_as(
                "SELECT agent.participant_id, participant.display_name
                 FROM chat_agents agent
                 JOIN chat_participants participant ON participant.id = agent.participant_id
                 WHERE agent.provider_family_id = 'codex'",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                agents,
                vec![(provider_agent_id("codex").unwrap(), "Codex".to_string())]
            );
        });
    }
}
