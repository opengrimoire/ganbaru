//! Continuation keys, session transitions, and teammate framing of assignment runs.
//!
//! A native provider session is reused only for a later assignment of the same reply thread
//! whose continuation key is unchanged; any other case starts a new session with the full
//! context package and records why. A reused session receives a delta of that package: only
//! the sources it has not already received.

use super::common::{MAX_MESSAGE_BYTES, sha256_hex, truncate_utf8};
use super::persistence_error;
use ganbaru_chat::agent_runs::{AttemptReason, DeliveredSource, DeliveryKind, HandoffReason};
use ganbaru_chat_contracts::models::*;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashSet};

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

/// Largest continuation header record, in UTF-8 bytes, including its line break.
pub(super) const MAX_CONTINUATION_HEADER_BYTES: usize = 256;

/// Record kind of the header that opens a delta package.
const CONTINUATION_CONTEXT_KIND: &str = "continuation";

/// Source kind of the record that holds what the teammate was asked.
const TRIGGER_SOURCE_KIND: &str = "trigger";

/// The source a frozen package record carries.
#[derive(Clone, Debug)]
pub(super) struct PackageSource {
    pub(super) source_id: String,
    pub(super) source_kind: String,
    pub(super) content_sha256: String,
}

/// One JSON Lines record of a frozen context package, with the source it carries. The
/// coverage record carries none.
#[derive(Debug)]
pub(super) struct PackageRecord<'a> {
    pub(super) line: &'a str,
    pub(super) source: Option<PackageSource>,
}

/// What the native session of a provider thread has already received.
#[derive(Debug, Default)]
pub(super) struct SessionHistory {
    /// Sources, as `(source_id, content_sha256)`, that runs on the thread delivered.
    pub(super) delivered: HashSet<(String, String)>,
    /// Conversation items the thread's runs posted, which the session produced itself.
    pub(super) own_items: HashSet<String>,
}

/// A delta package: the header and the records a continued session has not received, and the
/// sources it delivers.
#[derive(Debug)]
pub(super) struct DeltaPackage {
    pub(super) text: String,
    pub(super) sources: Vec<DeliveredSource>,
}

/// Selects what a continued session still needs from a frozen package: the trigger always, and
/// every other source whose content the session has neither received nor produced. Records
/// keep the package order behind one continuation header, so a delta is a subsequence of the
/// full package. The coverage record passes through uncounted, so the session still learns how
/// much history the package left out, including history newer than its last turn. Returns `None` when the header or the delta cannot fit its bound, so
/// the run sends the full package instead.
pub(super) fn select_delta(
    assignment_id: &ChatWorkAssignmentId,
    records: &[PackageRecord<'_>],
    history: &SessionHistory,
) -> ChatResult<Option<DeltaPackage>> {
    let mut body = String::new();
    let mut sources = Vec::new();
    let mut omitted_delivered = 0_u64;
    for record in records {
        let Some(source) = record.source.as_ref() else {
            body.push_str(record.line);
            continue;
        };
        let received = history.own_items.contains(&source.source_id)
            || history
                .delivered
                .contains(&(source.source_id.clone(), source.content_sha256.clone()));
        if received && source.source_kind != TRIGGER_SOURCE_KIND {
            omitted_delivered = omitted_delivered.saturating_add(1);
            continue;
        }
        body.push_str(record.line);
        sources.push(DeliveredSource {
            source_id: source.source_id.clone(),
            content_sha256: source.content_sha256.clone(),
            kind: DeliveryKind::Package,
        });
    }
    let header = continuation_header_line(assignment_id, omitted_delivered)?;
    if header.len() > MAX_CONTINUATION_HEADER_BYTES || header.len() + body.len() > MAX_MESSAGE_BYTES
    {
        return Ok(None);
    }
    Ok(Some(DeltaPackage {
        text: format!("{header}{body}"),
        sources,
    }))
}

fn continuation_header_line(
    assignment_id: &ChatWorkAssignmentId,
    omitted_delivered: u64,
) -> ChatResult<String> {
    let mut line = serde_json::to_string(&serde_json::json!({
        "kind": CONTINUATION_CONTEXT_KIND,
        "assignment_id": assignment_id.as_str(),
        "omitted_delivered": omitted_delivered,
    }))
    .map_err(|error| {
        ChatError::new(
            ChatErrorCode::Internal,
            format!("The continuation header could not be encoded: {error}"),
            false,
        )
    })?;
    line.push('\n');
    Ok(line)
}

/// Pairs the records of the frozen package `context_package_id`, whose text is `serialized`,
/// with the sources it recorded. A source row's ordinal is the index of its record.
async fn read_package_records<'a>(
    pool: &SqlitePool,
    context_package_id: &str,
    serialized: &'a str,
) -> ChatResult<Vec<PackageRecord<'a>>> {
    let mut sources = sqlx::query_as::<_, (i64, String, String, String)>(
        "SELECT ordinal, source_id, source_kind, content_sha256
         FROM chat_assignment_context_sources
         WHERE context_package_id = ? ORDER BY ordinal",
    )
    .bind(context_package_id)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .map(|(ordinal, source_id, source_kind, content_sha256)| {
        let ordinal = usize::try_from(ordinal).map_err(|_| package_mismatch_error())?;
        Ok((
            ordinal,
            PackageSource {
                source_id,
                source_kind,
                content_sha256,
            },
        ))
    })
    .collect::<ChatResult<BTreeMap<_, _>>>()?;
    let records = serialized
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| PackageRecord {
            line,
            source: sources.remove(&index),
        })
        .collect::<Vec<_>>();
    if !sources.is_empty() {
        return Err(package_mismatch_error());
    }
    Ok(records)
}

