//! Chat adapter: the value checks that mirror the Chat identity and policy table checks, and the
//! priorities of their coupled groups.
//!
//! A teammate's current policy and an access profile's current revision are coupled with a copy
//! of the revision's restrictiveness. Concurrent edits keep the more restrictive one: asking
//! for approval outranks approving for the user, which outranks full access, which outranks a
//! custom mode that defers to the provider, and a lower maximum folder capability outranks a
//! higher one. An edit made after
//! seeing the other still wins by causality.
//!
//! Conversation memberships, AI channel memberships, and working folder grants keep the narrower
//! access among concurrent edits: removal and revocation first, then the stricter approval
//! override, folder capability, and channel capabilities. A channel archive outranks a
//! concurrent unarchive.

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::chat::{
    ADAPTER, CHANNEL_MEMBERSHIPS_TABLE, CHANNELS_TABLE, CONVERSATIONS_TABLE, FOLDER_GRANTS_TABLE,
    MEMBERSHIPS_TABLE, PARTICIPANTS_TABLE, POLICY_REVISIONS_TABLE, PROFILE_REVISIONS_TABLE,
    PROFILES_TABLE, TEAMMATES_TABLE, channel_group, channel_membership_group, conversation_group,
    folder_grant_group, membership_group, participant_group, policy_revision_group, profile_group,
    profile_revision_group, teammate_group,
};

/// Characters of a display name after trimming.
pub const MAX_DISPLAY_NAME_CHARS: usize = 160;
/// Characters of a teammate role after trimming.
pub const MAX_ROLE_CHARS: usize = 1000;
/// Characters of teammate instructions.
pub const MAX_INSTRUCTIONS_CHARS: usize = 65_536;
/// Characters of a provider instance id.
pub const MAX_PROVIDER_INSTANCE_CHARS: usize = 1024;
/// Characters of a stored JSON object: an avatar, a model selection, or provider options.
pub const MAX_JSON_CHARS: usize = 16_384;
/// Characters of the longest enumerated value any Chat replicated column holds.
pub const MAX_ENUM_CHARS: usize = 32;
/// Fewest characters of a stored timestamp, as the tables check it.
pub const MIN_TIMESTAMP_CHARS: usize = 20;

/// Participant kinds.
pub const PARTICIPANT_KINDS: [&str; 3] = ["local_user", "ai_teammate", "human"];
/// Teammate safety modes, least restrictive first. Custom defers to the
/// provider's own configuration, which can grant anything, so it ranks lowest.
pub const SAFETY_MODES: [&str; 4] = [
    "custom",
    "full_access",
    "approve_for_me",
    "ask_for_approval",
];
/// Reasoning effort levels.
pub const EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];
/// Response speeds.
pub const SPEEDS: [&str; 2] = ["standard", "fast"];
/// Interaction modes.
pub const INTERACTION_MODES: [&str; 1] = ["build"];
/// Built-in access profile keys.
pub const BUILTIN_PROFILE_KEYS: [&str; 5] = [
    "conversation_only",
    "read_only",
    "edit_files",
    "build_and_test",
    "publish_changes",
];
/// History boundaries of an access profile revision.
pub const HISTORY_BOUNDARIES: [&str; 2] = ["entire", "from_grant"];
/// Maximum folder capabilities, least restrictive first.
pub const FOLDER_CAPABILITIES: [&str; 5] = ["publish", "execute", "edit", "read", "none"];
/// Characters of a channel name.
pub const MAX_CHANNEL_NAME_CHARS: usize = 80;
/// Characters of a channel topic.
pub const MAX_CHANNEL_TOPIC_CHARS: usize = 250;
/// Conversation kinds.
pub const CONVERSATION_KINDS: [&str; 3] = ["channel", "direct_message", "task_discussion"];
/// Conversation membership roles, least restrictive first.
pub const MEMBERSHIP_ROLES: [&str; 2] = ["owner", "member"];
/// Runtime approval overrides, least restrictive first. Provider custom defers to the
/// provider's own configuration, mirroring the custom safety mode.
pub const APPROVAL_POLICIES: [&str; 4] = ["provider_custom", "unattended", "auto_approve", "ask"];
/// Rank of no approval override, the least restrictive.
const INHERITED_APPROVAL_RANK: u8 = 1;

fn text(field: &Field) -> Result<&str, AdapterError> {
    match field {
        Field::Text(text) => Ok(text),
        _ => Err(AdapterError("chat text field")),
    }
}

fn optional_text(field: &Field) -> Result<Option<&str>, AdapterError> {
    match field {
        Field::Null => Ok(None),
        field => text(field).map(Some),
    }
}

