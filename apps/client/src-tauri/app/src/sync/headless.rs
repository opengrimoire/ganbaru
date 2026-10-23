//! Android background sync: the JNI entry Android's WorkManager worker calls to run one headless
//! pass, the coordinator records its network service discovery browse hands to the dialer, and
//! the scheduling state the app sends to the background sync plugin.
//!
//! Everything except the JNI symbol and the plugin call is platform-neutral, so desktop tests
//! cover the input bounds, record validation, outcome mapping, and panic containment.

#![allow(
    unsafe_code,
    reason = "Retained unsafe boundary; see docs/data/security/unsafe-rust.md."
)]

use super::service::HeadlessPass;
use super::status::{SyncRole, SyncState, SyncStatusView};
use crate::vault::handoff::pairing::CoordinatorPin;
use crate::vault::handoff::protocol::decode_certificate_fingerprint;
use ganbaru_handoff::discovery::{self, DiscoveredRecord, DiscoveryKey, MAX_ADVERTISED_ADDRESSES};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
#[cfg(target_os = "android")]
use tauri::wry::prelude::jni;

/// The longest app config directory path the worker may pass, in UTF-8 bytes.
pub(crate) const MAX_CONFIG_DIR_BYTES: usize = 4 * 1024;
/// The longest discovered records JSON the worker may pass, in UTF-8 bytes.
pub(crate) const MAX_RECORDS_BYTES: usize = 16 * 1024;
/// The most discovered records one pass accepts.
pub(crate) const MAX_RECORDS: usize = 16;
/// The most passes one background run makes while exchanges stop at their byte bound.
const MAX_PASSES: usize = 4;
/// The longest DNS-SD instance label.
const MAX_INSTANCE_NAME_BYTES: usize = 63;
/// The longest address text a record may carry, which bounds parsing work.
const MAX_ADDRESS_BYTES: usize = 64;
/// The longest TXT version value a record may carry.
const MAX_VERSION_BYTES: usize = 16;

/// What one background pass achieved, as the worker reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PassOutcome {
    /// The pass exchanged with the coordinator.
    Synced,
    /// The pass had nothing to exchange: not linked, paused, waiting for the person key, or
    /// this device is the coordinator.
    Idle,
    /// The coordinator could not be reached.
    Unreachable,
    /// The app service of this process ran the pass.
    Delegated,
    /// The input was invalid, the pass failed, or it panicked.
    Failed,
}

/// The JSON result of one background pass.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PassResult {
    pub(crate) outcome: PassOutcome,
    /// DNS-SD instance names the coordinator may advertise now, for an unreachable coordinator
    /// whose pin carries a discovery key; empty otherwise.
    pub(crate) discovery_names: Vec<String>,
}

impl PassResult {
    fn failed() -> Self {
        Self {
            outcome: PassOutcome::Failed,
            discovery_names: Vec::new(),
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"outcome":"failed","discoveryNames":[]}"#.to_string())
    }
}

/// A string argument read from Java.
#[derive(Debug)]
pub(crate) enum JavaText {
    /// Java passed `null`.
    Null,
    /// The string is longer than its bound, so it was not copied.
    TooLong,
    Text(String),
}

/// One resolved service as the worker reports it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordInput {
    instance_name: String,
    port: u16,
    addresses: Vec<String>,
    #[serde(default)]
    version: Option<String>,
}

impl RecordInput {
    /// The record the dialer may match, or `None` when a field is out of bounds. Addresses that
    /// do not parse, such as IPv6 addresses with a scope suffix, are left out.
    fn validate(self) -> Option<DiscoveredRecord> {
        let name_fits = (1..=MAX_INSTANCE_NAME_BYTES).contains(&self.instance_name.len());
        let version_fits = self
            .version
            .as_ref()
            .is_none_or(|version| version.len() <= MAX_VERSION_BYTES);
        if !name_fits
            || self.instance_name.chars().any(char::is_control)
            || self.port == 0
            || self.addresses.len() > MAX_ADVERTISED_ADDRESSES
            || !version_fits
        {
            return None;
        }
        let addresses = self
            .addresses
            .iter()
            .filter(|address| address.len() <= MAX_ADDRESS_BYTES)
            .filter_map(|address| address.parse::<IpAddr>().ok())
            .collect();
        Some(DiscoveredRecord {
            instance_name: self.instance_name,
            port: self.port,
            addresses,
            version: self.version,
        })
    }
}

