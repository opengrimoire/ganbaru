use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{PluginApi, PluginHandle},
};

const PLUGIN_IDENTIFIER: &str = "org.opengrimoire.ganbaruai.mobile.distractions";

#[derive(Debug)]
pub struct MobileDistractions<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Clone for MobileDistractions<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessStatus {
    pub usage_access: bool,
    pub accessibility: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchableApp {
    pub name: String,
    pub package_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingEvent {
    pub id: String,
    pub kind: String,
    pub package_name: String,
    pub display_name: String,
    pub started_at_ms: i64,
    pub elapsed_seconds: i64,
    pub local_date: String,
    pub occurred_at_ms: i64,
    pub reason: Option<String>,
    pub rule_id: Option<String>,
    pub run_id: Option<String>,
    pub phase: Option<String>,
    pub vault_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyRulesRequest<'a> {
    snapshot_json: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AcknowledgeRequest<'a> {
    ids: &'a [String],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountingVaultRequest<'a> {
    vault_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingEventsRequest<'a> {
    vault_id: &'a str,
    usage_only: bool,
}

pub(crate) fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> tauri::Result<MobileDistractions<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MobileDistractionsPlugin")?;
    Ok(MobileDistractions(handle))
}

impl<R: Runtime> MobileDistractions<R> {
    pub fn access_status(&self) -> Result<AccessStatus, String> {
        self.0
            .run_mobile_plugin("accessStatus", ())
            .map_err(|error| format!("read Distractions access status: {error}"))
    }

    pub fn open_usage_access_settings(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin("openUsageAccessSettings", ())
            .map_err(|error| format!("open Usage Access settings: {error}"))
    }

    pub fn open_accessibility_settings(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin("openAccessibilitySettings", ())
            .map_err(|error| format!("open Accessibility settings: {error}"))
    }

    pub fn list_launchable_apps(&self) -> Result<Vec<LaunchableApp>, String> {
        self.0
            .run_mobile_plugin("listLaunchableApps", ())
            .map_err(|error| format!("list launchable Android apps: {error}"))
    }

    pub fn apply_rules(&self, snapshot_json: &str) -> Result<(), String> {
        self.0
            .run_mobile_plugin("applyRules", ApplyRulesRequest { snapshot_json })
            .map_err(|error| format!("apply mobile Distractions rules: {error}"))
    }

    /// Revoke Guardian policy without deleting pending evidence or its vault attribution.
    pub fn invalidate_rules(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin("invalidateRules", ())
            .map_err(|error| format!("invalidate Guardian rules: {error}"))
    }

    /// Read a vault-scoped batch and mark its identities immutable before transport.
    pub fn pending_events(
        &self,
        vault_id: &str,
        usage_only: bool,
    ) -> Result<Vec<PendingEvent>, String> {
        self.0
            .run_mobile_plugin(
                "pendingEvents",
                PendingEventsRequest {
                    vault_id,
                    usage_only,
                },
            )
            .map_err(|error| format!("read mobile Distractions journal: {error}"))
    }

    /// Capture pending usage and local counter baselines in the serialized Guardian process.
    pub fn accounting_snapshot(&self, vault_id: &str) -> Result<String, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            snapshot_json: String,
        }
        let response: Response = self
            .0
            .run_mobile_plugin("accountingSnapshot", AccountingVaultRequest { vault_id })
            .map_err(|error| format!("read Guardian accounting snapshot: {error}"))?;
        if response.snapshot_json.len() > 512 * 1024 {
            return Err("Guardian accounting snapshot exceeds its byte limit".into());
        }
        Ok(response.snapshot_json)
    }

    pub fn acknowledge_events(&self, ids: &[String]) -> Result<(), String> {
        self.0
            .run_mobile_plugin("acknowledgeEvents", AcknowledgeRequest { ids })
            .map_err(|error| format!("acknowledge mobile Distractions journal: {error}"))
    }

    pub fn take_notification_action(&self) -> Result<Option<String>, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            target: Option<String>,
        }
        self.0
            .run_mobile_plugin::<Response>("takeNotificationAction", ())
            .map(|response| response.target)
            .map_err(|error| format!("read mobile Distractions notification action: {error}"))
    }
}

/// Access Ganbaru AI's Android Distractions enforcement bridge.
pub trait MobileDistractionsExt<R: Runtime> {
    fn mobile_distractions(&self) -> &MobileDistractions<R>;
}

impl<R: Runtime, T: Manager<R>> MobileDistractionsExt<R> for T {
    fn mobile_distractions(&self) -> &MobileDistractions<R> {
        self.state::<MobileDistractions<R>>().inner()
    }
}
