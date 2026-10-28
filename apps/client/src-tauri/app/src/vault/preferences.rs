//! Vault preferences, split by where each one belongs.
//!
//! The [registry](registry::REGISTRY) assigns every preference path a scope. Portable values
//! are rows of `vault_preferences`, shared by every device of the vault and written by the owner
//! and by linked replicas, since the preferences sync domain replicates concurrently. Device
//! values live in a file per vault in the app config directory and are written on every device.
//! The device file also holds native-owned entries, such as [`CHAT_DEVICE_ENTRY`], that native
//! commands validate and write and that the registry never routes a patch to.
//!
//! [`PreferencesState`] caches the effective view: the portable rows and device entries
//! composed into the nested object the frontend and native readers expect. Loading the view runs
//! no side effects; vault replacement resumes the native runtimes itself.
//!
//! After a change, [`run_preference_effects`] tells native owners that cache a preference. Native
//! Focus caches its presentation preferences, so a change under `preferences` invalidates it.
//! Music and Contacts read the view when they act, so they need no notice.

mod device;
mod registry;
mod view;

pub(crate) use view::value_at;

use super::config::{VaultConfigPatch, validate_config_patch_path};
use super::{active_replicated_asset_path, active_vault_id, now_utc};
use chrono::SecondsFormat;
use ganbaru_sync::ChangeNotice;
use ganbaru_sync::domains::preferences::PREFERENCE_TOPIC;
use ganbaru_sync::manifest::vault::preferences::DOMAIN_NAME as PREFERENCES_DOMAIN;
use registry::{PATH_SEPARATOR, PreferenceSpec, Scope, Shape, Target};
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Manager, Runtime};
use view::PortableRow;

/// The top-level branch whose values native Focus presentation parses.
const FOCUS_PREFERENCE_BRANCH: &str = "preferences";
/// The device file entry that holds native Chat device settings: provider settings, remembered
/// selections, working folder provider preferences, and panels. It never replicates.
pub(crate) const CHAT_DEVICE_ENTRY: &str = "chat";
/// Device file entries that native commands own. The registry never routes a patch to them and
/// the composed view leaves them out.
const NATIVE_DEVICE_ENTRIES: &[&str] = &[CHAT_DEVICE_ENTRY];

/// Managed cache of the active vault's effective preference view.
#[derive(Default)]
pub(crate) struct PreferencesState {
    view: RwLock<Option<CachedView>>,
    /// Bumped by [`PreferencesState::reset`], so a load that began before a vault replacement
    /// never stores its result.
    generation: AtomicU64,
    /// Serializes loads and patch batches.
    load: tauri::async_runtime::Mutex<()>,
}

struct CachedView {
    vault_id: String,
    generation: u64,
    view: Arc<Value>,
}

impl PreferencesState {
    /// Drops the cached view after the vault was replaced or the active vault changed.
    pub(crate) fn reset(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.invalidate_view();
    }

    /// Drops the cached view so the next read composes it again from storage.
    pub(crate) fn invalidate_view(&self) {
        match self.view.write() {
            Ok(mut view) => *view = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
    }

    fn cached(&self, vault_id: &str) -> Option<Arc<Value>> {
        let generation = self.generation.load(Ordering::Acquire);
        let view = self.view.read().ok()?;
        view.as_ref()
            .filter(|cached| cached.vault_id == vault_id && cached.generation == generation)
            .map(|cached| Arc::clone(&cached.view))
    }

    fn store(&self, vault_id: &str, generation: u64, view: Arc<Value>) {
        if self.generation.load(Ordering::Acquire) != generation {
            return;
        }
        if let Ok(mut cached) = self.view.write() {
            *cached = Some(CachedView {
                vault_id: vault_id.to_owned(),
                generation,
                view,
            });
        }
    }
}

/// Drops the cached view after a vault replacement. Call it before native runtimes resume, so
/// they read the replaced vault's preferences.
pub(crate) fn reset<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<PreferencesState>() {
        state.reset();
    }
}