fn package_mismatch_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "The context package does not match its recorded sources",
        false,
    )
}

/// Reads what the native session of `provider_thread_id` has received: the sources delivered
/// by its runs that reached the provider, and the items those runs posted.
async fn read_session_history(
    pool: &SqlitePool,
    provider_thread_id: &str,
) -> ChatResult<SessionHistory> {
    let delivered = sqlx::query_as::<_, (String, String)>(
        "SELECT delivered.source_id, delivered.content_sha256
         FROM chat_agent_run_delivered_sources delivered
         JOIN chat_agent_runs run ON run.id = delivered.run_id
         WHERE run.provider_thread_id = ?
           AND run.state IN ('working', 'waiting', 'completed')",
    )
    .bind(provider_thread_id)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .collect();
    let own_items = sqlx::query_scalar::<_, String>(
        "SELECT semantic.item_id
         FROM chat_work_semantic_updates semantic
         JOIN chat_agent_runs run
           ON run.id = json_extract(semantic.payload_data, '$.agentRunId')
          AND run.assignment_id = semantic.assignment_id
         WHERE run.provider_thread_id = ?",
    )
    .bind(provider_thread_id)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .collect();
    Ok(SessionHistory {
        delivered,
        own_items,
    })
}

/// Builds the delta a run continuing `provider_thread_id` sends for the frozen package
/// `context_package_id` whose text is `serialized`, or `None` when the run sends the full
/// package.
pub(super) async fn build_delta_package(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    context_package_id: &str,
    serialized: &str,
    provider_thread_id: &str,
) -> ChatResult<Option<DeltaPackage>> {
    let records = read_package_records(pool, context_package_id, serialized).await?;
    let history = read_session_history(pool, provider_thread_id).await?;
    select_delta(assignment_id, &records, &history)
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

    /// Delta selection over packed test packages and over a stored package.
    mod delta {
        use super::super::*;
        use crate::chat::organization::context::{
            TestPackageRecord, follow_up_context_line, test_package_records,
        };
        use serde_json::Value;

        /// Largest bytes a full package adds around a lone trigger's text. Raising it is a
        /// reviewed change to what every fresh run pays.
        const MAX_FULL_RECORD_OVERHEAD_BYTES: usize = 192;

        fn assignment() -> ChatWorkAssignmentId {
            ChatWorkAssignmentId::new("assignment:later").unwrap()
        }

        fn records(package: &[TestPackageRecord]) -> Vec<PackageRecord<'_>> {
            package
                .iter()
                .map(|(line, source)| PackageRecord {
                    line,
                    source: source.as_ref().map(|(id, kind, sha)| PackageSource {
                        source_id: id.clone(),
                        source_kind: kind.clone(),
                        content_sha256: sha.clone(),
                    }),
                })
                .collect()
        }

        fn delivered(package: &[TestPackageRecord], ids: &[&str]) -> HashSet<(String, String)> {
            package
                .iter()
                .filter_map(|(_, source)| source.as_ref())
                .filter(|(id, _, _)| ids.contains(&id.as_str()))
                .map(|(id, _, sha)| (id.clone(), sha.clone()))
                .collect()
        }

        fn parsed(text: &str) -> Vec<Value> {
            text.lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }

        fn ids(text: &str) -> Vec<String> {
            parsed(text)
                .iter()
                .filter_map(|record| record["id"].as_str().map(str::to_string))
                .collect()
        }

        fn thread_package(edited: &str, trigger: &str) -> Vec<TestPackageRecord> {
            test_package_records(
                ("item:trigger", trigger),
                Some(("item:root", "Plan the release")),
                &[
                    ("item:human-1", "Looks good"),
                    ("item:teammate", "Done, the tests pass"),
                    ("item:human-2", edited),
                ],
                0,
            )
        }

        fn earlier_history(package: &[TestPackageRecord]) -> SessionHistory {
            SessionHistory {
                delivered: delivered(package, &["item:root", "item:human-1", "item:human-2"]),
                own_items: HashSet::from(["item:teammate".to_string()]),
            }
        }

        #[test]
        fn a_continued_session_receives_only_the_header_and_the_new_trigger() {
            let package = thread_package("Ship it after review", "Now update the docs");
            let delta = select_delta(
                &assignment(),
                &records(&package),
                &earlier_history(&package),
            )
            .unwrap()
            .expect("the delta fits");

            let parsed = parsed(&delta.text);
            assert_eq!(parsed.len(), 2);
            assert_eq!(parsed[0]["kind"], "continuation");
            assert_eq!(parsed[0]["assignment_id"], "assignment:later");
            assert_eq!(parsed[0]["omitted_delivered"], 4);
            assert_eq!(parsed[1]["kind"], "trigger");
            assert_eq!(parsed[1]["text"], "Now update the docs");
            assert_eq!(
                delta
                    .sources
                    .iter()
                    .map(|source| (source.source_id.as_str(), source.kind))
                    .collect::<Vec<_>>(),
                vec![("item:trigger", DeliveryKind::Package)]
            );
        }

        #[test]
        fn an_edited_reply_reappears_with_its_new_text() {
            let earlier = thread_package("Ship it after review", "Now update the docs");
            let edited = thread_package("Ship it after the release notes", "Now update the docs");
            let delta = select_delta(&assignment(), &records(&edited), &earlier_history(&earlier))
                .unwrap()
                .unwrap();

            assert_eq!(ids(&delta.text), vec!["item:human-2", "item:trigger"]);
            assert_eq!(
                parsed(&delta.text)[1]["text"],
                "Ship it after the release notes"
            );
            assert_eq!(parsed(&delta.text)[0]["omitted_delivered"], 3);
        }

        #[test]
        fn a_steer_the_session_received_is_not_sent_again() {
            let steer_text = "Prefer the smaller fix";
            let package = test_package_records(
                ("item:trigger", "Now update the docs"),
                Some(("item:root", "Plan the release")),
                &[("item:steer", steer_text)],
                0,
            );
            let steered = follow_up_context_line("item:steer", 1, steer_text.to_string()).unwrap();
            let history = SessionHistory {
                delivered: HashSet::from([
                    ("item:steer".to_string(), steered.content_sha256),
                    delivered(&package, &["item:root"])
                        .into_iter()
                        .next()
                        .unwrap(),
                ]),
                own_items: HashSet::new(),
            };

            let delta = select_delta(&assignment(), &records(&package), &history)
                .unwrap()
                .unwrap();

            assert_eq!(ids(&delta.text), vec!["item:trigger"]);
        }

        #[test]
        fn a_delta_keeps_the_package_order_and_the_coverage_record() {
            let package = test_package_records(
                ("item:trigger", "Now update the docs"),
                Some(("item:root", "Plan the release")),
                &[
                    ("item:reply-1", "First"),
                    ("item:reply-2", "Second"),
                    ("item:reply-3", "Third"),
                    ("item:reply-4", "Fourth"),
                ],
                2,
            );
            let history = SessionHistory {
                delivered: delivered(&package, &["item:root", "item:reply-2", "item:trigger"]),
                own_items: HashSet::new(),
            };

            let delta = select_delta(&assignment(), &records(&package), &history)
                .unwrap()
                .unwrap();

            let full = package
                .iter()
                .map(|(line, _)| line.as_str())
                .collect::<String>();
            let mut full_lines = full.lines();
            for line in delta.text.lines().skip(1) {
                assert!(
                    full_lines.any(|candidate| candidate == line),
                    "{line} is out of package order"
                );
            }
            assert_eq!(
                ids(&delta.text),
                vec![
                    "item:reply-1",
                    "item:reply-3",
                    "item:reply-4",
                    "item:trigger"
                ]
            );
            let records = parsed(&delta.text);
            let coverage = records.last().unwrap();
            assert_eq!(coverage["kind"], "coverage");
            assert_eq!(coverage["omitted_replies"], 2);
            assert_eq!(records[records.len() - 2]["id"], "item:trigger");
            assert_eq!(records[0]["omitted_delivered"], 2);
        }

        #[test]
        fn a_header_that_cannot_fit_sends_the_full_package() {
            let package = test_package_records(("item:trigger", "Go"), None, &[], 0);
            let longest =
                ChatWorkAssignmentId::new("a".repeat(MAX_ORGANIZATIONAL_IDENTIFIER_BYTES)).unwrap();
            let fitting = select_delta(&longest, &records(&package), &SessionHistory::default())
                .unwrap()
                .expect("the longest plain identifier fits the header");
            assert!(fitting.text.lines().next().unwrap().len() < MAX_CONTINUATION_HEADER_BYTES);

            let escaped =
                ChatWorkAssignmentId::new("\"".repeat(MAX_ORGANIZATIONAL_IDENTIFIER_BYTES))
                    .unwrap();
            assert!(
                select_delta(&escaped, &records(&package), &SessionHistory::default())
                    .unwrap()
                    .is_none()
            );
        }

        #[test]
        fn context_overhead_stays_within_budget() {
            assert!(TEAMMATE_FRAMING_TEMPLATE.len() <= MAX_TEAMMATE_FRAMING_BYTES);

            let trigger_id = "i".repeat(MAX_ORGANIZATIONAL_IDENTIFIER_BYTES);
            let text = "Please run the tests";
            let lone = test_package_records((&trigger_id, text), None, &[], 0);
            let full = lone
                .iter()
                .map(|(line, _)| line.as_str())
                .collect::<String>();
            assert!(full.len() - text.len() <= MAX_FULL_RECORD_OVERHEAD_BYTES);

            let package = test_package_records(
                (&trigger_id, text),
                Some(("item:root", "Plan the release")),
                &[("item:reply", "Looks good")],
                0,
            );
            let history = SessionHistory {
                delivered: delivered(&package, &["item:root", "item:reply"]),
                own_items: HashSet::new(),
            };
            let longest =
                ChatWorkAssignmentId::new("a".repeat(MAX_ORGANIZATIONAL_IDENTIFIER_BYTES)).unwrap();
            let delta = select_delta(&longest, &records(&package), &history)
                .unwrap()
                .unwrap();
            assert!(delta.text.len() <= full.len() + MAX_CONTINUATION_HEADER_BYTES);
        }

        /// Stores `package` as the frozen package `context:later` of the fixture assignment.
        async fn store_package(pool: &SqlitePool, package: &[TestPackageRecord]) -> String {
            let serialized = package
                .iter()
                .map(|(line, _)| line.as_str())
                .collect::<String>();
            sqlx::query(
                "INSERT INTO chat_assignment_context_packages
                    (id, assignment_id, revision, triggering_message_item_id, serialized_text,
                     serialized_bytes, sha256, created_at)
                 VALUES ('context:later', 'assignment:folders', 1, 'item:folders', ?, ?, ?,
                         '2026-10-20T12:00:00.000Z')",
            )
            .bind(&serialized)
            .bind(i64::try_from(serialized.len()).unwrap())
            .bind(sha256_hex(serialized.as_bytes()))
            .execute(pool)
            .await
            .unwrap();
            for (ordinal, (_, source)) in package.iter().enumerate() {
                let Some((id, kind, sha)) = source else {
                    continue;
                };
                sqlx::query(
                    "INSERT INTO chat_assignment_context_sources
                        (context_package_id, source_kind, source_id, content_sha256, ordinal)
                     VALUES ('context:later', ?, ?, ?, ?)",
                )
                .bind(kind)
                .bind(id)
                .bind(sha)
                .bind(i64::try_from(ordinal).unwrap())
                .execute(pool)
                .await
                .unwrap();
            }
            serialized
        }

        #[tokio::test]
        async fn a_stored_delta_skips_what_started_runs_received_and_produced() {
            let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
            let package = test_package_records(
                ("item:trigger", "Now update the docs"),
                Some(("item:root", "Plan the release")),
                &[
                    ("item:human", "Looks good"),
                    ("item:teammate", "Done, the tests pass"),
                    ("item:resent", "Check the logs"),
                ],
                0,
            );
            let serialized = store_package(&pool, &package).await;
            let sha = |id: &str| {
                package
                    .iter()
                    .filter_map(|(_, source)| source.as_ref())
                    .find(|(source_id, _, _)| source_id == id)
                    .map(|(_, _, sha)| sha.clone())
                    .unwrap()
            };
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "UPDATE chat_agent_runs SET state = 'failed' WHERE id = 'run:queued';
                 INSERT INTO chat_agent_run_delivered_sources
                    (run_id, source_id, content_sha256, delivery_kind, delivered_at)
                 VALUES
                    ('run:working', 'item:root', '{root}', 'package', '2026-10-20T12:00:00.000Z'),
                    ('run:working', 'item:human', '{human}', 'steer', '2026-10-20T12:00:00.000Z'),
                    ('run:queued', 'item:resent', '{resent}', 'package',
                     '2026-10-20T12:00:00.000Z');
                 INSERT INTO chat_conversation_items
                    (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
                 SELECT 'item:teammate', conversation_id, id, 'message', 'a9',
                        '2026-10-20T12:00:00.000Z'
                 FROM chat_reply_threads WHERE id = 'reply:folders';
                 INSERT INTO chat_work_semantic_updates
                    (item_id, assignment_id, update_kind, payload_data, created_at)
                 VALUES ('item:teammate', 'assignment:folders', 'result',
                         '{{\"agentRunId\":\"run:working\"}}', '2026-10-20T12:00:00.000Z');",
                root = sha("item:root"),
                human = sha("item:human"),
                resent = sha("item:resent"),
            )))
            .execute(&pool)
            .await
            .unwrap();

            let delta = build_delta_package(
                &pool,
                &assignment(),
                "context:later",
                &serialized,
                "thread-1",
            )
            .await
            .unwrap()
            .unwrap();

            assert_eq!(
                ids(&delta.text),
                vec!["item:resent", "item:trigger"],
                "a source of a run that never started is sent again"
            );
            assert_eq!(parsed(&delta.text)[0]["omitted_delivered"], 3);

            let other_thread = build_delta_package(
                &pool,
                &assignment(),
                "context:later",
                &serialized,
                "thread-2",
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(ids(&other_thread.text).len(), 5);
        }
    }
}
