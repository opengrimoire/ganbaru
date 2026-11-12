//! Session instructions in the Claude system prompt.

use super::init_gate::{GateHarness, SERVER_NAME, internal_status, turn_request, written_subtypes};
use super::modes;
use crate::claude::instructions::*;
use crate::claude::protocol::*;
use crate::models::*;
use std::path::Path;

const SESSION_UUID: &str = "11111111-1111-4111-8111-111111111111";
const SERVER_URL: &str = "http://127.0.0.1:41234/mcp";

fn options(
    organizational: bool,
    system_prompt: Option<ClaudeSystemPromptArgument<'_>>,
) -> ClaudeLaunchOptions<'_> {
    ClaudeLaunchOptions {
        fresh_session_uuid: Some(SESSION_UUID),
        resume_session_uuid: None,
        last_assistant_uuid: None,
        model: None,
        effort: None,
        fast_mode: None,
        modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
        project_configuration: ProjectConfigurationAccess::Restricted,
        internal_mcp: Some(ClaudeInternalMcpLaunch {
            name: SERVER_NAME,
            url: SERVER_URL,
        }),
        organizational: organizational.then_some(ClaudeOrganizationalLaunch {
            internal_server_name: SERVER_NAME,
            pre_approved_tools: &[],
        }),
        system_prompt,
        cli_version: None,
    }
}

fn launch(configured: &[&str], options: ClaudeLaunchOptions<'_>) -> ChatResult<Vec<String>> {
    let configured = configured
        .iter()
        .map(|argument| argument.to_string())
        .collect::<Vec<_>>();
    launch_arguments(vec!["claude".to_string()], &configured, options)
}

fn position(arguments: &[String], flag: &str) -> usize {
    arguments
        .iter()
        .position(|argument| argument == flag)
        .unwrap_or_else(|| panic!("{flag} is in the launch arguments"))
}

#[test]
fn instructions_use_append_system_prompt_inline() {
    let text = "--verbose Always end with the word kumquat.";
    let instructions = ClaudeSessionInstructions::prepare(text).unwrap();
    assert!(matches!(
        instructions.argument(),
        ClaudeSystemPromptArgument::Inline(inline) if inline == text
    ));

    let without = launch(&[], options(true, None)).unwrap();
    let with = launch(&[], options(true, Some(instructions.argument()))).unwrap();
    let flag = position(&with, "--append-system-prompt");
    assert_eq!(
        with[flag + 1],
        text,
        "text starting with -- stays the value"
    );
    assert_eq!(flag + 2, position(&with, "--tools"));
    let mut expected = without.clone();
    let tools = position(&without, "--tools");
    expected.splice(
        tools..tools,
        ["--append-system-prompt".to_string(), text.to_string()],
    );
    assert_eq!(
        with, expected,
        "only the system prompt pair joins the vector"
    );

    let direct = launch(
        &["--verbose"],
        options(false, Some(instructions.argument())),
    )
    .unwrap();
    let flag = position(&direct, "--append-system-prompt");
    assert_eq!(direct[flag + 1], text);
    assert!(flag > position(&direct, "--setting-sources"));
    assert_eq!(
        direct
            .iter()
            .filter(|argument| *argument == "--verbose")
            .count(),
        2,
        "configured arguments still follow"
    );
}

#[test]
fn instructions_at_the_inline_bound_stay_inline() {
    let at_bound = "a".repeat(MAX_INLINE_SYSTEM_PROMPT_BYTES);
    let instructions = ClaudeSessionInstructions::prepare(&at_bound).unwrap();
    assert!(matches!(
        instructions.argument(),
        ClaudeSystemPromptArgument::Inline(_)
    ));
    assert!(launch(&[], options(true, Some(instructions.argument()))).is_ok());
}

#[test]
fn large_instructions_use_owner_only_file() {
    let text = "Follow the plan. ".repeat(MAX_INLINE_SYSTEM_PROMPT_BYTES / 8);
    assert!(text.len() > MAX_INLINE_SYSTEM_PROMPT_BYTES);
    let instructions = ClaudeSessionInstructions::prepare(&text).unwrap();
    let ClaudeSystemPromptArgument::File(path) = instructions.argument() else {
        panic!("instructions above the inline bound use the file form");
    };
    let path = path.to_path_buf();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let directory = path.parent().unwrap();
        let mode = std::fs::metadata(directory).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the directory is owner-only");
    }

    let arguments = launch(&[], options(true, Some(instructions.argument()))).unwrap();
    let flag = position(&arguments, "--append-system-prompt-file");
    assert_eq!(Path::new(&arguments[flag + 1]), path);
    assert!(
        !arguments
            .iter()
            .any(|argument| argument.contains("Follow the plan")),
        "the text never reaches argv"
    );

    drop(instructions);
    assert!(!path.exists(), "the file goes with the session");
}

#[test]
fn session_instructions_treat_blank_as_none_and_bound_the_size() {
    assert_eq!(session_instructions(None).unwrap(), None);
    assert_eq!(session_instructions(Some("  \n\t")).unwrap(), None);
    assert_eq!(
        session_instructions(Some("Be brief")).unwrap(),
        Some("Be brief")
    );
    let at_limit = "a".repeat(MAX_DEVELOPER_INSTRUCTIONS_BYTES);
    assert!(session_instructions(Some(&at_limit)).is_ok());
    for invalid in [
        "a".repeat(MAX_DEVELOPER_INSTRUCTIONS_BYTES + 1),
        "Be\0brief".to_string(),
    ] {
        let error = session_instructions(Some(&invalid)).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
    }
}