/// The active vault's effective preference view.
pub(crate) async fn effective_view<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Value>, String> {
    let state = managed_state(app)?;
    let vault_id = active_vault_id(app)?;
    if let Some(view) = state.cached(&vault_id) {
        return Ok(view);
    }
    let _load = state.load.lock().await;
    if let Some(view) = state.cached(&vault_id) {
        return Ok(view);
    }
    load_locked(app, &state, &vault_id).await
}

fn managed_state<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::State<'_, PreferencesState>, String> {
    app.try_state::<PreferencesState>()
        .ok_or_else(|| "vault preferences are unavailable".to_owned())
}

/// Composes the view from storage and caches it. The caller holds the load lock.
async fn load_locked<R: Runtime>(
    app: &AppHandle<R>,
    state: &PreferencesState,
    vault_id: &str,
) -> Result<Arc<Value>, String> {
    let generation = state.generation.load(Ordering::Acquire);
    let pool = crate::db::connect_vault(app).await?;
    let rows = view::load_portable_rows(&pool).await?;
    let device_path = device_path(app, vault_id)?;
    let device = match blocking(move || device::read_device_file(&device_path)).await {
        Ok(device) => device,
        Err(error) => {
            eprintln!("device preferences read as defaults: {error}");
            Map::new()
        }
    };
    let composed = Arc::new(view::compose(&rows, &device));
    state.store(vault_id, generation, Arc::clone(&composed));
    Ok(composed)
}

/// One portable write of a patch batch.
#[derive(Debug, PartialEq)]
enum PortableOp {
    Upsert {
        key: String,
        value: String,
    },
    Delete {
        key: String,
    },
    /// Makes the children of a per-child path exactly `children`.
    ReplaceChildren {
        path: &'static str,
        children: Vec<PortableRow>,
    },
}

/// One device file write of a patch batch.
#[derive(Debug, PartialEq)]
enum DeviceOp {
    Set { key: String, value: Value },
    Remove { key: String },
}

/// A validated patch batch, grouped by store.
#[derive(Debug, Default)]
struct PatchPlan {
    portable: Vec<PortableOp>,
    device: Vec<DeviceOp>,
    /// The dotted path of every patch, in batch order.
    changed_paths: Vec<String>,
}

/// Validates every patch against the registry and groups the batch by store. A patch at an
/// unregistered path, or with a value its rule refuses, fails the batch with the path named.
fn plan_patches(patches: Vec<VaultConfigPatch>) -> Result<PatchPlan, String> {
    let mut plan = PatchPlan::default();
    for patch in patches {
        validate_config_patch_path(&patch.path)?;
        let dotted = patch.path.join(PATH_SEPARATOR);
        let (spec, child) = match registry::route(&patch.path)? {
            Target::Whole(spec) => (spec, None),
            Target::Child(spec, child) => (spec, Some(child.to_owned())),
        };
        let value = (!patch.remove).then_some(&patch.value);
        match (spec.scope, child) {
            (Scope::Device, _) => plan.device.push(match value {
                Some(value) => DeviceOp::Set {
                    key: spec.path.to_owned(),
                    value: checked(spec, &dotted, value).map(|_| value.clone())?,
                },
                None => DeviceOp::Remove {
                    key: spec.path.to_owned(),
                },
            }),
            (Scope::Portable, Some(child)) => {
                plan.portable
                    .push(portable_op(spec.child_key(&child), spec, &dotted, value)?);
            }
            (Scope::Portable, None) => plan.portable.push(match spec.shape {
                Shape::Exact => portable_op(spec.path.to_owned(), spec, &dotted, value)?,
                Shape::PerChild { max_children } => PortableOp::ReplaceChildren {
                    path: spec.path,
                    children: value
                        .map(|value| child_rows(spec, max_children, &dotted, value))
                        .transpose()?
                        .unwrap_or_default(),
                },
            }),
        }
        plan.changed_paths.push(dotted);
    }
    Ok(plan)
}

