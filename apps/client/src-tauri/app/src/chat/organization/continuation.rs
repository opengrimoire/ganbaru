//! Continuation keys, session transitions, and teammate framing of assignment runs.
//!
//! A native provider session is reused only for a later assignment of the same reply thread
//! whose continuation key is unchanged; any other case starts a new session with the full
//! context package and records why.

use super::common::{sha256_hex, truncate_utf8};
use ganbaru_chat::agent_runs::{AttemptReason, HandoffReason};
use ganbaru_chat_contracts::models::*;
use serde::Serialize;

/// Everything a reused native session must still agree with: the authority scope of the run,
/// the provider instance that holds the session, the model selection, and the system-level
/// instructions the session was started with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContinuationKey {
    pub(super) authorization_scope_digest: String,
    pub(super) provider_instance_id: String,
    pub(super) model_selection_sha256: String,
    pub(super) instructions_sha256: String,
}

impl ContinuationKey {
    /// Builds the key a stored run was launched under, or `None` when the run recorded no model
    /// selection or instruction digest, which never matches a current key.
    pub(super) fn stored(
        authorization_scope_digest: String,
        provider_instance_id: String,
        model_selection_sha256: Option<String>,
        instructions_sha256: Option<String>,
    ) -> Option<Self> {
        Some(Self {
            authorization_scope_digest,
            provider_instance_id,
            model_selection_sha256: model_selection_sha256?,
            instructions_sha256: instructions_sha256?,
        })
    }
}

/// The model values a run sends, in the encoding its continuation key digests.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelSelection<'a> {
    provider_managed_model: bool,
    model_id: Option<&'a ModelId>,
    model_options: &'a [ModelOptionSelection],
}

/// Digest of the model selection a run sends, over the same values dispatch passes to the
/// turn, so a change of model or of any model option ends session reuse.
pub(super) fn model_selection_sha256(
    provider_managed_model: bool,
    model_id: Option<&ModelId>,
    model_options: &[ModelOptionSelection],
) -> ChatResult<String> {
    let selection = ModelSelection {
        provider_managed_model,
        model_id,
        model_options,
    };
    let encoded = serde_json::to_vec(&selection).map_err(|error| {
        ChatError::new(
            ChatErrorCode::Persistence,
            format!("The model selection could not be encoded: {error}"),
            false,
        )
    })?;
    Ok(sha256_hex(&encoded))
}

/// Digest of the system-level instructions a run sends.
pub(super) fn instructions_sha256(instructions: &str) -> String {
    sha256_hex(instructions.as_bytes())
}

/// The newest completed run of an earlier assignment in the same reply thread and execution
/// target, whose native session a new run could continue.
#[derive(Debug)]
pub(super) struct CandidateRun<'a> {
    pub(super) thread_id: &'a str,
    /// The key the candidate was launched under, `None` when it recorded none.
    pub(super) key: Option<ContinuationKey>,
    /// Whether the candidate's authorization revision is still allowed and unrevoked.
    pub(super) authorization_valid: bool,
}

/// What decides how the next run of an assignment starts.
#[derive(Debug)]
pub(super) struct TransitionInput<'a> {
    pub(super) key: &'a ContinuationKey,
    /// Whether the assignment already has a run, so this run retries it.
    pub(super) earlier_run_of_assignment: bool,
    pub(super) candidate: Option<CandidateRun<'a>>,
    /// Whether the provider reported that the session this run tried to continue is gone.
    pub(super) native_session_lost: bool,
}

/// How the next run starts: why, and the provider thread it continues, if any.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct SessionDecision {
    pub(super) reason: AttemptReason,
    /// The provider thread whose native session the run continues; `None` starts a new one.
    pub(super) reuse_thread: Option<String>,
}

impl SessionDecision {
    fn new_session(reason: AttemptReason) -> Self {
        Self {
            reason,
            reuse_thread: None,
        }
    }
}