fn integer(field: &Field) -> Result<i64, AdapterError> {
    match field {
        Field::Integer(value) => Ok(*value),
        _ => Err(AdapterError("chat integer field")),
    }
}

fn one_of(field: &Field, allowed: &[&str], what: &'static str) -> Result<(), AdapterError> {
    if allowed.contains(&text(field)?) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn optional_one_of(
    field: &Field,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => one_of(field, allowed, what),
    }
}

fn timestamp(field: &Field) -> Result<(), AdapterError> {
    if text(field)?.chars().count() >= MIN_TIMESTAMP_CHARS {
        Ok(())
    } else {
        Err(AdapterError("chat timestamp is too short"))
    }
}

fn optional_timestamp(field: &Field) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => timestamp(field),
    }
}

/// Text trimmed of spaces, as SQLite `trim` does, holding 1 to `max` characters.
fn trimmed(field: &Field, max: usize, what: &'static str) -> Result<(), AdapterError> {
    let trimmed = text(field)?.trim_matches(' ').chars().count();
    if (1..=max).contains(&trimmed) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn bounded(field: &Field, min: usize, max: usize, what: &'static str) -> Result<(), AdapterError> {
    if (min..=max).contains(&text(field)?.chars().count()) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn schema_version(field: &Field) -> Result<(), AdapterError> {
    if integer(field)? >= 1 {
        Ok(())
    } else {
        Err(AdapterError("chat schema version"))
    }
}

/// A JSON object within [`MAX_JSON_CHARS`], as the table checks require.
fn json_object(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    let json = text(field)?;
    if json.chars().count() > MAX_JSON_CHARS {
        return Err(AdapterError(what));
    }
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json)
        .map(|_| ())
        .map_err(|_| AdapterError(what))
}

fn flag(field: &Field) -> Result<(), AdapterError> {
    if matches!(integer(field)?, 0 | 1) {
        Ok(())
    } else {
        Err(AdapterError("chat flag"))
    }
}

/// A current revision pointer and its copied restrictiveness: both null, or a revision id with
/// a value from `allowed`.
fn current_pair(
    id: &Field,
    copied: &Field,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), AdapterError> {
    match (optional_text(id)?, copied) {
        (None, Field::Null) => Ok(()),
        (Some(_), copied) => one_of(copied, allowed, what),
        (None, _) => Err(AdapterError(what)),
    }
}

fn validate_participant(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (participant_group::CREATED, [kind, created_at]) => {
            one_of(kind, &PARTICIPANT_KINDS, "chat participant kind")?;
            timestamp(created_at)
        }
        (participant_group::PROFILE, [name, avatar_version, avatar]) => {
            trimmed(name, MAX_DISPLAY_NAME_CHARS, "chat participant name")?;
            schema_version(avatar_version)?;
            json_object(avatar, "chat participant avatar")
        }
        (participant_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        (participant_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat participant value")),
    }
}

fn validate_teammate(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (teammate_group::CREATED | teammate_group::UPDATED, [at]) => timestamp(at),
        (teammate_group::PROFILE, [role, instructions]) => {
            trimmed(role, MAX_ROLE_CHARS, "chat teammate role")?;
            bounded(
                instructions,
                0,
                MAX_INSTRUCTIONS_CHARS,
                "chat teammate instructions",
            )
        }
        (teammate_group::POLICY, [id, safety]) => {
            current_pair(id, safety, &SAFETY_MODES, "chat teammate current policy")
        }
        (teammate_group::HOME, [device]) => match optional_text(device)? {
            Some("") => Err(AdapterError("chat teammate home device")),
            _ => Ok(()),
        },
        _ => Err(AdapterError("chat teammate value")),
    }
}

fn validate_policy_revision(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            policy_revision_group::CREATED,
            [
                _teammate,
                provider,
                model_version,
                model,
                effort,
                speed,
                options_version,
                options,
                mode,
                created_at,
                safety,
            ],
        ) => {
            bounded(
                provider,
                1,
                MAX_PROVIDER_INSTANCE_CHARS,
                "chat policy provider instance",
            )?;
            schema_version(model_version)?;
            json_object(model, "chat policy model selection")?;
            optional_one_of(effort, &EFFORTS, "chat policy effort")?;
            optional_one_of(speed, &SPEEDS, "chat policy speed")?;
            schema_version(options_version)?;
            json_object(options, "chat policy provider options")?;
            one_of(mode, &INTERACTION_MODES, "chat policy interaction mode")?;
            timestamp(created_at)?;
            one_of(safety, &SAFETY_MODES, "chat policy safety mode")
        }
        _ => Err(AdapterError("chat policy revision value")),
    }
}