fn portable_op(
    key: String,
    spec: &PreferenceSpec,
    dotted: &str,
    value: Option<&Value>,
) -> Result<PortableOp, String> {
    Ok(match value {
        Some(value) => PortableOp::Upsert {
            key,
            value: checked(spec, dotted, value)?,
        },
        None => PortableOp::Delete { key },
    })
}

/// The rows of a whole per-child object, each child checked like a single child patch.
fn child_rows(
    spec: &PreferenceSpec,
    max_children: u16,
    dotted: &str,
    value: &Value,
) -> Result<Vec<PortableRow>, String> {
    let children = value
        .as_object()
        .ok_or_else(|| format!("preference {dotted} must be an object"))?;
    if children.len() > usize::from(max_children) {
        return Err(format!(
            "preference {dotted} exceeds {max_children} entries"
        ));
    }
    children
        .iter()
        .map(|(child, value)| {
            registry::validate_child_key(spec, child)?;
            let text = checked(spec, &format!("{dotted}{PATH_SEPARATOR}{child}"), value)?;
            Ok((spec.child_key(child), text))
        })
        .collect()
}

/// The canonical JSON text of `value` when its rule accepts it and it fits a row.
fn checked(spec: &PreferenceSpec, dotted: &str, value: &Value) -> Result<String, String> {
    let refused = || format!("preference {dotted} refuses the value");
    if !spec.rule.accepts(value) {
        return Err(refused());
    }
    serde_json::to_string(value)
        .ok()
        .filter(|text| text.len() <= registry::MAX_VALUE_BYTES)
        .ok_or_else(refused)
}

/// Applies the portable writes of a batch in one transaction. An upsert that stores the value
/// already present keeps its `updated_at`.
async fn apply_portable(
    pool: &SqlitePool,
    ops: &[PortableOp],
    updated_at: &str,
) -> Result<(), String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin preference write: {error}"))?;
    for op in ops {
        match op {
            PortableOp::Upsert { key, value } => {
                upsert(&mut transaction, key, value, updated_at).await?;
            }
            PortableOp::Delete { key } => delete(&mut transaction, key).await?,
            PortableOp::ReplaceChildren { path, children } => {
                let prefix = format!("{path}{PATH_SEPARATOR}");
                let existing = sqlx::query_scalar::<_, String>(
                    "SELECT key FROM vault_preferences WHERE substr(key, 1, length(?1)) = ?1",
                )
                .bind(&prefix)
                .fetch_all(&mut *transaction)
                .await
                .map_err(|error| format!("read preference {path}: {error}"))?;
                for key in existing {
                    if !children.iter().any(|(child, _)| *child == key) {
                        delete(&mut transaction, &key).await?;
                    }
                }
                for (key, value) in children {
                    upsert(&mut transaction, key, value, updated_at).await?;
                }
            }
        }
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit preference write: {error}"))
}

async fn upsert(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: &str,
    updated_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO vault_preferences (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
         WHERE value IS NOT excluded.value",
    )
    .bind(key)
    .bind(value)
    .bind(updated_at)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(|error| format!("write preference {key}: {error}"))
}

async fn delete(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM vault_preferences WHERE key = ?")
        .bind(key)
        .execute(&mut **transaction)
        .await
        .map(|_| ())
        .map_err(|error| format!("delete preference {key}: {error}"))
}

/// Applies the device writes of a batch. Returns whether the entries changed.
fn apply_device_ops(entries: &mut Map<String, Value>, ops: Vec<DeviceOp>) -> bool {
    let mut changed = false;
    for op in ops {
        changed |= match op {
            DeviceOp::Set { key, value } => entries.insert(key, value.clone()) != Some(value),
            DeviceOp::Remove { key } => entries.remove(&key).is_some(),
        };
    }
    changed
}