/// Decides how the next run of an assignment starts. The first matching rule wins, which is
/// also the precedence of the recorded attempt reason:
///
/// 1. A lost native session hands off to a new session.
/// 2. A second run of one assignment retries in a new session, because a failed or cancelled
///    run may have left native state half applied.
/// 3. Without a candidate the run starts fresh.
/// 4. A candidate whose authorization is no longer valid hands off.
/// 5. A candidate whose key is missing or differs hands off.
/// 6. Otherwise the run continues the candidate's session.
pub(super) fn decide_session_transition(input: &TransitionInput<'_>) -> SessionDecision {
    if input.native_session_lost {
        return SessionDecision::new_session(AttemptReason::Handoff(
            HandoffReason::NativeSessionLost,
        ));
    }
    if input.earlier_run_of_assignment {
        return SessionDecision::new_session(AttemptReason::Retry);
    }
    let Some(candidate) = input.candidate.as_ref() else {
        return SessionDecision::new_session(AttemptReason::Fresh);
    };
    if !candidate.authorization_valid {
        return SessionDecision::new_session(AttemptReason::Handoff(
            HandoffReason::AuthorityRevoked,
        ));
    }
    if candidate.key.as_ref() != Some(input.key) {
        return SessionDecision::new_session(AttemptReason::Handoff(HandoffReason::KeyChanged));
    }
    SessionDecision {
        reason: AttemptReason::Continuation,
        reuse_thread: Some(candidate.thread_id.to_string()),
    }
}

/// Placeholder in `TEAMMATE_FRAMING_TEMPLATE` for the teammate's display name.
const NAME_PLACEHOLDER: &str = "{display_name}";
/// Placeholder in `TEAMMATE_FRAMING_TEMPLATE` for the teammate's role.
const ROLE_PLACEHOLDER: &str = "{role}";

/// Tells the session who it is and where its reply goes. The context package format follows
/// it, then the teammate's own instructions.
const TEAMMATE_FRAMING_TEMPLATE: &str = "You are {display_name}, an AI teammate in a Ganbaru \
channel, with this role: {role}. Your reply is posted in the reply thread that assigned the \
work, so report your results there.";

/// Largest framing template, in UTF-8 bytes, with its placeholders unfilled.
pub(super) const MAX_TEAMMATE_FRAMING_BYTES: usize = 1536;

/// Largest display name, in UTF-8 bytes, the framing carries: 160 characters of at most four
/// bytes each, the stored bound on the trimmed name.
const MAX_FRAMED_NAME_BYTES: usize = 160 * 4;

/// Largest role, in UTF-8 bytes, the framing carries. A longer role is cut on a character
/// boundary and marked, so the framing stays within its reserve.
const MAX_FRAMED_ROLE_BYTES: usize = 1536;

/// Marks a framed value that was cut to its bound.
const SHORTENED_MARKER: &str = " [shortened]";

/// Separates the framing, the context package format, and the teammate's instructions.
const INSTRUCTIONS_SEPARATOR: &str = "\n\n";

/// Room in the provider developer instruction cap kept for everything dispatch puts before a
/// teammate's instructions. It is fixed rather than measured so the teammate limit, which the
/// settings editor mirrors, stays put when the framing grows.
pub(super) const INSTRUCTIONS_FRAMING_RESERVE_BYTES: usize = 4 * 1024;

/// Largest teammate instructions, in UTF-8 bytes, that still fit the provider developer
/// instruction cap once dispatch adds the framing and the context package format.
pub(super) const MAX_TEAMMATE_INSTRUCTIONS_BYTES: usize =
    MAX_DEVELOPER_INSTRUCTIONS_BYTES - INSTRUCTIONS_FRAMING_RESERVE_BYTES;

/// Largest framing and format text dispatch puts before a teammate's instructions.
const MAX_FRAMING_BYTES: usize = TEAMMATE_FRAMING_TEMPLATE.len()
    + MAX_FRAMED_NAME_BYTES
    + MAX_FRAMED_ROLE_BYTES
    + INSTRUCTIONS_SEPARATOR.len()
    + super::context::CONTEXT_PACKAGE_FORMAT_INSTRUCTIONS.len()
    + INSTRUCTIONS_SEPARATOR.len();