/// Parses the worker's records JSON: a bounded array of records, of which those with invalid
/// fields are dropped. Anything else refuses the whole input.
fn parse_records(text: &str) -> Result<Vec<DiscoveredRecord>, String> {
    if text.len() > MAX_RECORDS_BYTES {
        return Err("discovered records are too large".to_string());
    }
    let inputs: Vec<RecordInput> =
        serde_json::from_str(text).map_err(|error| format!("read discovered records: {error}"))?;
    if inputs.len() > MAX_RECORDS {
        return Err("too many discovered records".to_string());
    }
    Ok(inputs
        .into_iter()
        .filter_map(RecordInput::validate)
        .collect())
}

/// Validates the app config directory the worker passes: an absolute path without parent
/// components or NUL bytes, within [`MAX_CONFIG_DIR_BYTES`].
fn parse_config_dir(text: &str) -> Result<PathBuf, String> {
    if text.is_empty() || text.len() > MAX_CONFIG_DIR_BYTES || text.contains('\0') {
        return Err("the app config directory is invalid".to_string());
    }
    let path = PathBuf::from(text);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("the app config directory must be absolute".to_string());
    }
    Ok(path)
}

/// Records a network service discovery browse found, which the coordinator dialer of this
/// process reads while a background pass runs.
#[cfg(target_os = "android")]
static PROVIDED_RECORDS: Mutex<Vec<DiscoveredRecord>> = Mutex::new(Vec::new());

/// Clears the provided records when a pass ends, also when it unwinds.
struct ProvidedRecords<'a>(&'a Mutex<Vec<DiscoveredRecord>>);

impl<'a> ProvidedRecords<'a> {
    fn install(store: &'a Mutex<Vec<DiscoveredRecord>>, records: Vec<DiscoveredRecord>) -> Self {
        if let Ok(mut provided) = store.lock() {
            *provided = records;
        }
        Self(store)
    }
}

impl Drop for ProvidedRecords<'_> {
    fn drop(&mut self) {
        if let Ok(mut provided) = self.0.lock() {
            provided.clear();
        }
    }
}

/// The instance names `pin`'s coordinator may advertise at `unix_seconds`, or none when the pin
/// carries no valid discovery key or fingerprint.
pub(crate) fn discovery_names(pin: &CoordinatorPin, unix_seconds: u64) -> Vec<String> {
    let Some(key) = pin
        .discovery_key
        .as_deref()
        .and_then(|key| DiscoveryKey::decode(key).ok())
    else {
        return Vec::new();
    };
    let Some(fingerprint) = decode_certificate_fingerprint(&pin.certificate_fingerprint) else {
        return Vec::new();
    };
    discovery::expected_instance_names(&key, &[fingerprint], unix_seconds)
}

fn provided_endpoints_in(
    store: &Mutex<Vec<DiscoveredRecord>>,
    pin: &CoordinatorPin,
    unix_seconds: u64,
) -> Vec<SocketAddr> {
    let expected = discovery_names(pin, unix_seconds);
    if expected.is_empty() {
        return Vec::new();
    }
    store
        .lock()
        .map(|records| discovery::match_candidates(&expected, &records))
        .unwrap_or_default()
}