/// Applies a validated patch batch: portable values in one transaction and device values in one
/// file write. The view is composed again afterwards and the side
/// effects of the changed paths run, also when a later store failed after an earlier one
/// committed.
pub(crate) async fn apply_patches<R: Runtime>(
    app: &AppHandle<R>,
    patches: Vec<VaultConfigPatch>,
) -> Result<(), String> {
    let plan = plan_patches(patches)?;
    let state = managed_state(app)?;
    let vault_id = active_vault_id(app)?;
    let load = state.load.lock().await;
    let changed_paths = plan.changed_paths.clone();
    let result = write_plan(app, &vault_id, plan).await;
    state.invalidate_view();
    if let Err(error) = load_locked(app, &state, &vault_id).await {
        eprintln!("compose preferences after a change: {error}");
    }
    drop(load);
    run_preference_effects(app, &changed_paths);
    result
}

async fn write_plan<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
    plan: PatchPlan,
) -> Result<(), String> {
    if !plan.portable.is_empty() {
        let _write_permit = active_replicated_asset_path(app, PREFERENCES_DOMAIN)?;
        let pool = crate::db::connect_vault(app).await?;
        apply_portable(&pool, &plan.portable, &timestamp()).await?;
    }
    if !plan.device.is_empty() {
        let device_path = device_path(app, vault_id)?;
        let ops = plan.device;
        blocking(move || {
            device::update_device_file(&device_path, |entries| apply_device_ops(entries, ops))
        })
        .await?;
    }
    Ok(())
}

/// The native owners a set of changed preference paths affects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PreferenceEffects {
    /// Native Focus caches its presentation preferences and calendar plan.
    pub focus: bool,
}

/// Classifies changed dotted paths, or row keys, by the native owners they affect.
pub(crate) fn effects_for<'a>(paths: impl IntoIterator<Item = &'a str>) -> PreferenceEffects {
    let focus_prefix = format!("{FOCUS_PREFERENCE_BRANCH}{PATH_SEPARATOR}");
    let mut effects = PreferenceEffects::default();
    for path in paths {
        effects.focus |= path == FOCUS_PREFERENCE_BRANCH || path.starts_with(&focus_prefix);
    }
    effects
}

/// Tells the native owners that cache a preference about changed paths.
pub(crate) fn run_preference_effects<R: Runtime>(app: &AppHandle<R>, paths: &[String]) {
    let effects = effects_for(paths.iter().map(String::as_str));
    if effects.focus {
        crate::pomodoro::invalidate_calendar(app);
    }
}

/// Reacts to the portable preference rows that a sync apply or seal changed, as `notices`
/// report them.
pub(crate) async fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
    on_replicated_keys(app, &replicated_preference_keys(notices.iter())).await;
}

/// The distinct preference keys among sync change notices, in first-seen order.
fn replicated_preference_keys<'a>(notices: impl Iterator<Item = &'a ChangeNotice>) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for notice in notices {
        if notice.topic == PREFERENCE_TOPIC && !keys.contains(&notice.key) {
            keys.push(notice.key.clone());
        }
    }
    keys
}

/// Reacts to portable preference rows that sync applied: drops the cached view so the next read
/// composes the merged rows, and tells the native owners that cache one of `keys`. Holding the
/// load lock keeps a load that read the rows before the apply from caching them afterwards.
async fn on_replicated_keys<R: Runtime>(app: &AppHandle<R>, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    if let Some(state) = app.try_state::<PreferencesState>() {
        let _load = state.load.lock().await;
        state.invalidate_view();
    }
    run_preference_effects(app, keys);
}

/// The native-owned entry `name` of the active vault's device preference file, if present.
pub(crate) fn read_native_device_entry<R: Runtime>(
    app: &AppHandle<R>,
    name: &'static str,
) -> Result<Option<Value>, String> {
    let path = native_device_path(app, name)?;
    Ok(device::read_device_file(&path)?.remove(name))
}

