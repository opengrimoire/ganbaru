use ganbaru_sync_contracts::{Change, ChangeAction};

use super::*;
use crate::codec::OwnedRow;
use crate::{Engine, validate};

const AT: &str = "2026-10-24T10:51:41.000Z";

fn value(fields: Vec<Field>) -> Value {
    Value::new(fields).unwrap()
}

fn t(text: &str) -> Field {
    Field::Text(text.to_owned())
}

fn policy_revision(model: &str, mode: &str) -> Value {
    value(vec![
        t("agent-1"),
        Field::Integer(1),
        t(model),
        Field::Integer(1),
        t("{}"),
        t(mode),
        t(AT),
    ])
}

#[test]
fn chat_access_priorities_prefer_the_narrower_level_at_every_scope() {
    let adapter = ChatAdapter;
    for (table, group) in [
        (APP_SETTINGS_TABLE, app_settings_group::ACCESS),
        (PROJECT_SETTINGS_TABLE, project_settings_group::ACCESS),
        (CHANNELS_TABLE, channel_group::AGENT_ACCESS),
    ] {
        let priority =
            |access: Option<&str>| adapter.priority(table, group, &value(vec![optional(access)]));
        assert!(priority(Some("talk")) > priority(Some("read")));
        assert!(priority(Some("read")) > priority(Some("ask")));
        assert!(priority(Some("ask")) > priority(Some("own")));
        assert!(
            priority(Some("own")) > priority(None),
            "an explicit level outranks inheriting"
        );
    }
    assert_eq!(
        adapter.priority(
            AGENTS_TABLE,
            agent_group::POLICY,
            &value(vec![t("policy-1")])
        ),
        0,
        "the default model is a plain register"
    );
    assert_eq!(
        adapter.priority(
            CHANNELS_TABLE,
            channel_group::AGENT_INSTRUCTIONS,
            &value(vec![t("Be brief")])
        ),
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
            vec![t("ai_agent"), t(AT)]
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

    let agent = |family: &str| check(AGENTS_TABLE, agent_group::CREATED, vec![t(family), t(AT)]);
    assert!(agent("codex").is_ok());
    assert!(agent("  ").is_err());
    assert!(agent(&"p".repeat(MAX_PROVIDER_INSTANCE_CHARS + 1)).is_err());
    assert!(check(AGENTS_TABLE, agent_group::POLICY, vec![Field::Null]).is_ok());
    assert!(check(AGENTS_TABLE, agent_group::POLICY, vec![t("policy-1")]).is_ok());
    assert!(
        check(
            AGENTS_TABLE,
            agent_group::POLICY,
            vec![t("policy-1"), t("custom")]
        )
        .is_err()
    );

    let revision = |model, mode| {
        adapter.validate(
            POLICY_REVISIONS_TABLE,
            policy_revision_group::CREATED,
            &policy_revision(model, mode),
        )
    };
    assert!(revision("{\"model\":\"gpt\"}", "build").is_ok());
    assert!(revision("\"gpt\"", "build").is_err());
    assert!(revision("{}", "plan").is_err());

    let too_long = "a".repeat(MAX_INSTRUCTIONS_CHARS + 1);
    for (table, access, instructions, inherits) in [
        (
            APP_SETTINGS_TABLE,
            app_settings_group::ACCESS,
            app_settings_group::INSTRUCTIONS,
            false,
        ),
        (
            PROJECT_SETTINGS_TABLE,
            project_settings_group::ACCESS,
            project_settings_group::INSTRUCTIONS,
            true,
        ),
        (
            CHANNELS_TABLE,
            channel_group::AGENT_ACCESS,
            channel_group::AGENT_INSTRUCTIONS,
            true,
        ),
    ] {
        for level in AGENT_ACCESS_LEVELS {
            assert!(check(table, access, vec![t(level)]).is_ok(), "{level}");
        }
        assert!(check(table, access, vec![t("publish")]).is_err());
        assert_eq!(
            check(table, access, vec![Field::Null]).is_ok(),
            inherits,
            "only a project or channel inherits"
        );
        assert!(check(table, instructions, vec![t("")]).is_ok());
        assert!(check(table, instructions, vec![t(&too_long)]).is_err());
    }
}

fn optional(text: Option<&str>) -> Field {
    text.map_or(Field::Null, t)
}

fn membership(role: &str, removed: bool) -> Value {
    value(vec![t(role), optional(removed.then_some(AT))])
}

#[test]
fn chat_membership_priorities_prefer_removal_then_demotion() {
    let adapter = ChatAdapter;
    let priority =
        |value: &Value| adapter.priority(MEMBERSHIPS_TABLE, membership_group::MEMBERSHIP, value);
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
fn chat_channel_membership_priorities_prefer_removal() {
    let adapter = ChatAdapter;
    let priority = |removed: Option<&str>| {
        adapter.priority(
            CHANNEL_MEMBERSHIPS_TABLE,
            channel_membership_group::PRESENCE,
            &value(vec![optional(removed)]),
        )
    };
    assert!(priority(Some(AT)) > priority(None));
    assert_eq!(
        adapter.priority(
            CHANNEL_MEMBERSHIPS_TABLE,
            channel_membership_group::MODEL,
            &value(vec![Field::Integer(1), t("{}")]),
        ),
        0,
        "the channel's model is a plain register"
    );
}

#[test]
fn chat_push_approval_revocation_outranks_approval() {
    let adapter = ChatAdapter;
    let priority = |revoked: Option<&str>| {
        adapter.priority(
            PUSH_APPROVALS_TABLE,
            push_approval_group::APPROVAL,
            &value(vec![t("participant:local-owner"), t(AT), optional(revoked)]),
        )
    };
    assert!(priority(Some(AT)) > priority(None));
}

#[test]
fn chat_remembered_approval_revocation_outranks_approval() {
    let adapter = ChatAdapter;
    let priority = |revoked: Option<&str>| {
        adapter.priority(
            REMEMBERED_APPROVALS_TABLE,
            remembered_approval_group::APPROVAL,
            &value(vec![
                t("participant:local-owner"),
                t(AT),
                optional(revoked),
                t("own"),
            ]),
        )
    };
    assert!(priority(Some(AT)) > priority(None));
}

#[test]
fn chat_remembered_approvals_name_a_known_kind_a_printable_subject_and_an_access() {
    let check = |group, fields: Vec<Field>| {
        ChatAdapter.validate(REMEMBERED_APPROVALS_TABLE, group, &value(fields))
    };
    let identity = |kind: &str, subject: &str| {
        check(
            remembered_approval_group::IDENTITY,
            vec![t("conversation-1"), t("agent-1"), t(kind), t(subject)],
        )
    };
    assert!(identity("url_origin", "https://docs.example.com").is_ok());
    assert!(identity("command", "pnpm test").is_ok());
    assert!(identity("tool", "mcp__docs__search").is_ok());
    assert!(identity("path", "/etc").is_err());
    assert!(identity("command", "").is_err());
    assert!(identity("command", "pnpm test\nrm -rf .").is_err());

    let approval = |revoked: Field, access: &str| {
        check(
            remembered_approval_group::APPROVAL,
            vec![t("participant:local-owner"), t(AT), revoked, t(access)],
        )
    };
    assert!(approval(Field::Null, "ask").is_ok());
    assert!(approval(t(AT), "talk").is_ok());
    assert!(approval(Field::Null, "everything").is_err());
    assert!(approval(t("yesterday"), "ask").is_err());
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

/// An assignment STATE value: `state` with its settle time when final, and `review` as
/// outcome and reviewer.
fn assignment_state(state: &str, review: Option<(&str, &str)>) -> Value {
    let settled = matches!(state, "completed" | "failed" | "cancelled");
    value(vec![
        t(state),
        Field::Null,
        optional(settled.then_some(AT)),
        optional(review.map(|(outcome, _)| outcome)),
        optional(review.map(|_| AT)),
        optional(review.map(|(_, reviewer)| reviewer)),
    ])
}

#[test]
fn chat_assignment_priorities_prefer_the_most_restrictive_ending() {
    let adapter = ChatAdapter;
    let priority =
        |state: &Value| adapter.priority(ASSIGNMENTS_TABLE, assignment_group::STATE, state);
    let accepted = assignment_state("completed", Some(("accepted", "participant:local")));
    let ladder = [
        priority(&assignment_state("cancelled", None)),
        priority(&assignment_state("failed", None)),
        priority(&accepted),
        priority(&assignment_state("ready_for_review", None)),
        priority(&assignment_state("working", None)),
    ];
    assert!(
        ladder.windows(2).all(|pair| pair[0] > pair[1]),
        "{ladder:?}"
    );
    assert_eq!(
        priority(&assignment_state("completed", None)),
        priority(&accepted),
        "a review never changes the rank of its completion"
    );
    for active in [
        "queued",
        "working",
        "waiting_for_answer",
        "waiting_for_approval",
    ] {
        assert_eq!(
            priority(&assignment_state(active, None)),
            ladder[4],
            "{active}"
        );
    }
    assert!(ladder[0] < ENDED);
}

#[test]
fn a_replicated_scheduled_failure_carries_its_error() {
    let adapter = ChatAdapter;
    let check = |failed_at: Field, error: Field| {
        adapter.validate(
            SCHEDULED_TABLE,
            scheduled_group::FAILURE,
            &value(vec![failed_at, error]),
        )
    };
    assert!(check(Field::Null, Field::Null).is_ok());
    assert!(check(Field::Null, t("Retrying after a network error")).is_ok());
    assert!(check(t(AT), t("The agent left the channel")).is_ok());
    assert!(check(t(AT), Field::Null).is_err());
}

#[test]
fn chat_assignment_settled_time_is_present_exactly_on_settled_work() {
    let adapter = ChatAdapter;
    let check =
        |fields| adapter.validate(ASSIGNMENTS_TABLE, assignment_group::STATE, &value(fields));
    for state in ASSIGNMENT_STATES {
        let settled = SETTLED_ASSIGNMENT_STATES.contains(&state);
        let mut flipped = assignment_state(state, None).fields().to_vec();
        flipped[2] = if settled { Field::Null } else { t(AT) };
        assert!(
            check(flipped).is_err(),
            "{state} with its settled time flipped"
        );
    }
    let mut malformed = assignment_state("failed", None).fields().to_vec();
    malformed[2] = t("2026-10-24");
    assert!(check(malformed).is_err());
}

#[test]
fn chat_assignment_reviews_need_all_fields_on_completed_work() {
    let adapter = ChatAdapter;
    let check =
        |fields| adapter.validate(ASSIGNMENTS_TABLE, assignment_group::STATE, &value(fields));
    let reviewer = "participant:local";

    for outcome in ASSIGNMENT_REVIEW_OUTCOMES {
        let reviewed = assignment_state("completed", Some((outcome, reviewer)));
        assert!(check(reviewed.fields().to_vec()).is_ok(), "{outcome}");
    }
    for state in ASSIGNMENT_STATES {
        assert!(
            check(assignment_state(state, None).fields().to_vec()).is_ok(),
            "{state}"
        );
        if state != REVIEWED_ASSIGNMENT_STATE {
            let reviewed = assignment_state(state, Some(("accepted", reviewer)));
            assert!(
                check(reviewed.fields().to_vec()).is_err(),
                "review on {state}"
            );
        }
    }
    let complete = assignment_state("completed", Some(("accepted", reviewer)));
    for missing in 3..6 {
        let mut partial = complete.fields().to_vec();
        partial[missing] = Field::Null;
        assert!(check(partial).is_err(), "review without field {missing}");
    }
    let mut unknown = complete.fields().to_vec();
    unknown[3] = t("approved");
    assert!(check(unknown).is_err());
    let mut late = complete.fields().to_vec();
    late[4] = t("2026-10-24 10:51:41");
    assert!(check(late).is_err());
    let mut anonymous = complete.fields().to_vec();
    anonymous[5] = t("");
    assert!(check(anonymous).is_err());
    let mut longest = complete.fields().to_vec();
    longest[5] = t(&"p".repeat(MAX_PARTICIPANT_ID_BYTES));
    assert!(check(longest).is_ok());
    for reviewer in [
        "p".repeat(MAX_PARTICIPANT_ID_BYTES + 1),
        "é".repeat(MAX_PARTICIPANT_ID_BYTES / 2 + 1),
        "participant:\tlocal".to_owned(),
        "participant:local\n".to_owned(),
        "participant:\u{85}local".to_owned(),
    ] {
        let mut invalid = complete.fields().to_vec();
        invalid[5] = t(&reviewer);
        assert!(check(invalid).is_err(), "{reviewer:?}");
    }
    let mut short = complete.fields().to_vec();
    short.truncate(3);
    assert!(
        check(short).is_err(),
        "the state group carries its review fields"
    );
}

fn delegation(depth: i64, ceiling: &str) -> Vec<Field> {
    vec![
        t("assignment-parent"),
        Field::Integer(depth),
        t("Summarize the failing tests"),
        t("A list of failing tests with causes"),
        t(ceiling),
    ]
}

/// An assignment CREATED value carrying the model `version` and `data` and the resolved
/// `instructions`.
fn assignment_created(version: Field, data: Field, instructions: &str) -> Vec<Field> {
    vec![
        t("thread-1"),
        t("agent-1"),
        t("item-1"),
        Field::Null,
        t(AT),
        version,
        data,
        t(instructions),
    ]
}

#[test]
fn chat_assignment_delegation_is_complete_bounded_and_ceiling_checked() {
    let adapter = ChatAdapter;
    let check = |fields| {
        adapter.validate(
            ASSIGNMENTS_TABLE,
            assignment_group::DELEGATION,
            &value(fields),
        )
    };
    let ceiling = r#"[{"workingFolderId":"folder-1","capability":"edit"},{"workingFolderId":"folder-2","capability":"read"}]"#;

    let undelegated = vec![
        Field::Null,
        Field::Integer(0),
        Field::Null,
        Field::Null,
        Field::Null,
    ];
    assert!(check(undelegated).is_ok());
    for depth in 1..=MAX_DELEGATION_DEPTH {
        assert!(check(delegation(depth, ceiling)).is_ok(), "depth {depth}");
    }
    assert!(
        check(delegation(1, "[]")).is_ok(),
        "an empty ceiling grants no folder"
    );
    for capability in DELEGATION_CEILING_CAPABILITIES {
        let single = format!(r#"[{{"workingFolderId":"folder-1","capability":"{capability}"}}]"#);
        assert!(check(delegation(1, &single)).is_ok(), "{capability}");
    }

    for depth in [0, MAX_DELEGATION_DEPTH + 1, -1] {
        assert!(check(delegation(depth, ceiling)).is_err(), "depth {depth}");
    }
    let complete = delegation(1, ceiling);
    for missing in [0, 2, 3, 4] {
        let mut partial = complete.clone();
        partial[missing] = Field::Null;
        assert!(
            check(partial).is_err(),
            "delegation without field {missing}"
        );
        let mut stray = vec![
            Field::Null,
            Field::Integer(0),
            Field::Null,
            Field::Null,
            Field::Null,
        ];
        stray[missing] = complete[missing].clone();
        assert!(check(stray).is_err(), "undelegated with field {missing}");
    }
    let mut blank = complete.clone();
    blank[2] = t("  ");
    assert!(check(blank).is_err());
    let mut long_objective = complete.clone();
    long_objective[2] = t(&"o".repeat(MAX_DELEGATION_OBJECTIVE_CHARS + 1));
    assert!(check(long_objective).is_err());
    let mut long_result = complete.clone();
    long_result[3] = t(&"r".repeat(MAX_DELEGATION_EXPECTED_RESULT_CHARS + 1));
    assert!(check(long_result).is_err());

    for invalid in [
        "{}",
        "not json",
        r#"[{"workingFolderId":"folder-1","capability":"admin"}]"#,
        r#"[{"workingFolderId":"","capability":"read"}]"#,
        r#"[{"workingFolderId":"folder\n1","capability":"read"}]"#,
        r#"[{"workingFolderId":"folder-1"}]"#,
        r#"[{"workingFolderId":"folder-1","capability":"read","extra":true}]"#,
        r#"[{"workingFolderId":"folder-1","capability":"read"},{"workingFolderId":"folder-1","capability":"edit"}]"#,
    ] {
        assert!(check(delegation(1, invalid)).is_err(), "{invalid}");
    }
    let mut short = complete;
    short.truncate(4);
    assert!(
        check(short).is_err(),
        "the delegation group carries five fields"
    );
}

#[test]
fn chat_assignment_fixes_a_whole_model_and_bounded_instructions() {
    let adapter = ChatAdapter;
    let check =
        |fields| adapter.validate(ASSIGNMENTS_TABLE, assignment_group::CREATED, &value(fields));
    let selection = r#"{"modelId":"gpt-5","modelOptions":{"effort":"high"}}"#;

    assert!(check(assignment_created(Field::Integer(1), t(selection), "")).is_ok());
    let longest = "i".repeat(MAX_RESOLVED_INSTRUCTIONS_CHARS);
    assert!(
        check(assignment_created(
            Field::Integer(1),
            t(selection),
            &longest
        ))
        .is_ok()
    );
    let too_long = "i".repeat(MAX_RESOLVED_INSTRUCTIONS_CHARS + 1);
    assert!(
        check(assignment_created(
            Field::Integer(1),
            t(selection),
            &too_long
        ))
        .is_err()
    );
    assert!(
        check(assignment_created(Field::Null, Field::Null, "")).is_err(),
        "every assignment fixes the model it runs"
    );
    assert!(check(assignment_created(Field::Integer(1), Field::Null, "")).is_err());
    assert!(check(assignment_created(Field::Integer(0), t(selection), "")).is_err());
    assert!(check(assignment_created(Field::Integer(1), t("[]"), "")).is_err());
    let mut short = assignment_created(Field::Integer(1), t(selection), "");
    short.truncate(7);
    assert!(
        check(short).is_err(),
        "the created group carries the instructions"
    );

    let label = |label: Field| {
        adapter.validate(
            ASSIGNMENTS_TABLE,
            assignment_group::MODEL_LABEL,
            &value(vec![label]),
        )
    };
    assert!(label(Field::Null).is_ok());
    assert!(label(t("GPT-5 Codex")).is_ok());
    assert!(label(t("")).is_err());
    assert!(label(t(&"m".repeat(MAX_MODEL_LABEL_CHARS + 1))).is_err());
}

#[test]
fn chat_semantic_updates_accept_the_delegation_and_proposal_kinds() {
    let adapter = ChatAdapter;
    let update = |kind: &str| {
        value(vec![
            t("assignment-1"),
            t(kind),
            Field::Integer(1),
            t("{}"),
            t(AT),
        ])
    };
    for kind in ["delegation", "schedule_proposal", "note_proposal"] {
        assert!(SEMANTIC_UPDATE_KINDS.contains(&kind));
        assert!(
            adapter
                .validate(
                    SEMANTIC_UPDATES_TABLE,
                    semantic_update_group::CREATED,
                    &update(kind)
                )
                .is_ok(),
            "{kind}"
        );
    }
    assert!(
        adapter
            .validate(
                SEMANTIC_UPDATES_TABLE,
                semantic_update_group::CREATED,
                &update("summary")
            )
            .is_err()
    );
}

#[test]
fn chat_channel_model_is_whole_or_absent() {
    let adapter = ChatAdapter;
    let check = |fields| {
        adapter.validate(
            CHANNEL_MEMBERSHIPS_TABLE,
            channel_membership_group::MODEL,
            &value(fields),
        )
    };
    let selection = r#"{"providerManagedModel":true,"modelOptions":[]}"#;

    assert!(check(vec![Field::Null, Field::Null]).is_ok());
    assert!(check(vec![Field::Integer(1), t(selection)]).is_ok());
    assert!(check(vec![Field::Integer(1), Field::Null]).is_err());
    assert!(check(vec![Field::Null, t(selection)]).is_err());
    assert!(check(vec![Field::Integer(1), t("[]")]).is_err());
    assert!(
        adapter
            .validate(
                PARTICIPANT_TARGETS_TABLE,
                timeline_group::CREATED,
                &value(vec![t("agent-1")]),
            )
            .is_ok()
    );
    assert!(
        adapter
            .validate(
                PARTICIPANT_TARGETS_TABLE,
                timeline_group::CREATED,
                &value(vec![t("agent-1"), Field::Integer(1), t(selection)]),
            )
            .is_err(),
        "a mention carries no model"
    );
}

#[test]
fn chat_recurrence_rules_set_exactly_the_fields_their_kind_needs() {
    let adapter = ChatAdapter;
    let rule = |fields| adapter.validate(RECURRENCES_TABLE, recurrence_group::RULE, &value(fields));
    let interval = |minutes: i64| {
        vec![
            t("interval"),
            Field::Integer(minutes),
            Field::Null,
            Field::Null,
            t("America/Mexico_City"),
            t("defer"),
        ]
    };
    let fixed = |minute: i64, mask: i64, zone: &str, policy: &str| {
        vec![
            t("fixed_time"),
            Field::Null,
            Field::Integer(minute),
            Field::Integer(mask),
            t(zone),
            t(policy),
        ]
    };

    assert!(rule(interval(60)).is_ok());
    assert!(rule(interval(43_200)).is_ok());
    assert!(
        rule(interval(59)).is_err(),
        "intervals are at least an hour"
    );
    assert!(
        rule(interval(43_201)).is_err(),
        "intervals are at most 30 days"
    );
    assert!(rule(fixed(0, 1, "Europe/Madrid", "ignore")).is_ok());
    assert!(rule(fixed(1439, 127, "Europe/Madrid", "defer")).is_ok());
    assert!(rule(fixed(1440, 127, "Europe/Madrid", "defer")).is_err());
    assert!(
        rule(fixed(540, 0, "Europe/Madrid", "defer")).is_err(),
        "no weekday"
    );
    assert!(rule(fixed(540, 128, "Europe/Madrid", "defer")).is_err());
    assert!(rule(fixed(540, 127, "  ", "defer")).is_err(), "blank zone");
    assert!(rule(fixed(540, 127, &"z".repeat(256), "defer")).is_err());
    assert!(rule(fixed(540, 127, "Europe/Madrid", "skip")).is_err());

    let mut mixed = interval(60);
    mixed[2] = Field::Integer(540);
    assert!(rule(mixed).is_err(), "an interval rule sets no local time");
    let mut mixed = fixed(540, 127, "Europe/Madrid", "defer");
    mixed[1] = Field::Integer(60);
    assert!(rule(mixed).is_err(), "a fixed time rule sets no interval");
    let mut unknown = interval(60);
    unknown[0] = t("cron");
    assert!(rule(unknown).is_err());

    let progress = |fields| {
        adapter.validate(
            RECURRENCES_TABLE,
            recurrence_group::PROGRESS,
            &value(fields),
        )
    };
    assert!(progress(vec![t(AT), Field::Integer(0)]).is_ok());
    assert!(progress(vec![t(AT), Field::Integer(-1)]).is_err());
    assert!(progress(vec![t("tomorrow"), Field::Integer(3)]).is_err());
}

#[test]
fn chat_input_delivery_prefers_what_happened_over_a_cancel() {
    let adapter = ChatAdapter;
    let delivery = |state: &str, revision: Option<&str>| {
        let delivered_at = (state == DELIVERED_STATE).then_some(AT);
        value(vec![t(state), optional(delivered_at), optional(revision)])
    };
    let priority = |state: &Value| adapter.priority(INPUTS_TABLE, input_group::DELIVERY, state);
    let ladder = [
        priority(&delivery("delivered", Some("revision-1"))),
        priority(&delivery("failed", None)),
        priority(&delivery("ignored", None)),
        priority(&delivery("pending", None)),
    ];
    assert!(
        ladder.windows(2).all(|pair| pair[0] > pair[1]),
        "{ladder:?}"
    );
    assert!(ladder[0] < ENDED);
    assert_eq!(priority(&delivery("unknown", None)), 0);
}

#[test]
fn chat_input_delivered_revision_needs_a_delivered_input() {
    let adapter = ChatAdapter;
    let check = |fields| adapter.validate(INPUTS_TABLE, input_group::DELIVERY, &value(fields));

    assert!(check(vec![t("delivered"), t(AT), t("revision-1")]).is_ok());
    assert!(
        check(vec![t("delivered"), t(AT), Field::Null]).is_ok(),
        "a delivery need not name a revision"
    );
    for state in DELIVERY_STATES {
        assert!(
            check(vec![t(state), Field::Null, Field::Null]).is_ok(),
            "{state}"
        );
        if state != DELIVERED_STATE {
            assert!(
                check(vec![t(state), Field::Null, t("revision-1")]).is_err(),
                "revision on {state}"
            );
        }
    }
    assert!(check(vec![t("delivered"), t(AT)]).is_err());
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

    let presence = |removed: Field| {
        check(
            CHANNEL_MEMBERSHIPS_TABLE,
            channel_membership_group::PRESENCE,
            vec![removed],
        )
    };
    assert!(presence(Field::Null).is_ok());
    assert!(presence(t(AT)).is_ok());
    assert!(presence(t("yesterday")).is_err());

    let approval = |fields| check(PUSH_APPROVALS_TABLE, push_approval_group::IDENTITY, fields);
    let identity = |url: &str, branch: &str| {
        vec![
            t("conversation-1"),
            t("agent-1"),
            t("folder-1"),
            t(url),
            t(branch),
        ]
    };
    assert!(approval(identity("git@example.com:team/app.git", "main")).is_ok());
    assert!(approval(identity("git@example.com:team/app.git", "my branch")).is_err());
    assert!(
        approval(identity("git@example.com:team/app.git", "main")[1..].to_vec()).is_err(),
        "an approval names its channel"
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

#[test]
fn chat_channel_reference_ranges_need_ordered_well_formed_keys() {
    let adapter = ChatAdapter;
    let range = |lower: (&str, &str), high: (&str, &str)| {
        adapter.validate(
            CHANNEL_TARGETS_TABLE,
            timeline_group::CREATED,
            &value(vec![
                t("reference-1"),
                t("channel-1"),
                t("channel-2"),
                t(lower.0),
                t(lower.1),
                t(high.0),
                t(high.1),
                t("revision-1"),
            ]),
        )
    };
    assert!(range(("a0", ""), ("a0", "item-1")).is_ok());
    assert!(range(("a0", "item-1"), ("a0", "item-1")).is_ok());
    assert!(range(("a0", "item-1"), ("a1", "")).is_ok());
    assert!(range(("a1", ""), ("a0", "item-1")).is_err());
    assert!(range(("a0", "item-2"), ("a0", "item-1")).is_err());
    assert!(range(("!", ""), ("a0", "item-1")).is_err());
    assert!(range(("a0", ""), ("a", "item-1")).is_err());

    let empty = adapter.validate(
        CHANNEL_TARGETS_TABLE,
        timeline_group::CREATED,
        &value(vec![
            t("reference-1"),
            t("channel-1"),
            t("channel-2"),
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
        ]),
    );
    assert!(empty.is_ok());
}

fn attachment_row(kind: &str, mime: &str, size: i64) -> OwnedRow {
    OwnedRow::flat(vec![
        t("attachment-1"),
        t("working-folder:project-1"),
        t(kind),
        t("screen.png"),
        t(mime),
        Field::Integer(size),
        t(&"a".repeat(64)),
    ])
}

fn attachments_value(rows: &[OwnedRow]) -> Value {
    value(vec![Field::Blob(
        codec::encode(&ATTACHMENT_REFERENCES, rows).unwrap(),
    )])
}

#[test]
fn chat_attachments_round_trip_and_mirror_the_reference_checks() {
    let adapter = ChatAdapter;
    let rows = vec![
        attachment_row("image", "image/png", 8),
        attachment_row("text_snippet", "text/plain", 0),
    ];
    let encoded = codec::encode(&ATTACHMENT_REFERENCES, &rows).unwrap();
    assert_eq!(
        codec::decode(&ATTACHMENT_REFERENCES, &encoded).unwrap(),
        rows
    );

    let check = |rows: &[OwnedRow]| {
        adapter.validate(
            REVISIONS_TABLE,
            timeline_group::ATTACHMENTS,
            &attachments_value(rows),
        )
    };
    assert_eq!(check(&rows), Ok(()));
    assert_eq!(check(&[]), Ok(()));
    let limit = i64::from(MAX_ATTACHMENT_BYTES);
    assert_eq!(
        check(&[attachment_row("image", "image/gif", limit)]),
        Ok(())
    );
    let oversized = attachment_row("image", "image/png", limit + 1);
    assert!(codec::encode(&ATTACHMENT_REFERENCES, &[oversized]).is_err());
    assert!(check(&[attachment_row("image", "image/png", 0)]).is_err());
    assert!(check(&[attachment_row("text_snippet", "image/png", 8)]).is_err());
    assert!(check(&[attachment_row("image", "application/pdf", 8)]).is_err());

    let mut upper_digest = attachment_row("image", "image/png", 8);
    upper_digest.fields[6] = t(&"A".repeat(64));
    assert!(check(&[upper_digest]).is_err());
    let mut long_id = attachment_row("image", "image/png", 8);
    long_id.fields[0] = t(&"x".repeat(MAX_ATTACHMENT_ID_CHARS + 1));
    assert!(codec::encode(&ATTACHMENT_REFERENCES, &[long_id]).is_err());
    let too_many = vec![attachment_row("image", "image/png", 8); MAX_MESSAGE_ATTACHMENTS + 1];
    assert!(codec::encode(&ATTACHMENT_REFERENCES, &too_many).is_err());
}

#[test]
fn chat_attachment_asset_refs_skip_empty_snippets() {
    let adapter = ChatAdapter;
    assert_eq!(
        adapter.asset_groups(REVISIONS_TABLE),
        &[timeline_group::ATTACHMENTS]
    );
    let refs = adapter
        .asset_refs(
            REVISIONS_TABLE,
            timeline_group::ATTACHMENTS,
            &attachments_value(&[
                attachment_row("image", "image/png", 8),
                attachment_row("text_snippet", "text/plain", 0),
            ]),
        )
        .unwrap();
    assert_eq!(refs.len(), 1, "an empty text snippet has no file");
    assert_eq!(refs[0].class().name, "chat/attachments");
    assert_eq!(refs[0].content_type(), "image/png");
    assert_eq!(refs[0].byte_size(), 8);
    assert_eq!(refs[0].file_name(), format!("{}.png", "a".repeat(64)));
    assert!(
        adapter
            .asset_refs(
                REVISIONS_TABLE,
                timeline_group::CREATED,
                &value(vec![t("body")])
            )
            .unwrap()
            .is_empty()
    );
}

/// A create of AI agent participant `id` created at `created_at`.
fn participant_create(id: &str, created_at: &str) -> Change {
    let group = |group, fields| GroupValue::new(PARTICIPANTS_TABLE, group, value(fields));
    Change {
        table: PARTICIPANTS_TABLE,
        row: RowKey::new(id).unwrap(),
        action: ChangeAction::Create,
        groups: vec![
            group(
                participant_group::CREATED,
                vec![t("ai_agent"), t(created_at)],
            ),
            group(
                participant_group::PROFILE,
                vec![t("Reviewer"), Field::Integer(1), t("{}")],
            ),
            group(participant_group::LIFECYCLE, vec![Field::Null, Field::Null]),
            group(participant_group::UPDATED, vec![t(AT)]),
        ],
        replaced_by: None,
        cause: None,
    }
}

/// Text the stored-row readers refuse, each long enough for the table length check.
const NOT_UTC_TIMESTAMPS: [&str; 5] = [
    "not a timestamp at all, honestly",
    "2026-10-24t10:51:41.000Z",
    "2026-10-24 10:51:41.000Z",
    "2026-10-24T10:51:41.000+02:00",
    "2026-10-24T10:51:60.000Z",
];

#[test]
fn chat_timestamps_must_be_rfc3339_utc() {
    let engine = Engine::vault();
    let adapter = ChatAdapter;
    for bad in NOT_UTC_TIMESTAMPS {
        let forged = participant_create("participant:reviewer", bad);
        assert!(validate::change(&engine, &forged).is_err(), "created {bad}");

        for (table, group) in [
            (PARTICIPANTS_TABLE, participant_group::UPDATED),
            (CHANNELS_TABLE, channel_group::UPDATED),
            (CHANNELS_TABLE, channel_group::LIFECYCLE),
            (CONVERSATIONS_TABLE, conversation_group::ACTIVITY),
            (MESSAGES_TABLE, message_group::LIFECYCLE),
            (CONVERSATION_CURSORS_TABLE, read_cursor_group::UPDATED),
            (SCHEDULED_TABLE, scheduled_group::UPDATED),
            (ASSIGNMENTS_TABLE, assignment_group::UPDATED),
        ] {
            assert!(
                adapter
                    .validate(table, group, &value(vec![t(bad)]))
                    .is_err(),
                "{table:?} {group:?} {bad}"
            );
        }
        for lifecycle in [vec![t(bad), Field::Null], vec![t(AT), t(bad)]] {
            assert!(
                adapter
                    .validate(
                        PARTICIPANTS_TABLE,
                        participant_group::LIFECYCLE,
                        &value(lifecycle)
                    )
                    .is_err(),
                "participant lifecycle {bad}"
            );
        }
    }
}

#[test]
fn chat_a_participant_is_hidden_only_while_off() {
    let adapter = ChatAdapter;
    let lifecycle = |fields| {
        adapter.validate(
            PARTICIPANTS_TABLE,
            participant_group::LIFECYCLE,
            &value(fields),
        )
    };
    assert!(lifecycle(vec![Field::Null, Field::Null]).is_ok());
    assert!(lifecycle(vec![t(AT), Field::Null]).is_ok());
    assert!(lifecycle(vec![t(AT), t(AT)]).is_ok());
    assert!(lifecycle(vec![Field::Null, t(AT)]).is_err());
}

#[test]
fn chat_timestamps_accept_the_forms_honest_writers_store() {
    let engine = Engine::vault();
    let adapter = ChatAdapter;
    for good in [AT, "2026-10-24T10:51:41Z", "2026-10-24T10:51:41.000+00:00"] {
        let honest = participant_create("participant:reviewer", good);
        assert_eq!(validate::change(&engine, &honest), Ok(()), "created {good}");
        assert!(
            adapter
                .validate(
                    MESSAGES_TABLE,
                    message_group::LIFECYCLE,
                    &value(vec![t(good)])
                )
                .is_ok(),
            "{good}"
        );
    }
    assert!(
        adapter
            .validate(
                MESSAGES_TABLE,
                message_group::LIFECYCLE,
                &value(vec![Field::Null])
            )
            .is_ok()
    );
}

#[test]
fn recovered_chat_rows_present_their_names() {
    let adapter = ChatAdapter;
    let channel = adapter.presentation(
        CHANNELS_TABLE,
        &[(
            channel_group::DETAILS,
            value(vec![t("design"), t("Mockups\n and  reviews")]),
        )],
    );
    assert_eq!(channel.title, "design");
    assert_eq!(channel.preview, "Mockups and reviews");
    let agent = adapter.presentation(
        AGENTS_TABLE,
        &[(agent_group::CREATED, value(vec![t("codex"), t(AT)]))],
    );
    assert_eq!(
        agent,
        Presentation {
            title: "codex".to_string(),
            preview: String::new(),
        }
    );
    let participant = adapter.presentation(
        PARTICIPANTS_TABLE,
        &[(
            participant_group::PROFILE,
            value(vec![t("Ana"), Field::Integer(1), t("{}")]),
        )],
    );
    assert_eq!(participant.title, "Ana");
    assert_eq!(
        adapter.presentation(
            MESSAGES_TABLE,
            &[(
                message_group::CONTENT,
                value(vec![t("revision-1"), Field::Null])
            )],
        ),
        Presentation::default()
    );
}

#[test]
fn chat_provider_status_accepts_only_canonical_payloads() {
    let adapter = ChatAdapter;
    let check = |group, fields| adapter.validate(PROVIDER_STATUS_TABLE, group, &value(fields));
    let catalog = r#"{"models":[{"availability":"available","displayName":"GPT-5","id":"gpt-5","options":[]}]}"#;
    let limits = r#"{"limited":false,"providerData":null,"resetsAt":null}"#;

    assert!(
        check(
            provider_status_group::STATUS,
            vec![Field::Integer(1), t(AT)]
        )
        .is_ok()
    );
    assert!(
        check(
            provider_status_group::STATUS,
            vec![Field::Integer(2), t(AT)]
        )
        .is_err()
    );
    assert!(check(provider_status_group::CATALOG, vec![t(catalog), t(AT)]).is_ok());
    assert!(
        check(
            provider_status_group::CATALOG,
            vec![t(r#"{"models":[]}"#), t(AT)]
        )
        .is_ok()
    );
    assert!(
        check(
            provider_status_group::CATALOG,
            vec![t(r#"{"models":[],"executable":"/usr/bin/codex"}"#), t(AT)],
        )
        .is_err(),
        "a catalog carries only whitelisted fields"
    );
    assert!(
        check(
            provider_status_group::CATALOG,
            vec![
                t(r#"{"models":[{"availability":"available","description":"user@example.com","displayName":"GPT-5","id":"gpt-5","options":[]}]}"#),
                t(AT),
            ],
        )
        .is_err(),
        "a catalog drops descriptions"
    );
    assert!(
        check(
            provider_status_group::LIMITS,
            vec![Field::Null, Field::Null]
        )
        .is_ok()
    );
    assert!(check(provider_status_group::LIMITS, vec![t(limits), t(AT)]).is_ok());
    assert!(check(provider_status_group::LIMITS, vec![t(limits), Field::Null]).is_err());
    assert!(check(provider_status_group::LIMITS, vec![Field::Null, t(AT)]).is_err());
    assert!(
        check(
            provider_status_group::LIMITS,
            vec![
                t(r#"{"detail":"signed in as user@example.com","limited":false,"providerData":null,"resetsAt":null}"#),
                t(AT),
            ],
        )
        .is_err(),
        "limits drop the detail text"
    );
}

#[test]
fn chat_provider_status_rows_are_keyed_by_their_identity() {
    let adapter = ChatAdapter;
    let identity = |device: &str, family: &str| {
        vec![GroupValue::new(
            PROVIDER_STATUS_TABLE,
            provider_status_group::IDENTITY,
            value(vec![t(device), t(family)]),
        )]
    };
    let key = provider_status_key("device-a", "codex").unwrap();
    assert!(
        adapter
            .validate_row(PROVIDER_STATUS_TABLE, &key, &identity("device-a", "codex"))
            .is_ok()
    );
    assert!(
        adapter
            .validate_row(PROVIDER_STATUS_TABLE, &key, &identity("device-a", "claude"))
            .is_err()
    );
    assert!(
        adapter
            .validate_row(PROVIDER_STATUS_TABLE, &key, &identity("device-b", "codex"))
            .is_err()
    );
}