/// Endpoints of `pin`'s coordinator among the records the running background pass was given.
#[cfg(target_os = "android")]
pub(crate) fn provided_endpoints(pin: &CoordinatorPin) -> Vec<SocketAddr> {
    provided_endpoints_in(&PROVIDED_RECORDS, pin, unix_now())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// The outcome the worker acts on. An unreachable coordinator is reported as such even when the
/// app service ran the pass, so the worker browses for it.
fn outcome_of(pass: &Result<HeadlessPass, String>) -> PassOutcome {
    let Ok(pass) = pass else {
        return PassOutcome::Failed;
    };
    match pass.status.state {
        SyncState::Offline => PassOutcome::Unreachable,
        SyncState::Error => PassOutcome::Failed,
        _ if pass.delegated => PassOutcome::Delegated,
        SyncState::Idle if pass.status.role == Some(SyncRole::Client) => PassOutcome::Synced,
        _ => PassOutcome::Idle,
    }
}

/// Runs a background pass for the worker's arguments and returns its JSON result. While an
/// exchange stops at its byte bound, up to [`MAX_PASSES`] passes continue it. The arguments are
/// validated before use, `records` are visible to the dialer only during the passes, and a panic
/// in `pass` or `names` is reported as `failed`.
fn run_pass_with(
    store: &Mutex<Vec<DiscoveredRecord>>,
    config_dir: JavaText,
    records: JavaText,
    mut pass: impl FnMut(&Path) -> Result<HeadlessPass, String>,
    names: impl FnOnce(&Path) -> Result<Vec<String>, String>,
) -> String {
    let run = catch_unwind(AssertUnwindSafe(|| {
        let JavaText::Text(config_dir) = config_dir else {
            return PassResult::failed();
        };
        let Ok(config_dir) = parse_config_dir(&config_dir) else {
            return PassResult::failed();
        };
        let records = match records {
            JavaText::Null => Vec::new(),
            JavaText::TooLong => return PassResult::failed(),
            JavaText::Text(text) => match parse_records(&text) {
                Ok(records) => records,
                Err(_) => return PassResult::failed(),
            },
        };
        let provided = ProvidedRecords::install(store, records);
        let mut result = pass(&config_dir);
        for _ in 1..MAX_PASSES {
            match &result {
                Ok(done) if done.more && !done.delegated => result = pass(&config_dir),
                _ => break,
            }
        }
        drop(provided);
        let outcome = outcome_of(&result);
        if let Err(error) = &result {
            eprintln!("background sync: {error}");
        }
        let discovery_names = if outcome == PassOutcome::Unreachable {
            names(&config_dir).unwrap_or_else(|error| {
                eprintln!("background sync discovery names: {error}");
                Vec::new()
            })
        } else {
            Vec::new()
        };
        PassResult {
            outcome,
            discovery_names,
        }
    }));
    match run {
        Ok(result) => result.to_json(),
        Err(payload) => {
            // A panic payload can itself panic on drop. Keep unwinding out of JNI.
            std::mem::forget(payload);
            PassResult::failed().to_json()
        }
    }
}

/// [`run_pass_with`] against the process record store.
#[cfg(target_os = "android")]
fn run_pass_json(
    config_dir: JavaText,
    records: JavaText,
    pass: impl FnMut(&Path) -> Result<HeadlessPass, String>,
    names: impl FnOnce(&Path) -> Result<Vec<String>, String>,
) -> String {
    run_pass_with(&PROVIDED_RECORDS, config_dir, records, pass, names)
}

/// The discovery names of the coordinator pinned in `config_dir`, read through the pairing
/// manager this process shares with the app runtime.
#[cfg(target_os = "android")]
fn pinned_discovery_names(config_dir: &Path) -> Result<Vec<String>, String> {
    let managers = crate::vault::process_managers::process_managers();
    let device_id = crate::vault::ensure_device_id_in(config_dir)?;
    managers.load_pairing(config_dir, device_id)?;
    Ok(managers
        .pairing()
        .coordinator_pin()?
        .map(|pin| discovery_names(&pin, unix_now()))
        .unwrap_or_default())
}

/// Reads a Java string argument, refusing one longer than `max_bytes` UTF-8 bytes. The length
/// is checked in UTF-16 units before copying, and again in bytes after.
#[cfg(target_os = "android")]
fn read_java_text(
    env: &mut jni::JNIEnv<'_>,
    value: &jni::objects::JString<'_>,
    max_bytes: usize,
) -> JavaText {
    if value.is_null() {
        return JavaText::Null;
    }
    let units = env
        .call_method(value, "length", "()I", &[])
        .and_then(|length| length.i());
    match units {
        Ok(units) if usize::try_from(units).is_ok_and(|units| units <= max_bytes) => {}
        _ => return JavaText::TooLong,
    }
    match env.get_string(value) {
        Ok(text) => {
            let text = String::from(text);
            if text.len() > max_bytes {
                JavaText::TooLong
            } else {
                JavaText::Text(text)
            }
        }
        Err(_) => JavaText::TooLong,
    }
}

/// JNI entry of Kotlin's static `SyncNative.runPass(String, String?): String?`. It runs one
/// headless pass for the app config directory and returns the JSON [`PassResult`], or `null`
/// when the result string cannot be created.
#[cfg(target_os = "android")]
// SAFETY: The unique symbol matches Kotlin's static runPass(String, String): String in
// org.opengrimoire.ganbaru.mobile.sync.SyncNative per the JNI naming rules. The parameters and
// result use the jni crate's transparent wrappers for JNIEnv*, jclass, jstring, and jstring, so
// the ABI matches the JNI signature. Local references are used only during this call. Every
// panic, including a panic-on-drop payload, is contained before returning to Java.
#[unsafe(export_name = "Java_org_opengrimoire_ganbaru_mobile_sync_SyncNative_runPass")]
pub extern "system" fn native_sync_run_pass<'local>(
    mut env: jni::JNIEnv<'local>,
    _class: jni::objects::JClass<'local>,
    config_dir: jni::objects::JString<'local>,
    records: jni::objects::JString<'local>,
) -> jni::sys::jstring {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let config_dir = read_java_text(&mut env, &config_dir, MAX_CONFIG_DIR_BYTES);
        let records = read_java_text(&mut env, &records, MAX_RECORDS_BYTES);
        let json = run_pass_json(
            config_dir,
            records,
            |config_dir| {
                tauri::async_runtime::block_on(super::service::headless_pass(
                    config_dir.to_path_buf(),
                ))
            },
            pinned_discovery_names,
        );
        env.new_string(json)
            .map(jni::objects::JString::into_raw)
            .unwrap_or(std::ptr::null_mut())
    }));
    match result {
        Ok(json) => json,
        Err(payload) => {
            // A panic payload can itself panic on drop. Keep unwinding out of JNI.
            std::mem::forget(payload);
            std::ptr::null_mut()
        }
    }
}