/// Replaces the native-owned entry `name` of the active vault's device preference file with the
/// value `replace` returns for the current entry, under the device file lock, and returns the
/// rest of its result. An error from `replace` leaves the file unchanged.
pub(crate) fn update_native_device_entry<R: Runtime, T, E>(
    app: &AppHandle<R>,
    name: &'static str,
    replace: impl FnOnce(Option<&Value>) -> Result<(Value, T), E>,
    storage_error: impl Fn(String) -> E,
) -> Result<T, E> {
    let path = native_device_path(app, name).map_err(&storage_error)?;
    let mut outcome = None;
    device::update_device_file(&path, |entries| {
        let (changed, result) = replace_native_entry(entries, name, replace);
        outcome = Some(result);
        changed
    })
    .map_err(&storage_error)?;
    outcome.unwrap_or_else(|| Err(storage_error("device preferences were not read".to_owned())))
}

/// Applies `replace` to the entry `name` of `entries`. Returns whether the entry changed.
fn replace_native_entry<T, E>(
    entries: &mut Map<String, Value>,
    name: &str,
    replace: impl FnOnce(Option<&Value>) -> Result<(Value, T), E>,
) -> (bool, Result<T, E>) {
    match replace(entries.get(name)) {
        Ok((value, result)) => {
            let changed = entries.get(name) != Some(&value);
            entries.insert(name.to_owned(), value);
            (changed, Ok(result))
        }
        Err(error) => (false, Err(error)),
    }
}

fn native_device_path<R: Runtime>(
    app: &AppHandle<R>,
    name: &'static str,
) -> Result<PathBuf, String> {
    if !NATIVE_DEVICE_ENTRIES.contains(&name) {
        return Err(format!(
            "device preference entry {name} is not native-owned"
        ));
    }
    device_path(app, &active_vault_id(app)?)
}

fn device_path<R: Runtime>(app: &AppHandle<R>, vault_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("resolve the app config directory: {error}"))?;
    device::device_file_path(&directory, vault_id)
}

