//! Spawn-path tests that run real provider drivers against the fake replay CLI.

use ganbaru_chat_contracts::events::CanonicalRuntimeEvent;
use ganbaru_chat_contracts::models::*;
use ganbaru_chat_providers::{
    DriverCancellation, DriverFuture, DriverOperationContext, ProviderDriver,
    ProviderDriverFactory, ProviderDriverRegistry, ProviderEventSink,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const FAKE_CLI: &str = env!("CARGO_BIN_EXE_ganbaru-provider-replay");
const OPERATION_DEADLINE: Duration = Duration::from_secs(30);

/// A workspace and provider home for one test, removed on drop.
struct SpawnDirectories {
    root: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
}

impl SpawnDirectories {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ganbaru-replay-spawn-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = root.join("workspace");
        let home = root.join("home");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Self {
            workspace: workspace.canonicalize().unwrap(),
            home: home.canonicalize().unwrap(),
            root,
        }
    }

    /// Where the fake CLI records why it stopped.
    fn failure_log(&self) -> PathBuf {
        self.root.join("failures.log")
    }

    /// Every failure the fake CLI recorded, for assertion messages.
    fn failures(&self) -> String {
        std::fs::read_to_string(self.failure_log()).unwrap_or_default()
    }
}

impl Drop for SpawnDirectories {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct DiscardingSink;

impl ProviderEventSink for DiscardingSink {
    fn emit<'a>(&'a self, _event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn transcript(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("replay")
        .join(name)
}

fn claude_configuration(
    directories: &SpawnDirectories,
    transcript_name: &str,
) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "claude-replay",
        "familyId": "claude",
        "label": "Claude",
        "enabled": true,
        "executable": FAKE_CLI,
        "providerHome": directories.home,
        "launchArguments": [],
        "environment": {
            "GANBARU_REPLAY_TRANSCRIPT": transcript(transcript_name),
            "GANBARU_REPLAY_WORKSPACE": directories.workspace,
            "GANBARU_REPLAY_HOME": directories.home,
            "GANBARU_REPLAY_FAILURE_LOG": directories.failure_log(),
        },
        "credentialReferences": {},
        "providerConfig": {
            "schemaVersion": 1,
            "value": { "allowCustomModels": false, "customModelIds": [], "customModelLabels": {} }
        }
    }))
    .unwrap()
}

fn driver(configuration: ProviderInstanceConfig) -> Box<dyn ProviderDriver> {
    ProviderDriverRegistry.create_driver(configuration).unwrap()
}

fn context() -> DriverOperationContext {
    DriverOperationContext {
        operation_id: "replay-spawn".to_string(),
        deadline: Instant::now() + OPERATION_DEADLINE,
        cancellation: DriverCancellation::default(),
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn start_request(directories: &SpawnDirectories) -> StartSessionRequest {
    StartSessionRequest {
        thread_id: ChatThreadId::new("replay-thread".to_string()).unwrap(),
        workspace: VerifiedWorkspaceContext {
            working_folder_id: ProjectWorkingFolderId::new("replay-folder".to_string()).unwrap(),
            canonical_path: directories.workspace.to_string_lossy().into_owned(),
            repository_kind: RepositoryKind::None,
            repository_identity: None,
            project_configuration: ProjectConfigurationAccess::Restricted,
        },
        provider_instance_id: ProviderInstanceId::new("claude-replay".to_string()).unwrap(),
        modes: TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
        model_id: None,
        model_options: Vec::new(),
        developer_instructions: None,
    }
}

#[test]
fn the_claude_probe_parses_the_version_the_cli_prints() {
    let directories = SpawnDirectories::new();
    let mut driver = driver(claude_configuration(&directories, "claude-probe.jsonl"));
    let probe = block_on(driver.probe(&context())).unwrap();
    assert_eq!(
        probe.version.as_deref(),
        Some("2.1.322"),
        "{probe:?}\n{}",
        directories.failures()
    );
    assert_eq!(
        probe.negotiated_protocol_version.as_deref(),
        Some("stream-json")
    );
    assert_eq!(
        probe.state,
        ProbeState::AuthenticationRequired,
        "the transcript reports no account"
    );
}

#[test]
fn a_claude_cli_that_exits_before_its_first_frame_probes_as_unavailable() {
    let directories = SpawnDirectories::new();
    let mut driver = driver(claude_configuration(
        &directories,
        "claude-exit-before-first-frame.jsonl",
    ));
    let probe = block_on(driver.probe(&context())).unwrap();
    assert_eq!(probe.state, ProbeState::TransportUnavailable, "{probe:?}");
    assert_eq!(probe.version, None);
    assert_eq!(
        directories.failures(),
        "",
        "the CLI exited as the transcript says, not on a mismatch"
    );
}

#[test]
fn an_organizational_claude_session_launches_with_the_locked_down_arguments() {
    let directories = SpawnDirectories::new();
    let mut configuration =
        claude_configuration(&directories, "claude-organizational-launch.jsonl");
    configuration.internal_mcp = Some(ProviderInternalMcpConfig {
        name: "ganbaru-chat".to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "replay-token".to_string(),
        organizational_authority: true,
        pre_approved_tools: Vec::new(),
        organizational_scope: Some(
            ganbaru_chat_contracts::models::ProviderOrganizationalScope::Conversation,
        ),
    });
    let mut driver = driver(configuration);
    let snapshot = block_on(async {
        let context = context();
        let snapshot = driver
            .start_session(
                start_request(&directories),
                Arc::new(DiscardingSink),
                &context,
            )
            .await?;
        driver
            .stop_session(
                StopSessionRequest {
                    session_id: snapshot.session_id.clone(),
                    force: false,
                },
                &context,
            )
            .await?;
        Ok::<_, ChatError>(snapshot)
    });
    assert!(snapshot.is_ok(), "{snapshot:?}\n{}", directories.failures());
}

#[test]
fn record_mode_relays_the_exchange_and_writes_it_line_by_line() {
    let directories = SpawnDirectories::new();
    let recording = directories.root.join("recording.jsonl");
    let mut configuration = claude_configuration(&directories, "claude-probe.jsonl");
    for (name, value) in [
        ("GANBARU_REPLAY_RECORD_TARGET", FAKE_CLI.to_string()),
        (
            "GANBARU_REPLAY_RECORD_OUTPUT",
            recording.to_string_lossy().into_owned(),
        ),
        ("GANBARU_REPLAY_RECORD_PROVIDER", "claude".to_string()),
        ("GANBARU_REPLAY_RECORD_SCENARIO", "probe".to_string()),
        ("GANBARU_REPLAY_RECORD_PROTOCOL", "stream-json".to_string()),
    ] {
        configuration.environment.insert(name.to_string(), value);
    }
    let mut driver = driver(configuration);
    let probe = block_on(driver.probe(&context())).unwrap();
    assert_eq!(
        probe.version.as_deref(),
        Some("2.1.322"),
        "{probe:?}\n{}",
        directories.failures()
    );
    assert_eq!(probe.state, ProbeState::AuthenticationRequired, "{probe:?}");
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&recording)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "transcript_start");
    assert_eq!(lines[0]["cli_version"], "2.1.322");
    assert_eq!(lines[0]["scenario"], "probe");
    assert!(
        lines[0]["launch_arguments"]
            .as_array()
            .is_some_and(|arguments| !arguments.is_empty())
    );
    assert_eq!(lines[1]["type"], "expect_outbound");
    assert_eq!(lines[1]["frame"]["request"]["subtype"], "initialize");
    assert_eq!(lines[2]["type"], "emit_inbound");
    assert_eq!(lines[2]["frame"]["type"], "control_response");
}