/// The scheduling state the background sync plugin needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackgroundConfig {
    /// This device syncs the active vault with a coordinator, so background work is scheduled.
    pub(crate) linked: bool,
    /// Local changes wait to be exchanged.
    pub(crate) pending: bool,
    pub(crate) config_dir: PathBuf,
}

impl BackgroundConfig {
    /// The state for a device whose linking is `linked` and whose status is `view`.
    pub(crate) fn of(linked: bool, view: &SyncStatusView, config_dir: PathBuf) -> Self {
        Self {
            linked,
            pending: view.pending_local_changes > 0,
            config_dir,
        }
    }
}

/// The latest state the app wants the plugin to hold.
#[cfg(target_os = "android")]
static DESIRED_CONFIG: Mutex<Option<BackgroundConfig>> = Mutex::new(None);
/// The state the plugin last accepted. Its lock also orders deliveries.
#[cfg(target_os = "android")]
static DELIVERED_CONFIG: Mutex<Option<BackgroundConfig>> = Mutex::new(None);

/// Stores `next` and reports whether it differs from what `desired` held.
fn take_change(desired: &Mutex<Option<BackgroundConfig>>, next: BackgroundConfig) -> bool {
    let Ok(mut desired) = desired.lock() else {
        return false;
    };
    if desired.as_ref() == Some(&next) {
        return false;
    }
    *desired = Some(next);
    true
}

/// Sends the latest desired state when the plugin does not hold it yet, so deliveries that run
/// out of order never leave an older state in place. A failed send clears the desired state, so
/// the next status publication retries.
fn deliver_latest(
    desired: &Mutex<Option<BackgroundConfig>>,
    delivered: &Mutex<Option<BackgroundConfig>>,
    send: impl FnOnce(&BackgroundConfig) -> Result<(), String>,
) -> Result<(), String> {
    let mut delivered = delivered
        .lock()
        .map_err(|_| "background sync delivery state is unavailable".to_string())?;
    let latest = desired
        .lock()
        .map_err(|_| "background sync desired state is unavailable".to_string())?
        .clone();
    let Some(latest) = latest else {
        return Ok(());
    };
    if delivered.as_ref() == Some(&latest) {
        return Ok(());
    }
    match send(&latest) {
        Ok(()) => {
            *delivered = Some(latest);
            Ok(())
        }
        Err(error) => {
            if let Ok(mut desired) = desired.lock()
                && desired.as_ref() == Some(&latest)
            {
                *desired = None;
            }
            Err(error)
        }
    }
}