fn validate_profile(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (profile_group::CREATED, [builtin, created_at]) => {
            optional_one_of(builtin, &BUILTIN_PROFILE_KEYS, "chat built-in profile key")?;
            timestamp(created_at)
        }
        (profile_group::NAME, [name]) => {
            trimmed(name, MAX_DISPLAY_NAME_CHARS, "chat access profile name")
        }
        (profile_group::CURRENT, [id, capability]) => current_pair(
            id,
            capability,
            &FOLDER_CAPABILITIES,
            "chat access profile current revision",
        ),
        (profile_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        (profile_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat access profile value")),
    }
}

fn validate_profile_revision(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            profile_revision_group::CREATED,
            [
                _profile,
                read_history,
                participate,
                boundary,
                capability,
                created_at,
            ],
        ) => {
            flag(read_history)?;
            flag(participate)?;
            one_of(
                boundary,
                &HISTORY_BOUNDARIES,
                "chat profile history boundary",
            )?;
            one_of(
                capability,
                &FOLDER_CAPABILITIES,
                "chat profile folder capability",
            )?;
            timestamp(created_at)
        }
        _ => Err(AdapterError("chat access profile revision value")),
    }
}

fn validate_conversation(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (conversation_group::CREATED, [_project, kind, created_at]) => {
            one_of(kind, &CONVERSATION_KINDS, "chat conversation kind")?;
            timestamp(created_at)
        }
        (conversation_group::ACTIVITY | conversation_group::UPDATED, [at]) => timestamp(at),
        (conversation_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        _ => Err(AdapterError("chat conversation value")),
    }
}

fn validate_membership(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (membership_group::IDENTITY, [_, _]) | (membership_group::EXISTS, []) => Ok(()),
        (membership_group::MEMBERSHIP, [role, removed_at]) => {
            one_of(role, &MEMBERSHIP_ROLES, "chat membership role")?;
            optional_timestamp(removed_at)
        }
        (membership_group::CREATED | membership_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat membership value")),
    }
}

/// A history boundary and its ordinal: `entire` with none, or `from_grant` with one of at
/// least 1.
fn history_pair(boundary: &Field, ordinal: &Field) -> Result<(), AdapterError> {
    one_of(boundary, &HISTORY_BOUNDARIES, "chat history boundary")?;
    match (text(boundary)?, ordinal) {
        ("entire", Field::Null) => Ok(()),
        ("from_grant", Field::Integer(ordinal)) if *ordinal >= 1 => Ok(()),
        _ => Err(AdapterError("chat history boundary ordinal")),
    }
}

fn validate_channel_membership(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (channel_membership_group::IDENTITY, [_, _]) | (channel_membership_group::EXISTS, []) => {
            Ok(())
        }
        (
            channel_membership_group::ACCESS,
            [
                _profile,
                read_history,
                read_history_inherits,
                participate,
                participate_inherits,
                boundary,
                boundary_inherits,
                ordinal,
                approval,
                scratch_approval,
                removed_at,
            ],
        ) => {
            for value in [
                read_history,
                read_history_inherits,
                participate,
                participate_inherits,
                boundary_inherits,
            ] {
                flag(value)?;
            }
            history_pair(boundary, ordinal)?;
            optional_one_of(approval, &APPROVAL_POLICIES, "chat approval policy")?;
            optional_one_of(scratch_approval, &APPROVAL_POLICIES, "chat approval policy")?;
            optional_timestamp(removed_at)
        }
        (channel_membership_group::CREATED | channel_membership_group::UPDATED, [at]) => {
            timestamp(at)
        }
        _ => Err(AdapterError("chat channel membership value")),
    }
}

fn validate_folder_grant(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (folder_grant_group::IDENTITY, [_, _, _]) | (folder_grant_group::EXISTS, []) => Ok(()),
        (folder_grant_group::GRANT, [capability, inherits, is_default, approval, revoked_at]) => {
            one_of(
                capability,
                &FOLDER_CAPABILITIES,
                "chat folder grant capability",
            )?;
            flag(inherits)?;
            flag(is_default)?;
            optional_one_of(approval, &APPROVAL_POLICIES, "chat approval policy")?;
            optional_timestamp(revoked_at)
        }
        (folder_grant_group::CREATED, [_project, created_at]) => timestamp(created_at),
        _ => Err(AdapterError("chat folder grant value")),
    }
}