const _: () = assert!(
    TEAMMATE_FRAMING_TEMPLATE.len() <= MAX_TEAMMATE_FRAMING_BYTES,
    "the teammate framing template must fit its bound"
);
const _: () = assert!(
    MAX_FRAMING_BYTES <= INSTRUCTIONS_FRAMING_RESERVE_BYTES,
    "the assignment instruction framing must fit its reserve"
);

/// Cuts `value` to at most `maximum` bytes on a character boundary, marking a cut value.
fn bounded(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_string();
    }
    let kept = truncate_utf8(value, maximum - SHORTENED_MARKER.len());
    format!("{kept}{SHORTENED_MARKER}")
}

/// Fills the framing template in one pass with the trimmed name and role, so a name or role
/// that contains a placeholder is never filled again.
fn teammate_framing(display_name: &str, role: &str) -> String {
    let display_name = bounded(display_name.trim(), MAX_FRAMED_NAME_BYTES);
    let role = bounded(role.trim(), MAX_FRAMED_ROLE_BYTES);
    let mut framing =
        String::with_capacity(TEAMMATE_FRAMING_TEMPLATE.len() + display_name.len() + role.len());
    let mut rest = TEAMMATE_FRAMING_TEMPLATE;
    while let Some(start) = rest.find('{') {
        framing.push_str(&rest[..start]);
        let placeholder = &rest[start..];
        if let Some(after) = placeholder.strip_prefix(NAME_PLACEHOLDER) {
            framing.push_str(&display_name);
            rest = after;
        } else if let Some(after) = placeholder.strip_prefix(ROLE_PLACEHOLDER) {
            framing.push_str(&role);
            rest = after;
        } else {
            framing.push('{');
            rest = &placeholder[1..];
        }
    }
    framing.push_str(rest);
    framing
}