/// Tells the background sync plugin the state at startup, before the service publishes a status,
/// so a device that is no longer linked cancels work scheduled by an earlier process.
#[cfg(target_os = "android")]
pub(crate) fn configure_background_at_start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    configure_background(app, &SyncStatusView::default());
}

/// Tells the background sync plugin the linked state, whether changes are pending, and the app
/// config directory, when any of them changed since the last call. The plugin call blocks on the
/// Android main thread, so it runs on the blocking pool.
#[cfg(target_os = "android")]
pub(crate) fn configure_background<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    view: &SyncStatusView,
) {
    use super::host::SyncHost;
    use ganbaru_mobile_sync::MobileSyncExt;
    let config = match (app.sync_enabled(), app.config_dir()) {
        (Ok(linked), Ok(config_dir)) => BackgroundConfig::of(linked, view, config_dir),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("background sync configuration: {error}");
            return;
        }
    };
    if !take_change(&DESIRED_CONFIG, config) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let delivered = deliver_latest(&DESIRED_CONFIG, &DELIVERED_CONFIG, |config| {
            app.mobile_sync()
                .ok_or_else(|| "the background sync plugin is not registered".to_string())?
                .configure(config.linked, config.pending, &config.config_dir)
        });
        if let Err(error) = delivered {
            eprintln!("{error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::status::SyncStatusView;

    fn store() -> Mutex<Vec<DiscoveredRecord>> {
        Mutex::new(Vec::new())
    }

    fn pass(state: SyncState, role: Option<SyncRole>, delegated: bool) -> HeadlessPass {
        HeadlessPass {
            status: SyncStatusView {
                state,
                role,
                ..SyncStatusView::default()
            },
            delegated,
            more: false,
        }
    }

    fn config_dir() -> JavaText {
        JavaText::Text("/data/user/0/org.opengrimoire.ganbaru/files".to_string())
    }

    fn pin() -> CoordinatorPin {
        CoordinatorPin {
            device_id: "coordinator".to_string(),
            device_label: None,
            endpoint: "192.168.1.20:43821".to_string(),
            vault_id: "vault".to_string(),
            generation: 1,
            certificate_fingerprint: "ab".repeat(32),
            last_endpoint: None,
            reported_endpoints: Vec::new(),
            // The unpadded base64url form of 32 bytes of 7.
            discovery_key: Some(format!("{}Bwc", "BwcH".repeat(10))),
        }
    }

    fn run(records: JavaText, result: Result<HeadlessPass, String>) -> serde_json::Value {
        let mut result = Some(result);
        let json = run_pass_with(
            &store(),
            config_dir(),
            records,
            |_| result.take().expect("one pass"),
            |_| Ok(vec!["name".to_string()]),
        );
        serde_json::from_str(&json).expect("result JSON")
    }

    fn outcome(records: JavaText, result: Result<HeadlessPass, String>) -> String {
        run(records, result)["outcome"]
            .as_str()
            .expect("outcome")
            .to_string()
    }

    #[test]
    fn pass_states_map_to_the_outcomes_the_worker_acts_on() {
        let cases = [
            (
                pass(SyncState::Idle, Some(SyncRole::Client), false),
                "synced",
            ),
            (pass(SyncState::Idle, Some(SyncRole::Hub), false), "idle"),
            (pass(SyncState::Off, None, false), "idle"),
            (
                pass(SyncState::Paused, Some(SyncRole::Client), false),
                "idle",
            ),
            (
                pass(SyncState::WaitingForIdentity, Some(SyncRole::Client), false),
                "idle",
            ),
            (
                pass(SyncState::Offline, Some(SyncRole::Client), false),
                "unreachable",
            ),
            (
                pass(SyncState::Error, Some(SyncRole::Client), false),
                "failed",
            ),
            (
                pass(SyncState::Idle, Some(SyncRole::Client), true),
                "delegated",
            ),
            (
                pass(SyncState::Offline, Some(SyncRole::Client), true),
                "unreachable",
            ),
            (
                pass(SyncState::Error, Some(SyncRole::Client), true),
                "failed",
            ),
        ];
        for (result, expected) in cases {
            assert_eq!(outcome(JavaText::Null, Ok(result)), expected);
        }
        assert_eq!(
            outcome(JavaText::Null, Err("vault is busy".to_string())),
            "failed"
        );
    }

    #[test]
    fn only_an_unreachable_coordinator_carries_discovery_names() {
        let unreachable = run(
            JavaText::Null,
            Ok(pass(SyncState::Offline, Some(SyncRole::Client), false)),
        );
        assert_eq!(
            unreachable,
            serde_json::json!({ "outcome": "unreachable", "discoveryNames": ["name"] })
        );
        let synced = run(
            JavaText::Null,
            Ok(pass(SyncState::Idle, Some(SyncRole::Client), false)),
        );
        assert_eq!(
            synced,
            serde_json::json!({ "outcome": "synced", "discoveryNames": [] })
        );
        let unnamed = run_pass_with(
            &store(),
            config_dir(),
            JavaText::Null,
            |_| Ok(pass(SyncState::Offline, Some(SyncRole::Client), false)),
            |_| Err("pairing state is unreadable".to_string()),
        );
        assert_eq!(unnamed, r#"{"outcome":"unreachable","discoveryNames":[]}"#);
    }

    #[test]
    fn passes_continue_a_bounded_exchange_up_to_the_pass_limit() {
        let continuing = |delegated| HeadlessPass {
            more: true,
            ..pass(SyncState::Idle, Some(SyncRole::Client), delegated)
        };
        let mut calls = 0;
        let json = run_pass_with(
            &store(),
            config_dir(),
            JavaText::Null,
            |_| {
                calls += 1;
                Ok(continuing(false))
            },
            |_| Ok(Vec::new()),
        );
        assert_eq!(calls, MAX_PASSES);
        assert_eq!(json, r#"{"outcome":"synced","discoveryNames":[]}"#);

        let mut calls = 0;
        run_pass_with(
            &store(),
            config_dir(),
            JavaText::Null,
            |_| {
                calls += 1;
                Ok(if calls == 2 {
                    pass(SyncState::Idle, Some(SyncRole::Client), false)
                } else {
                    continuing(false)
                })
            },
            |_| Ok(Vec::new()),
        );
        assert_eq!(calls, 2);

        let mut calls = 0;
        let json = run_pass_with(
            &store(),
            config_dir(),
            JavaText::Null,
            |_| {
                calls += 1;
                Ok(continuing(true))
            },
            |_| Ok(Vec::new()),
        );
        assert_eq!(calls, 1);
        assert_eq!(json, r#"{"outcome":"delegated","discoveryNames":[]}"#);
    }

    #[test]
    fn invalid_config_directories_fail_without_running_a_pass() {
        let too_long = format!("/{}", "a".repeat(MAX_CONFIG_DIR_BYTES));
        for config_dir in [
            JavaText::Null,
            JavaText::TooLong,
            JavaText::Text(String::new()),
            JavaText::Text("relative/config".to_string()),
            JavaText::Text("/data/../etc".to_string()),
            JavaText::Text("/data/\0config".to_string()),
            JavaText::Text(too_long),
        ] {
            let json = run_pass_with(
                &store(),
                config_dir,
                JavaText::Null,
                |_| panic!("an invalid config directory must not run a pass"),
                |_| Ok(Vec::new()),
            );
            assert_eq!(json, r#"{"outcome":"failed","discoveryNames":[]}"#);
        }
        let longest = format!("/{}", "a".repeat(MAX_CONFIG_DIR_BYTES - 1));
        assert!(parse_config_dir(&longest).is_ok());
    }

    #[test]
    fn records_beyond_their_bounds_or_shape_fail_without_running_a_pass() {
        let record = r#"{"instanceName":"tag","port":43821,"addresses":["192.168.1.20"]}"#;
        let too_many = format!("[{}]", vec![record; MAX_RECORDS + 1].join(","));
        let too_large = format!(
            r#"[{{"instanceName":"tag","port":1,"addresses":[],"version":"{}"}}]"#,
            "v".repeat(MAX_RECORDS_BYTES)
        );
        for records in [
            JavaText::TooLong,
            JavaText::Text(too_many),
            JavaText::Text(too_large),
            JavaText::Text("{}".to_string()),
            JavaText::Text("[1]".to_string()),
            JavaText::Text(r#"[{"instanceName":"tag","port":70000,"addresses":[]}]"#.to_string()),
            JavaText::Text(
                r#"[{"instanceName":"tag","port":1,"addresses":[],"extra":true}]"#.to_string(),
            ),
        ] {
            let json = run_pass_with(
                &store(),
                config_dir(),
                records,
                |_| panic!("invalid records must not run a pass"),
                |_| Ok(Vec::new()),
            );
            assert_eq!(json, r#"{"outcome":"failed","discoveryNames":[]}"#);
        }
        let most = format!("[{}]", vec![record; MAX_RECORDS].join(","));
        assert_eq!(parse_records(&most).expect("records").len(), MAX_RECORDS);
    }

    #[test]
    fn records_with_invalid_fields_are_dropped_and_unparsable_addresses_skipped() {
        let records = parse_records(
            r#"[
                {"instanceName":"","port":1,"addresses":["192.168.1.20"]},
                {"instanceName":"tag","port":0,"addresses":["192.168.1.20"]},
                {"instanceName":"tag\u0007","port":1,"addresses":["192.168.1.20"]},
                {"instanceName":"tag","port":1,"addresses":["192.168.1.20"],"version":"vvvvvvvvvvvvvvvvv"},
                {"instanceName":"tag","port":1,"addresses":["1.1.1.1","1.1.1.2","1.1.1.3","1.1.1.4","1.1.1.5","1.1.1.6","1.1.1.7","1.1.1.8","1.1.1.9"]},
                {"instanceName":"kept","port":43821,"addresses":["fe80::1%wlan0","not an address","10.0.0.4"],"version":"1"}
            ]"#,
        )
        .expect("records");
        assert_eq!(
            records,
            vec![DiscoveredRecord {
                instance_name: "kept".to_string(),
                port: 43_821,
                addresses: vec!["10.0.0.4".parse().expect("address")],
                version: Some("1".to_string()),
            }]
        );
        let long_name = format!(
            r#"[{{"instanceName":"{}","port":1,"addresses":[]}}]"#,
            "a".repeat(MAX_INSTANCE_NAME_BYTES + 1)
        );
        assert!(parse_records(&long_name).expect("records").is_empty());
    }

    #[test]
    fn provided_records_reach_the_dialer_only_during_the_pass() {
        let store = store();
        let pin = pin();
        let now = unix_now();
        let name = discovery_names(&pin, now)[1].clone();
        let records = format!(
            r#"[
                {{"instanceName":"{name}","port":43821,"addresses":["192.168.1.30"],"version":"1"}},
                {{"instanceName":"stranger","port":43821,"addresses":["192.168.1.40"],"version":"1"}}
            ]"#
        );
        let mut seen = Vec::new();
        run_pass_with(
            &store,
            config_dir(),
            JavaText::Text(records),
            |_| {
                seen = provided_endpoints_in(&store, &pin, now);
                Ok(pass(SyncState::Idle, Some(SyncRole::Client), false))
            },
            |_| Ok(Vec::new()),
        );
        assert_eq!(seen, vec!["192.168.1.30:43821".parse().expect("endpoint")]);
        assert!(store.lock().expect("store").is_empty());
        assert!(provided_endpoints_in(&store, &pin, now).is_empty());
    }

    #[test]
    fn a_pin_without_a_discovery_key_matches_no_provided_record() {
        let store = store();
        let mut pin = pin();
        let name = discovery_names(&pin, unix_now())[1].clone();
        *store.lock().expect("store") = vec![DiscoveredRecord {
            instance_name: name,
            port: 43_821,
            addresses: vec!["192.168.1.30".parse().expect("address")],
            version: Some("1".to_string()),
        }];
        pin.discovery_key = None;
        assert!(discovery_names(&pin, unix_now()).is_empty());
        assert!(provided_endpoints_in(&store, &pin, unix_now()).is_empty());
    }

    #[test]
    fn discovery_names_are_the_tags_the_coordinator_advertises() {
        let pin = pin();
        let hour_start = 478_000 * 3600;
        let names = discovery_names(&pin, hour_start);
        let fingerprint =
            decode_certificate_fingerprint(&pin.certificate_fingerprint).expect("fingerprint");
        let advertised = discovery::advertisement(
            &DiscoveryKey::from_bytes([7; 32]),
            &fingerprint,
            "0011223344556677.local.",
            43_821,
            &[],
            hour_start,
        );
        assert_eq!(names.len(), 3);
        assert_eq!(names[1], advertised.instance_name);
    }

    #[test]
    fn panics_and_panicking_payload_destructors_report_failed() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("payload destructor must not run across JNI");
            }
        }
        let failed = r#"{"outcome":"failed","discoveryNames":[]}"#;
        let store = store();
        let json = run_pass_with(
            &store,
            config_dir(),
            JavaText::Text("[]".to_string()),
            |_| panic!("pass failed"),
            |_| Ok(Vec::new()),
        );
        assert_eq!(json, failed);
        let json = run_pass_with(
            &store,
            config_dir(),
            JavaText::Null,
            |_| std::panic::panic_any(PanicOnDrop),
            |_| Ok(Vec::new()),
        );
        assert_eq!(json, failed);
        let json = run_pass_with(
            &store,
            config_dir(),
            JavaText::Null,
            |_| Ok(pass(SyncState::Offline, Some(SyncRole::Client), false)),
            |_| panic!("names failed"),
        );
        assert_eq!(json, failed);
        assert!(store.lock().expect("store").is_empty());
    }

    #[test]
    fn deliveries_send_the_latest_state_once_and_retry_after_a_failure() {
        let desired = Mutex::new(None);
        let delivered = Mutex::new(None);
        let view = SyncStatusView::default();
        let linked = BackgroundConfig::of(true, &view, PathBuf::from("/config"));
        let unlinked = BackgroundConfig::of(false, &view, PathBuf::from("/config"));
        let mut sent = Vec::new();

        assert_eq!(
            deliver_latest(&desired, &delivered, |_| panic!("nothing to send")),
            Ok(())
        );
        assert!(take_change(&desired, linked.clone()));
        assert!(take_change(&desired, unlinked.clone()));
        deliver_latest(&desired, &delivered, |config| {
            sent.push(config.clone());
            Ok(())
        })
        .expect("first delivery");
        deliver_latest(&desired, &delivered, |_| panic!("already delivered"))
            .expect("second delivery");
        assert_eq!(sent, vec![unlinked.clone()]);

        assert!(take_change(&desired, linked.clone()));
        let failed = deliver_latest(&desired, &delivered, |_| Err("plugin failed".to_string()));
        assert_eq!(failed, Err("plugin failed".to_string()));
        assert_eq!(*delivered.lock().expect("delivered"), Some(unlinked));
        assert!(take_change(&desired, linked.clone()));
        deliver_latest(&desired, &delivered, |config| {
            sent.push(config.clone());
            Ok(())
        })
        .expect("retried delivery");
        assert_eq!(sent.last(), Some(&linked));
    }

    #[test]
    fn background_configuration_is_sent_only_when_it_changes() {
        let sent = Mutex::new(None);
        let mut view = SyncStatusView::default();
        let dir = PathBuf::from("/config");
        assert!(take_change(
            &sent,
            BackgroundConfig::of(true, &view, dir.clone())
        ));
        view.state = SyncState::Syncing;
        assert!(!take_change(
            &sent,
            BackgroundConfig::of(true, &view, dir.clone())
        ));
        view.pending_local_changes = 3;
        assert!(take_change(
            &sent,
            BackgroundConfig::of(true, &view, dir.clone())
        ));
        view.pending_local_changes = 5;
        assert!(!take_change(
            &sent,
            BackgroundConfig::of(true, &view, dir.clone())
        ));
        assert!(take_change(&sent, BackgroundConfig::of(false, &view, dir)));
        assert!(take_change(
            &sent,
            BackgroundConfig::of(false, &view, PathBuf::from("/other"))
        ));
    }
}