fn validate_channel(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (channel_group::CREATED, [_project, _conversation, is_default, created_at]) => {
            flag(is_default)?;
            timestamp(created_at)
        }
        (channel_group::DETAILS, [name, topic]) => {
            bounded(name, 1, MAX_CHANNEL_NAME_CHARS, "chat channel name")?;
            bounded(topic, 0, MAX_CHANNEL_TOPIC_CHARS, "chat channel topic")
        }
        (channel_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        (channel_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat channel value")),
    }
}

/// Priority of an ended row: a removal or revocation outranks every live value.
const ENDED: u8 = u8::MAX;

/// Rank of an approval override, 1 for the least restrictive.
///
/// No override inherits: a folder grant's or scratch override falls back to the channel
/// override and a channel override to the teammate's default, which an explicit override can
/// only narrow (the runtime keeps the narrower of the default and the override). The default
/// can be unattended or defer to the provider, so inheriting ranks lowest, below the explicit
/// policies in [`APPROVAL_POLICIES`] order. A folder or scratch override replaces the channel
/// override instead of narrowing it, so when the channel override is stricter than a concurrent
/// explicit folder or scratch override, inheriting was the stricter intent; a static rank cannot
/// see the channel row, and the loser's value is not available to Chat after the merge.
fn approval_rank(field: Option<&Field>) -> u8 {
    match field {
        None | Some(Field::Null) => INHERITED_APPROVAL_RANK,
        field => INHERITED_APPROVAL_RANK + restrictiveness(field, &APPROVAL_POLICIES),
    }
}

/// Number of approval override ranks.
const APPROVAL_RANKS: u8 = INHERITED_APPROVAL_RANK + APPROVAL_POLICIES.len() as u8;

/// Whether the field at `index` is set, which marks a removal or revocation.
fn ended(fields: &[Field], index: usize) -> bool {
    !matches!(fields.get(index), None | Some(Field::Null))
}

/// Whether the inherit flag at `index` is set, so the value beside it comes from the profile.
fn inherits(fields: &[Field], index: usize) -> bool {
    matches!(fields.get(index), Some(Field::Integer(1)))
}

/// A membership removal wins, then demotion to member over promotion to owner.
fn membership_priority(fields: &[Field]) -> u8 {
    if ended(fields, 1) {
        ENDED
    } else {
        restrictiveness(fields.first(), &MEMBERSHIP_ROLES)
    }
}

/// A channel removal wins, then the value that clears more of history reading, participation,
/// and full history, then the stricter channel approval override, then the stricter scratch
/// approval override.
///
/// A capability counts as cleared only when it is set explicitly: an inherited one follows the
/// profile, which may grant it, and an explicit grant is still capped by the profile, so only
/// an explicit clear narrows. The access profile itself carries no rank, so concurrent edits
/// that differ only in the profile keep the engine's tie order.
fn channel_membership_priority(fields: &[Field]) -> u8 {
    if ended(fields, 10) {
        return ENDED;
    }
    let cleared = [
        !inherits(fields, 2) && matches!(fields.get(1), Some(Field::Integer(0))),
        !inherits(fields, 4) && matches!(fields.get(3), Some(Field::Integer(0))),
        !inherits(fields, 6)
            && matches!(fields.get(5), Some(Field::Text(text)) if text == "from_grant"),
    ]
    .into_iter()
    .filter(|cleared| *cleared)
    .count();
    let cleared = u8::try_from(cleared).unwrap_or(0);
    let approvals =
        (approval_rank(fields.get(8)) - 1) * APPROVAL_RANKS + (approval_rank(fields.get(9)) - 1);
    1 + cleared * APPROVAL_RANKS * APPROVAL_RANKS + approvals
}

/// A revocation wins, then the lower folder capability, then the stricter approval override.
/// An inherited capability follows the profile's maximum, which may be publishing, so it ranks
/// as the least restrictive capability.
fn folder_grant_priority(fields: &[Field]) -> u8 {
    if ended(fields, 4) {
        return ENDED;
    }
    let capability = if inherits(fields, 1) {
        1
    } else {
        restrictiveness(fields.first(), &FOLDER_CAPABILITIES).max(1)
    };
    1 + (capability - 1) * APPROVAL_RANKS + (approval_rank(fields.get(3)) - 1)
}

/// An archive wins over a concurrent unarchive.
fn lifecycle_priority(fields: &[Field]) -> u8 {
    if ended(fields, 0) { 2 } else { 1 }
}

/// One plus the position of `field`'s text in `ranked`, least restrictive first, or 0 for null
/// or anything else, so a cleared pointer never outranks a set one.
fn restrictiveness(field: Option<&Field>, ranked: &[&str]) -> u8 {
    let Some(Field::Text(text)) = field else {
        return 0;
    };
    ranked
        .iter()
        .position(|candidate| candidate == text)
        .and_then(|position| u8::try_from(position + 1).ok())
        .unwrap_or(0)
}

/// Adapter of the Chat domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChatAdapter;

impl DomainAdapter for ChatAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    /// The more restrictive current policy or profile revision wins among concurrent edits.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        let copied = value.fields().get(1);
        if table == TEAMMATES_TABLE && group == teammate_group::POLICY {
            restrictiveness(copied, &SAFETY_MODES)
        } else if table == PROFILES_TABLE && group == profile_group::CURRENT {
            restrictiveness(copied, &FOLDER_CAPABILITIES)
        } else if table == MEMBERSHIPS_TABLE && group == membership_group::MEMBERSHIP {
            membership_priority(value.fields())
        } else if table == CHANNEL_MEMBERSHIPS_TABLE && group == channel_membership_group::ACCESS {
            channel_membership_priority(value.fields())
        } else if table == FOLDER_GRANTS_TABLE && group == folder_grant_group::GRANT {
            folder_grant_priority(value.fields())
        } else if (table == CHANNELS_TABLE && group == channel_group::LIFECYCLE)
            || (table == CONVERSATIONS_TABLE && group == conversation_group::LIFECYCLE)
        {
            lifecycle_priority(value.fields())
        } else {
            0
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        let fields = value.fields();
        if table == PARTICIPANTS_TABLE {
            validate_participant(group, fields)
        } else if table == TEAMMATES_TABLE {
            validate_teammate(group, fields)
        } else if table == POLICY_REVISIONS_TABLE {
            validate_policy_revision(group, fields)
        } else if table == PROFILES_TABLE {
            validate_profile(group, fields)
        } else if table == PROFILE_REVISIONS_TABLE {
            validate_profile_revision(group, fields)
        } else if table == CONVERSATIONS_TABLE {
            validate_conversation(group, fields)
        } else if table == MEMBERSHIPS_TABLE {
            validate_membership(group, fields)
        } else if table == CHANNEL_MEMBERSHIPS_TABLE {
            validate_channel_membership(group, fields)
        } else if table == FOLDER_GRANTS_TABLE {
            validate_folder_grant(group, fields)
        } else if table == CHANNELS_TABLE {
            validate_channel(group, fields)
        } else {
            Ok(())
        }
    }

    /// Chat is owner-only until its presentation lands, so rows present nothing.
    fn presentation(&self, _table: TableId, _values: &[(GroupId, Value)]) -> Presentation {
        Presentation::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "2026-10-24T10:51:41.000Z";

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn t(text: &str) -> Field {
        Field::Text(text.to_owned())
    }

    fn policy(safety: Option<&str>) -> Value {
        match safety {
            Some(safety) => value(vec![t("policy-1"), t(safety)]),
            None => value(vec![Field::Null, Field::Null]),
        }
    }

    fn current(capability: Option<&str>) -> Value {
        match capability {
            Some(capability) => value(vec![t("revision-1"), t(capability)]),
            None => value(vec![Field::Null, Field::Null]),
        }
    }

    fn policy_revision(model: &str, safety: &str) -> Value {
        value(vec![
            t("teammate-1"),
            t("codex"),
            Field::Integer(1),
            t(model),
            t("high"),
            Field::Null,
            Field::Integer(1),
            t("{}"),
            t("build"),
            t(AT),
            t(safety),
        ])
    }

    #[test]
    fn chat_priorities_prefer_the_more_restrictive_revision() {
        let adapter = ChatAdapter;
        let safety =
            |mode| adapter.priority(TEAMMATES_TABLE, teammate_group::POLICY, &policy(mode));
        assert!(safety(Some("ask_for_approval")) > safety(Some("approve_for_me")));
        assert!(safety(Some("approve_for_me")) > safety(Some("full_access")));
        assert!(safety(Some("full_access")) > safety(Some("custom")));
        assert!(safety(Some("custom")) > safety(None));

        let capability =
            |level| adapter.priority(PROFILES_TABLE, profile_group::CURRENT, &current(level));
        assert!(capability(Some("none")) > capability(Some("read")));
        assert!(capability(Some("read")) > capability(Some("edit")));
        assert!(capability(Some("edit")) > capability(Some("execute")));
        assert!(capability(Some("execute")) > capability(Some("publish")));
        assert!(capability(Some("publish")) > capability(None));

        let name = value(vec![t("Profile")]);
        assert_eq!(
            adapter.priority(PROFILES_TABLE, profile_group::NAME, &name),
            0
        );
    }

    #[test]
    fn chat_validation_mirrors_the_table_checks() {
        let adapter = ChatAdapter;
        let check = |table, group, fields| adapter.validate(table, group, &value(fields));

        assert!(
            check(
                PARTICIPANTS_TABLE,
                participant_group::CREATED,
                vec![t("ai_teammate"), t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                PARTICIPANTS_TABLE,
                participant_group::CREATED,
                vec![t("robot"), t(AT)]
            )
            .is_err()
        );
        let avatar = |json: &str| {
            check(
                PARTICIPANTS_TABLE,
                participant_group::PROFILE,
                vec![t(" Ada "), Field::Integer(1), t(json)],
            )
        };
        assert!(avatar("{\"kind\":\"initials\"}").is_ok());
        assert!(avatar("[]").is_err());
        assert!(avatar("{").is_err());
        let oversized = format!("{{\"k\":\"{}\"}}", "a".repeat(MAX_JSON_CHARS));
        assert!(avatar(&oversized).is_err());
        assert!(
            check(
                PARTICIPANTS_TABLE,
                participant_group::PROFILE,
                vec![t("   "), Field::Integer(1), t("{}")],
            )
            .is_err()
        );

        assert!(
            adapter
                .validate(TEAMMATES_TABLE, teammate_group::POLICY, &policy(None))
                .is_ok()
        );
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::POLICY,
                vec![Field::Null, t("custom")]
            )
            .is_err()
        );
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::POLICY,
                vec![t("policy-1"), Field::Null]
            )
            .is_err()
        );
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::POLICY,
                vec![t("policy-1"), t("reckless")]
            )
            .is_err()
        );
        assert!(check(TEAMMATES_TABLE, teammate_group::HOME, vec![Field::Null]).is_ok());
        assert!(check(TEAMMATES_TABLE, teammate_group::HOME, vec![t("")]).is_err());
        let instructions = "a".repeat(MAX_INSTRUCTIONS_CHARS + 1);
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::PROFILE,
                vec![t("Reviewer"), t(&instructions)]
            )
            .is_err()
        );

        let revision = |model, safety| {
            adapter.validate(
                POLICY_REVISIONS_TABLE,
                policy_revision_group::CREATED,
                &policy_revision(model, safety),
            )
        };
        assert!(revision("{\"model\":\"gpt\"}", "custom").is_ok());
        assert!(revision("\"gpt\"", "custom").is_err());
        assert!(revision("{}", "reckless").is_err());

        assert!(
            check(
                PROFILES_TABLE,
                profile_group::CREATED,
                vec![t("read_only"), t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                PROFILES_TABLE,
                profile_group::CREATED,
                vec![Field::Null, t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                PROFILES_TABLE,
                profile_group::CREATED,
                vec![t("root"), t(AT)]
            )
            .is_err()
        );
        assert!(
            adapter
                .validate(
                    PROFILES_TABLE,
                    profile_group::CURRENT,
                    &current(Some("edit"))
                )
                .is_ok()
        );

        let profile_revision = |flag_value, capability| {
            check(
                PROFILE_REVISIONS_TABLE,
                profile_revision_group::CREATED,
                vec![
                    t("profile-1"),
                    Field::Integer(flag_value),
                    Field::Integer(0),
                    t("from_grant"),
                    t(capability),
                    t(AT),
                ],
            )
        };
        assert!(profile_revision(1, "execute").is_ok());
        assert!(profile_revision(2, "execute").is_err());
        assert!(profile_revision(1, "admin").is_err());
    }

    fn optional(text: Option<&str>) -> Field {
        text.map_or(Field::Null, t)
    }

    fn membership(role: &str, removed: bool) -> Value {
        value(vec![t(role), optional(removed.then_some(AT))])
    }

    /// An AI channel membership access value with each capability set or cleared.
    fn channel_access(
        read_history: bool,
        participate: bool,
        from_grant: bool,
        approval: Option<&str>,
        scratch_approval: Option<&str>,
        removed: bool,
    ) -> Value {
        value(vec![
            t("profile-1"),
            Field::Integer(i64::from(read_history)),
            Field::Integer(0),
            Field::Integer(i64::from(participate)),
            Field::Integer(0),
            t(if from_grant { "from_grant" } else { "entire" }),
            Field::Integer(0),
            if from_grant {
                Field::Integer(3)
            } else {
                Field::Null
            },
            optional(approval),
            optional(scratch_approval),
            optional(removed.then_some(AT)),
        ])
    }

    fn grant(capability: &str, approval: Option<&str>, revoked: bool) -> Value {
        value(vec![
            t(capability),
            Field::Integer(0),
            Field::Integer(1),
            optional(approval),
            optional(revoked.then_some(AT)),
        ])
    }

    #[test]
    fn chat_membership_priorities_prefer_removal_then_demotion() {
        let adapter = ChatAdapter;
        let priority = |value: &Value| {
            adapter.priority(MEMBERSHIPS_TABLE, membership_group::MEMBERSHIP, value)
        };
        assert!(priority(&membership("owner", true)) > priority(&membership("member", false)));
        assert_eq!(
            priority(&membership("owner", true)),
            priority(&membership("member", true))
        );
        assert!(priority(&membership("member", false)) > priority(&membership("owner", false)));
        assert_eq!(
            adapter.priority(
                MEMBERSHIPS_TABLE,
                membership_group::CREATED,
                &value(vec![t(AT)])
            ),
            0
        );
    }

    #[test]
    fn chat_channel_membership_priorities_prefer_removal_then_fewer_capabilities_then_stricter_approval()
     {
        let adapter = ChatAdapter;
        let priority = |value: &Value| {
            adapter.priority(
                CHANNEL_MEMBERSHIPS_TABLE,
                channel_membership_group::ACCESS,
                value,
            )
        };
        let open = channel_access(true, true, false, Some("provider_custom"), None, false);
        let removed = channel_access(true, true, false, Some("provider_custom"), None, true);
        let strictest_live = channel_access(false, false, true, Some("ask"), Some("ask"), false);
        assert!(priority(&removed) > priority(&strictest_live));
        assert!(priority(&open) >= 1);
        assert!(priority(&strictest_live) < u8::MAX);

        // Clearing more capabilities wins over any approval override.
        let cleared = [
            channel_access(true, true, false, Some("ask"), Some("ask"), false),
            channel_access(false, true, false, None, None, false),
            channel_access(true, false, false, None, None, false),
        ];
        assert!(priority(&cleared[1]) > priority(&cleared[0]));
        assert!(priority(&cleared[2]) > priority(&cleared[0]));
        let more_cleared = [
            channel_access(true, true, false, Some("ask"), Some("ask"), false),
            channel_access(false, true, false, Some("ask"), Some("ask"), false),
            channel_access(false, false, false, Some("ask"), Some("ask"), false),
            channel_access(false, false, true, Some("ask"), Some("ask"), false),
        ];
        for (looser, stricter) in more_cleared.iter().zip(&more_cleared[1..]) {
            assert!(priority(stricter) > priority(looser));
        }
        assert!(
            priority(&channel_access(false, false, true, None, None, false))
                > priority(&channel_access(
                    false,
                    false,
                    false,
                    Some("ask"),
                    Some("ask"),
                    false
                ))
        );

        // With equal capabilities, the stricter approval override wins, and no override, which
        // inherits the teammate default, is the least strict.
        let approvals = [
            None,
            Some("provider_custom"),
            Some("unattended"),
            Some("auto_approve"),
            Some("ask"),
        ];
        for pair in approvals.windows(2) {
            let looser = channel_access(true, true, false, pair[0], Some("ask"), false);
            let stricter = channel_access(true, true, false, pair[1], None, false);
            assert!(priority(&stricter) > priority(&looser), "{pair:?}");
            let looser = channel_access(true, true, false, None, pair[0], false);
            let stricter = channel_access(true, true, false, None, pair[1], false);
            assert!(priority(&stricter) > priority(&looser), "scratch {pair:?}");
        }
    }

    #[test]
    fn chat_channel_membership_inherited_capabilities_rank_as_granted() {
        let adapter = ChatAdapter;
        let priority = |value: &Value| {
            adapter.priority(
                CHANNEL_MEMBERSHIPS_TABLE,
                channel_membership_group::ACCESS,
                value,
            )
        };
        let explicit = channel_access(false, false, true, None, None, false);
        let granted = channel_access(true, true, false, None, None, false);
        for inherit in [2, 4, 6] {
            let mut fields = explicit.fields().to_vec();
            fields[inherit] = Field::Integer(1);
            let inherited = value(fields);
            assert!(priority(&explicit) > priority(&inherited), "{inherit}");
            assert!(priority(&inherited) > priority(&granted), "{inherit}");
        }
        let mut fields = explicit.fields().to_vec();
        for inherit in [2, 4, 6] {
            fields[inherit] = Field::Integer(1);
        }
        assert_eq!(priority(&value(fields)), priority(&granted));
    }

    #[test]
    fn chat_folder_grant_priorities_prefer_revocation_then_lower_capability_then_stricter_approval()
    {
        let adapter = ChatAdapter;
        let priority =
            |value: &Value| adapter.priority(FOLDER_GRANTS_TABLE, folder_grant_group::GRANT, value);
        assert!(
            priority(&grant("publish", None, true)) > priority(&grant("none", Some("ask"), false))
        );
        for pair in FOLDER_CAPABILITIES.windows(2) {
            assert!(
                priority(&grant(pair[1], Some("provider_custom"), false))
                    > priority(&grant(pair[0], Some("ask"), false)),
                "{pair:?}"
            );
        }
        let approvals = [
            None,
            Some("provider_custom"),
            Some("unattended"),
            Some("auto_approve"),
            Some("ask"),
        ];
        for pair in approvals.windows(2) {
            assert!(
                priority(&grant("edit", pair[1], false)) > priority(&grant("edit", pair[0], false)),
                "{pair:?}"
            );
        }
        assert!(priority(&grant("none", Some("ask"), false)) < u8::MAX);

        // An inherited capability follows the profile's maximum, so it ranks as publishing.
        let mut inherited = grant("none", Some("ask"), false).fields().to_vec();
        inherited[1] = Field::Integer(1);
        assert_eq!(
            priority(&value(inherited)),
            priority(&grant("publish", Some("ask"), false))
        );
    }

    #[test]
    fn chat_channel_and_conversation_archive_outranks_unarchive() {
        let adapter = ChatAdapter;
        for (table, group) in [
            (CHANNELS_TABLE, channel_group::LIFECYCLE),
            (CONVERSATIONS_TABLE, conversation_group::LIFECYCLE),
        ] {
            let priority = |archived: Option<&str>| {
                adapter.priority(table, group, &value(vec![optional(archived)]))
            };
            assert!(priority(Some(AT)) > priority(None));
        }
        assert_eq!(
            adapter.priority(
                CHANNELS_TABLE,
                channel_group::DETAILS,
                &value(vec![t("general"), t("")])
            ),
            0
        );
    }

    #[test]
    fn chat_conversation_access_validation_mirrors_the_table_checks() {
        let adapter = ChatAdapter;
        let check = |table, group, fields| adapter.validate(table, group, &value(fields));

        assert!(
            check(
                CONVERSATIONS_TABLE,
                conversation_group::CREATED,
                vec![t("project-1"), t("channel"), t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                CONVERSATIONS_TABLE,
                conversation_group::CREATED,
                vec![t("project-1"), t("thread"), t(AT)]
            )
            .is_err()
        );
        assert!(
            adapter
                .validate(
                    MEMBERSHIPS_TABLE,
                    membership_group::MEMBERSHIP,
                    &membership("member", true)
                )
                .is_ok()
        );
        assert!(
            adapter
                .validate(
                    MEMBERSHIPS_TABLE,
                    membership_group::MEMBERSHIP,
                    &membership("admin", false)
                )
                .is_err()
        );

        let access = |boundary: &str, ordinal: Field, approval: &str| {
            let mut fields = channel_access(true, true, false, None, None, false)
                .fields()
                .to_vec();
            fields[5] = t(boundary);
            fields[7] = ordinal;
            fields[8] = t(approval);
            check(
                CHANNEL_MEMBERSHIPS_TABLE,
                channel_membership_group::ACCESS,
                fields,
            )
        };
        assert!(access("entire", Field::Null, "ask").is_ok());
        assert!(access("from_grant", Field::Integer(1), "unattended").is_ok());
        assert!(access("from_grant", Field::Null, "ask").is_err());
        assert!(access("from_grant", Field::Integer(0), "ask").is_err());
        assert!(access("entire", Field::Integer(2), "ask").is_err());
        assert!(access("entire", Field::Null, "always").is_err());

        assert!(
            adapter
                .validate(
                    FOLDER_GRANTS_TABLE,
                    folder_grant_group::GRANT,
                    &grant("execute", Some("auto_approve"), true)
                )
                .is_ok()
        );
        assert!(
            adapter
                .validate(
                    FOLDER_GRANTS_TABLE,
                    folder_grant_group::GRANT,
                    &grant("admin", None, false)
                )
                .is_err()
        );

        let details = |name: &str, topic: &str| {
            check(
                CHANNELS_TABLE,
                channel_group::DETAILS,
                vec![t(name), t(topic)],
            )
        };
        assert!(details("general", "").is_ok());
        assert!(details("", "").is_err());
        assert!(details(&"n".repeat(MAX_CHANNEL_NAME_CHARS + 1), "").is_err());
        assert!(details("general", &"t".repeat(MAX_CHANNEL_TOPIC_CHARS + 1)).is_err());
    }
}
