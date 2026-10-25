//! Chat adapter: the value checks that mirror the Chat identity and policy table checks, and the
//! priorities of their coupled groups.
//!
//! A teammate's current policy and an access profile's current revision are coupled with a copy
//! of the revision's restrictiveness. Concurrent edits keep the more restrictive one: asking
//! for approval outranks a custom mode, which outranks approving for the user, which outranks
//! full access, and a lower maximum folder capability outranks a higher one. An edit made after
//! seeing the other still wins by causality.

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::chat::{
    ADAPTER, PARTICIPANTS_TABLE, POLICY_REVISIONS_TABLE, PROFILE_REVISIONS_TABLE, PROFILES_TABLE,
    TEAMMATES_TABLE, participant_group, policy_revision_group, profile_group,
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
}