/// Composes the system-level instructions of an assignment run: the teammate framing, then the
/// context package format, then the teammate's own instructions when it has any.
pub(super) fn compose_system_instructions(
    display_name: &str,
    role: &str,
    instructions: &str,
) -> String {
    let framing = teammate_framing(display_name, role);
    let format = super::context::CONTEXT_PACKAGE_FORMAT_INSTRUCTIONS;
    if instructions.trim().is_empty() {
        format!("{framing}{INSTRUCTIONS_SEPARATOR}{format}")
    } else {
        format!("{framing}{INSTRUCTIONS_SEPARATOR}{format}{INSTRUCTIONS_SEPARATOR}{instructions}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ContinuationKey {
        ContinuationKey {
            authorization_scope_digest: "scope".to_string(),
            provider_instance_id: "codex".to_string(),
            model_selection_sha256: "model".to_string(),
            instructions_sha256: "instructions".to_string(),
        }
    }

    fn candidate(key: Option<ContinuationKey>, authorization_valid: bool) -> CandidateRun<'static> {
        CandidateRun {
            thread_id: "provider-execution:earlier",
            key,
            authorization_valid,
        }
    }

    fn decide(
        earlier_run_of_assignment: bool,
        candidate: Option<CandidateRun<'_>>,
        native_session_lost: bool,
    ) -> SessionDecision {
        decide_session_transition(&TransitionInput {
            key: &key(),
            earlier_run_of_assignment,
            candidate,
            native_session_lost,
        })
    }

    fn new_session(reason: AttemptReason) -> SessionDecision {
        SessionDecision::new_session(reason)
    }

    #[test]
    fn an_unchanged_key_continues_the_candidate_session() {
        assert_eq!(
            decide(false, Some(candidate(Some(key()), true)), false),
            SessionDecision {
                reason: AttemptReason::Continuation,
                reuse_thread: Some("provider-execution:earlier".to_string()),
            }
        );
    }

    #[test]
    fn each_rule_starts_a_new_session_with_its_reason() {
        assert_eq!(
            decide(false, None, false),
            new_session(AttemptReason::Fresh)
        );
        assert_eq!(decide(true, None, false), new_session(AttemptReason::Retry));
        assert_eq!(
            decide(false, Some(candidate(Some(key()), false)), false),
            new_session(AttemptReason::Handoff(HandoffReason::AuthorityRevoked))
        );
        assert_eq!(
            decide(false, Some(candidate(None, true)), false),
            new_session(AttemptReason::Handoff(HandoffReason::KeyChanged))
        );
        assert_eq!(
            decide(false, Some(candidate(Some(key()), true)), true),
            new_session(AttemptReason::Handoff(HandoffReason::NativeSessionLost))
        );
    }

    #[test]
    fn earlier_rules_take_precedence() {
        let changed = ContinuationKey {
            instructions_sha256: "edited".to_string(),
            ..key()
        };
        assert_eq!(
            decide(true, Some(candidate(Some(key()), true)), true),
            new_session(AttemptReason::Handoff(HandoffReason::NativeSessionLost)),
            "a lost session beats a retry"
        );
        assert_eq!(
            decide(true, Some(candidate(Some(key()), true)), false),
            new_session(AttemptReason::Retry),
            "a retry never resumes, even with a matching candidate"
        );
        assert_eq!(
            decide(false, Some(candidate(Some(changed), false)), false),
            new_session(AttemptReason::Handoff(HandoffReason::AuthorityRevoked)),
            "a revoked authorization beats a changed key"
        );
    }

    #[test]
    fn a_change_to_any_key_component_hands_off() {
        let changes = [
            ContinuationKey {
                authorization_scope_digest: "narrower".to_string(),
                ..key()
            },
            ContinuationKey {
                provider_instance_id: "claude".to_string(),
                ..key()
            },
            ContinuationKey {
                model_selection_sha256: "other-model".to_string(),
                ..key()
            },
            ContinuationKey {
                instructions_sha256: "edited".to_string(),
                ..key()
            },
        ];
        for stored in changes {
            assert_eq!(
                decide(false, Some(candidate(Some(stored), true)), false),
                new_session(AttemptReason::Handoff(HandoffReason::KeyChanged))
            );
        }
    }

    #[test]
    fn a_stored_key_needs_both_recorded_digests() {
        let stored = |model: Option<&str>, instructions: Option<&str>| {
            ContinuationKey::stored(
                "scope".to_string(),
                "codex".to_string(),
                model.map(str::to_string),
                instructions.map(str::to_string),
            )
        };
        assert_eq!(stored(Some("model"), Some("instructions")), Some(key()));
        assert_eq!(stored(None, Some("instructions")), None);
        assert_eq!(stored(Some("model"), None), None);
    }

    #[test]
    fn the_model_selection_digest_covers_model_id_options_and_provider_management() {
        let model = ModelId::new("gpt-5").unwrap();
        let other = ModelId::new("gpt-5-mini").unwrap();
        let effort = |value: &str| {
            vec![ModelOptionSelection {
                key: "effort".to_string(),
                value: ModelOptionValue::Choice(value.to_string()),
            }]
        };
        let digest = |managed, model: Option<&ModelId>, options: &[ModelOptionSelection]| {
            model_selection_sha256(managed, model, options).unwrap()
        };
        let base = digest(false, Some(&model), &effort("high"));
        assert_eq!(base, digest(false, Some(&model), &effort("high")));
        assert_ne!(base, digest(false, Some(&other), &effort("high")));
        assert_ne!(base, digest(false, Some(&model), &effort("low")));
        assert_ne!(base, digest(false, Some(&model), &[]));
        assert_ne!(base, digest(true, Some(&model), &effort("high")));
        assert_ne!(base, digest(false, None, &effort("high")));
    }

    #[test]
    fn instructions_put_framing_and_format_before_the_teammate_instructions() {
        let format = super::super::context::CONTEXT_PACKAGE_FORMAT_INSTRUCTIONS;
        let framing = teammate_framing("Reviewer", "Reviews pull requests");
        assert!(framing.contains("You are Reviewer,"));
        assert!(framing.contains("with this role: Reviews pull requests."));
        assert_eq!(
            compose_system_instructions("Reviewer", "Reviews pull requests", "  \n"),
            format!("{framing}\n\n{format}")
        );
        assert_eq!(
            compose_system_instructions("Reviewer", "Reviews pull requests", "Be brief."),
            format!("{framing}\n\n{format}\n\nBe brief.")
        );
    }

    #[test]
    fn a_name_or_role_holding_a_placeholder_is_not_filled_again() {
        let framing = teammate_framing("{role}", "{display_name}");
        assert!(framing.starts_with("You are {role}, "));
        assert!(framing.contains("with this role: {display_name}."));
    }

    #[test]
    fn a_framing_change_changes_the_instructions_digest() {
        let original = compose_system_instructions("Reviewer", "Reviews", "Be brief.");
        assert_ne!(
            instructions_sha256(&original),
            instructions_sha256(&compose_system_instructions(
                "Editor",
                "Reviews",
                "Be brief."
            ))
        );
        assert_ne!(
            instructions_sha256(&original),
            instructions_sha256(&compose_system_instructions(
                "Reviewer",
                "Edits",
                "Be brief."
            ))
        );
        assert_ne!(
            instructions_sha256(&original),
            instructions_sha256(&compose_system_instructions(
                "Reviewer", "Reviews", "Be kind."
            ))
        );
    }

    #[test]
    fn the_longest_valid_teammate_instructions_fit_the_provider_cap_after_framing() {
        use super::super::common::validate_profile_text;
        let ascii = "a".repeat(MAX_TEAMMATE_INSTRUCTIONS_BYTES);
        let multibyte = "é".repeat(MAX_TEAMMATE_INSTRUCTIONS_BYTES / 'é'.len_utf8());
        for instructions in [&ascii, &multibyte] {
            validate_profile_text(
                instructions,
                MAX_TEAMMATE_INSTRUCTIONS_BYTES,
                "instructions",
            )
            .unwrap();
            assert!(
                compose_system_instructions("Reviewer", "Reviews", instructions).len()
                    <= MAX_DEVELOPER_INSTRUCTIONS_BYTES
            );
        }
        let over = format!("{multibyte}é");
        let refused = validate_profile_text(&over, MAX_TEAMMATE_INSTRUCTIONS_BYTES, "instructions")
            .expect_err("one character over the byte bound is refused");
        assert_eq!(refused.code, ChatErrorCode::Validation);
    }

    #[test]
    fn the_settings_editor_mirrors_the_teammate_instruction_bound() {
        let editor = include_str!("../../../../../src/lib/chat/teammates/draft.ts");
        let mirrored = editor
            .lines()
            .find_map(|line| line.strip_prefix("export const TEAMMATE_INSTRUCTIONS_MAX_BYTES = "))
            .and_then(|value| value.strip_suffix(';'))
            .map(|value| value.replace('_', ""))
            .and_then(|value| value.parse::<usize>().ok());
        assert_eq!(mirrored, Some(MAX_TEAMMATE_INSTRUCTIONS_BYTES));
    }

    #[test]
    fn the_longest_name_and_role_fit_the_framing_reserve() {
        let name = "\u{1F600}".repeat(160);
        let role = "\u{1F600}".repeat(1_000);
        let teammate = "a".repeat(MAX_TEAMMATE_INSTRUCTIONS_BYTES);
        let composed = compose_system_instructions(&name, &role, &teammate);
        assert!(composed.len() <= MAX_DEVELOPER_INSTRUCTIONS_BYTES);
        assert!(composed.len() - teammate.len() <= INSTRUCTIONS_FRAMING_RESERVE_BYTES);
        let framing = teammate_framing(&name, &role);
        assert!(framing.contains(&name), "a valid name is never cut");
        assert!(framing.contains(SHORTENED_MARKER.trim_start()));
        let short_role = "\u{1F600}".repeat(MAX_FRAMED_ROLE_BYTES / 4);
        assert!(!teammate_framing(&name, &short_role).contains(SHORTENED_MARKER.trim_start()));
    }
}