fn timestamp() -> String {
    now_utc().to_rfc3339_opts(SecondsFormat::Millis, true)
}

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("preference worker failed: {error}"))?
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    const NOW: &str = "2026-10-22T10:51:41.000Z";
    const LATER: &str = "2026-10-23T00:00:00.000Z";

    #[test]
    fn replicated_preference_keys_are_distinct_and_ignore_other_topics() {
        let notice = |topic: &'static str, key: &str| ChangeNotice {
            topic,
            key: key.to_string(),
        };
        let notices = [
            notice(PREFERENCE_TOPIC, "focus.workMinutes"),
            notice("quick_notes", "note-1"),
            notice(PREFERENCE_TOPIC, "profile.image"),
            notice(PREFERENCE_TOPIC, "focus.workMinutes"),
        ];
        assert_eq!(
            replicated_preference_keys(notices.iter()),
            ["focus.workMinutes", "profile.image"]
        );
        assert!(replicated_preference_keys([notice("quick_notes", "a")].iter()).is_empty());
    }

    /// A migrated in-memory vault database.
    pub(crate) async fn memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    /// Every portable row as key and value text, ordered by key.
    pub(crate) async fn rows(pool: &SqlitePool) -> Vec<PortableRow> {
        view::load_portable_rows(pool).await.unwrap()
    }

    fn patch(dotted: &str, value: Value) -> VaultConfigPatch {
        VaultConfigPatch {
            path: dotted.split('.').map(str::to_owned).collect(),
            remove: false,
            value,
        }
    }

    fn removal(dotted: &str) -> VaultConfigPatch {
        VaultConfigPatch {
            remove: true,
            ..patch(dotted, Value::Null)
        }
    }

    fn row(key: &str, value: &str) -> PortableRow {
        (key.to_owned(), value.to_owned())
    }

    #[test]
    fn preferences_mixed_batch_lands_each_patch_in_its_store() {
        let plan = plan_patches(vec![
            patch("profile.displayName", json!("Ada")),
            patch("theme.activeId", json!("dark")),
            removal("preferences.calendarTimeFormat"),
            removal("projects.activeProjectId"),
            patch("notes.customOrderByProject.p1", json!(["a"])),
            patch("chatSidebar.orderByProject", json!({ "p1": "custom" })),
            removal("notes.sidebarOrderByProject"),
        ])
        .unwrap();
        assert_eq!(
            plan.portable,
            [
                PortableOp::Upsert {
                    key: "profile.displayName".to_owned(),
                    value: "\"Ada\"".to_owned(),
                },
                PortableOp::Delete {
                    key: "preferences.calendarTimeFormat".to_owned(),
                },
                PortableOp::Upsert {
                    key: "notes.customOrderByProject.p1".to_owned(),
                    value: "[\"a\"]".to_owned(),
                },
                PortableOp::ReplaceChildren {
                    path: "chatSidebar.orderByProject",
                    children: vec![row("chatSidebar.orderByProject.p1", "\"custom\"")],
                },
                PortableOp::ReplaceChildren {
                    path: "notes.sidebarOrderByProject",
                    children: Vec::new(),
                },
            ]
        );
        assert_eq!(
            plan.device,
            [
                DeviceOp::Set {
                    key: "theme.activeId".to_owned(),
                    value: json!("dark"),
                },
                DeviceOp::Remove {
                    key: "projects.activeProjectId".to_owned(),
                },
            ]
        );
        assert_eq!(plan.changed_paths.len(), 7);
        assert_eq!(plan.changed_paths[2], "preferences.calendarTimeFormat");
    }

    #[test]
    fn preferences_batches_refuse_unregistered_paths_and_invalid_values() {
        for (refused, value, named) in [
            (
                "preferences.unknownKey",
                json!(true),
                "preferences.unknownKey",
            ),
            ("preferences", json!({}), "preferences"),
            ("distractions.rules", json!([]), "distractions.rules"),
            ("profile.color", json!(32), "profile.color"),
            ("theme.activeId", json!(7), "theme.activeId"),
            (
                "notes.customOrderByProject",
                json!(["not", "an", "object"]),
                "notes.customOrderByProject",
            ),
            (
                "notes.customOrderByProject",
                json!({ "p1": [""] }),
                "notes.customOrderByProject.p1",
            ),
        ] {
            let error = plan_patches(vec![
                patch("profile.useImage", json!(true)),
                patch(refused, value),
            ])
            .unwrap_err();
            assert!(error.contains(named), "{refused}: {error}");
        }
        for (refused, value) in [
            ("chat", json!({})),
            ("chat.providers", json!({})),
            ("chat.panels", json!({})),
            ("chat.behavior.sendKey", json!("shift_enter")),
            ("chat.behavior.terminalScrollbackLines", json!(999)),
            ("chat.behavior.unknownKey", json!(true)),
        ] {
            assert!(
                plan_patches(vec![patch(refused, value)]).is_err(),
                "{refused}"
            );
        }
        let plan = plan_patches(vec![patch("chat.behavior.sendKey", json!("mod_enter"))]).unwrap();
        assert_eq!(
            plan.portable,
            [PortableOp::Upsert {
                key: "chat.behavior.sendKey".to_owned(),
                value: "\"mod_enter\"".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn preferences_portable_writes_replace_children_and_keep_unchanged_timestamps() {
        let pool = memory_pool().await;
        let first = plan_patches(vec![
            patch("profile.displayName", json!("Ada")),
            patch(
                "notes.customOrderByProject",
                json!({ "p1": ["a"], "p2": ["b"] }),
            ),
        ])
        .unwrap();
        apply_portable(&pool, &first.portable, NOW).await.unwrap();

        let second = plan_patches(vec![
            patch("profile.displayName", json!("Ada")),
            patch("notes.customOrderByProject", json!({ "p2": ["c"] })),
        ])
        .unwrap();
        apply_portable(&pool, &second.portable, LATER)
            .await
            .unwrap();
        let stored = sqlx::query_as::<_, (String, String, String)>(
            "SELECT key, value, updated_at FROM vault_preferences ORDER BY key",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let expected = [
            ("notes.customOrderByProject.p2", "[\"c\"]", LATER),
            ("profile.displayName", "\"Ada\"", NOW),
        ]
        .map(|(key, value, at)| (key.to_owned(), value.to_owned(), at.to_owned()));
        assert_eq!(stored, expected);

        let cleared = plan_patches(vec![
            removal("notes.customOrderByProject"),
            removal("profile.displayName"),
        ])
        .unwrap();
        apply_portable(&pool, &cleared.portable, LATER)
            .await
            .unwrap();
        assert!(rows(&pool).await.is_empty());
    }

    #[test]
    fn preferences_device_ops_report_only_real_changes() {
        let mut entries = Map::new();
        let set = || DeviceOp::Set {
            key: "theme.activeId".to_owned(),
            value: json!("dark"),
        };
        assert!(apply_device_ops(&mut entries, vec![set()]));
        assert!(!apply_device_ops(&mut entries, vec![set()]));
        let remove = || DeviceOp::Remove {
            key: "theme.activeId".to_owned(),
        };
        assert!(apply_device_ops(&mut entries, vec![remove()]));
        assert!(!apply_device_ops(&mut entries, vec![remove()]));
    }

    #[test]
    fn preferences_effects_reach_focus_only_for_the_preferences_branch() {
        let focus = PreferenceEffects { focus: true };
        assert_eq!(
            effects_for(["preferences.focusIdleThresholdMinutes"]),
            focus
        );
        assert_eq!(effects_for(["theme.activeId", "preferences"]), focus);
        assert_eq!(
            effects_for([
                "preferencesExtra.key",
                "profile.color",
                "music.review.autoplay"
            ]),
            PreferenceEffects::default()
        );
        assert_eq!(effects_for([]), PreferenceEffects::default());
    }

    #[test]
    fn preferences_native_device_entries_are_never_routed_or_composed() {
        for name in NATIVE_DEVICE_ENTRIES {
            assert!(
                registry::specs(Scope::Device).all(|spec| spec.path != *name),
                "{name}"
            );
            assert!(registry::route(&[(*name).to_owned()]).is_err(), "{name}");
        }
        let mut entries = Map::new();
        entries.insert(
            CHAT_DEVICE_ENTRY.to_owned(),
            json!({ "providers": { "codex": { "enabled": true } } }),
        );
        entries.insert("theme.activeId".to_owned(), json!("dark"));
        assert_eq!(
            view::compose(&[], &entries),
            json!({ "theme": { "activeId": "dark" } })
        );
    }

    #[test]
    fn preferences_device_patches_keep_native_entries() {
        let chat = json!({ "panels": { "sessionList": { "collapsed": true } } });
        let mut entries = Map::new();
        entries.insert(CHAT_DEVICE_ENTRY.to_owned(), chat.clone());
        let plan = plan_patches(vec![
            patch("theme.activeId", json!("dark")),
            removal("projects.activeProjectId"),
        ])
        .unwrap();
        assert!(apply_device_ops(&mut entries, plan.device));
        assert_eq!(entries[CHAT_DEVICE_ENTRY], chat);
    }

    #[test]
    fn preferences_native_entry_replacement_reports_changes_and_keeps_failures_out() {
        let mut entries = Map::new();
        let set = |value: Value| {
            move |current: Option<&Value>| Ok::<_, String>((value, current.cloned()))
        };
        let (changed, previous) =
            replace_native_entry(&mut entries, "chat", set(json!({ "a": 1 })));
        assert!(changed);
        assert_eq!(previous, Ok(None));
        let (changed, previous) =
            replace_native_entry(&mut entries, "chat", set(json!({ "a": 1 })));
        assert!(!changed);
        assert_eq!(previous, Ok(Some(json!({ "a": 1 }))));

        let (changed, result) = replace_native_entry(&mut entries, "chat", |_| {
            Err::<(Value, ()), _>("refused".to_owned())
        });
        assert!(!changed);
        assert_eq!(result, Err("refused".to_owned()));
        assert_eq!(entries["chat"], json!({ "a": 1 }));
    }
}