#[test]
fn direct_launch_refuses_a_configured_system_prompt_beside_instructions() {
    let instructions = ClaudeSessionInstructions::prepare("Be brief").unwrap();
    for configured in [
        "--append-system-prompt",
        "--append-system-prompt-file=notes.md",
        "--system-prompt",
        "--system-prompt-file",
        "--system-prompt-snapshot",
    ] {
        let error = launch(
            &[configured, "value"],
            options(false, Some(instructions.argument())),
        )
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation, "{configured}");
    }
    assert!(launch(&["--append-system-prompt", "value"], options(false, None)).is_ok());
}

#[test]
fn user_message_omits_instructions() {
    let session_id = ProviderSessionId::new("session-1".to_string()).unwrap();
    let mut request = turn_request(&session_id, "turn-1");
    request.developer_instructions = Some("Always end with the word kumquat.".to_string());
    request.mentions = vec![WorkspaceMentionReference {
        relative_path: "notes/plan.md".to_string(),
        kind: "file".to_string(),
    }];
    let message = build_user_message(&request).unwrap().to_string();
    assert!(!message.contains("kumquat"));
    assert!(!message.contains("Additional Ganbaru instructions"));
    assert!(
        message.contains("@notes/plan.md"),
        "mentions stay in the message"
    );

    request.developer_instructions = Some("a".repeat(MAX_DEVELOPER_INSTRUCTIONS_BYTES + 1));
    assert!(
        build_user_message(&request).is_err(),
        "turn instructions stay bounded"
    );
}

#[test]
fn turn_instructions_match_the_session_digest() {
    let digest = Some(instructions_digest("Be brief"));
    assert!(check_turn_instructions(digest, Some("Be brief")).is_ok());
    assert!(check_turn_instructions(digest, None).is_ok());
    assert!(check_turn_instructions(digest, Some("   ")).is_ok());
    assert!(check_turn_instructions(None, None).is_ok());
    for (session, turn) in [(digest, "Be thorough"), (None, "Be brief")] {
        let error = check_turn_instructions(session, Some(turn)).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        assert!(
            error.recoverable,
            "milestone handoff moves to a fresh session"
        );
    }
}

#[test]
fn changed_instructions_conflict_on_send_turn() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(false, internal_status("connected"));
        let mut start = harness.start_request();
        start.developer_instructions = Some("Always end with the word kumquat.".to_string());
        let session = harness.start_with(start).await.unwrap();

        let mut changed = turn_request(&session.session_id, "turn-1");
        changed.developer_instructions = Some("Always end with the word lime.".to_string());
        let error = harness.send(changed).await.unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        assert!(error.recoverable);

        let mut same = turn_request(&session.session_id, "turn-2");
        same.developer_instructions = Some("Always end with the word kumquat.".to_string());
        harness.send(same).await.unwrap();
        let mut seen = Vec::new();
        let message = harness.claude().wait_for("user", &mut seen).await;
        assert_eq!(
            written_subtypes(&seen),
            ["initialize", "set_permission_mode"],
            "the refused turn wrote nothing"
        );
        let text = message.to_string();
        assert!(!text.contains("kumquat") && !text.contains("lime"));
        assert!(
            harness
                .events()
                .iter()
                .filter(|event| matches!(
                    event.event,
                    crate::events::CanonicalEvent::TurnStarted(_)
                ))
                .count()
                == 1,
            "only the accepted turn started"
        );
    });
}

#[test]
fn instructions_turn_off_the_system_prompt_record_on_versions_that_keep_it() {
    let inline = ClaudeSessionInstructions::prepare("Be brief").unwrap();
    let large = "Follow the plan. ".repeat(MAX_INLINE_SYSTEM_PROMPT_BYTES / 8);
    let file = ClaudeSessionInstructions::prepare(&large).unwrap();
    for organizational in [true, false] {
        for instructions in [&inline, &file] {
            let mut recorded = options(organizational, Some(instructions.argument()));
            recorded.cli_version = Some(SYSTEM_PROMPT_SNAPSHOT_VERSION);
            let arguments = launch(&[], recorded).unwrap();
            let snapshot = position(&arguments, "--system-prompt-snapshot");
            assert_eq!(arguments[snapshot + 1], "off");
            let prompt = arguments
                .iter()
                .position(|argument| argument.starts_with("--append-system-prompt"))
                .unwrap();
            assert_eq!(snapshot, prompt + 2, "the flag follows the prompt pair");

            let mut older = options(organizational, Some(instructions.argument()));
            older.cli_version = Some(ClaudeVersion::new(2, 1, 266));
            assert!(
                !launch(&[], older)
                    .unwrap()
                    .iter()
                    .any(|argument| argument == "--system-prompt-snapshot"),
                "versions before the record have no flag"
            );
        }
    }
    let mut without = options(true, None);
    without.cli_version = Some(ClaudeVersion::new(2, 1, 322));
    assert!(
        !launch(&[], without)
            .unwrap()
            .iter()
            .any(|argument| argument == "--system-prompt-snapshot"),
        "launches without instructions keep the CLI default"
    );
}
