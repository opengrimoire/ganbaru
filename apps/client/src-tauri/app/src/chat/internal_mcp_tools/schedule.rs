//! The schedule proposal host tool: an agent proposes a message to send later, once or on a
//! repeat rule, and a person confirms or dismisses it from the card in the reply thread.
//!
//! The tool schedules nothing. A proposal is an agent message with a `schedule_proposal`
//! semantic update, written in one transaction and idempotent by its client request id.

use super::delegation::MAX_CLIENT_REQUEST_ID_CHARS;
use super::reports::{client_request_id, host_call};
use super::{HostToolContext, generic_denial_error};
use crate::chat::command_support::now_timestamp;
use crate::chat::organization::validate_proposed_schedule;
use ganbaru_chat::coordination::schedule_proposals::{
    MAX_PROPOSAL_REASON_CHARS, MAX_PROPOSED_MESSAGE_CHARS, ScheduleProposal,
    ScheduleProposalRefusal, normalize_proposal_text, record_host_schedule_proposal,
};
use ganbaru_chat::scheduling::{
    LAST_MINUTE_OF_DAY, MAX_RECURRENCE_INTERVAL_MINUTES, MIN_RECURRENCE_INTERVAL_MINUTES,
    QuietPolicy, RecurrenceRule, RecurrenceSpec, WeekdaySet,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use serde_json::{Map, Value, json};

/// Zone an interval proposal uses when the device zone has no IANA name.
const FALLBACK_TIME_ZONE: &str = "UTC";
/// Longest IANA zone name a proposal may carry, in bytes.
const MAX_TIME_ZONE_BYTES: usize = 64;
/// Days in a week, the highest ISO weekday number.
const DAYS_PER_WEEK: u64 = 7;

/// The input properties of the schedule proposal tool.
pub(super) fn properties() -> Value {
    json!({
        "message": { "type": "string", "minLength": 1, "maxLength": MAX_PROPOSED_MESSAGE_CHARS },
        "firstAt": {
            "type": "string",
            "description": "RFC 3339 UTC time of the first send, such as 2026-01-05T08:00:00.000Z"
        },
        "recurrence": {
            "description": "Omit to send once",
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "kind": { "const": "interval" },
                        "minutes": {
                            "type": "integer",
                            "minimum": MIN_RECURRENCE_INTERVAL_MINUTES,
                            "maximum": MAX_RECURRENCE_INTERVAL_MINUTES
                        }
                    },
                    "required": ["kind", "minutes"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "kind": { "const": "fixed_time" },
                        "localMinute": {
                            "type": "integer", "minimum": 0, "maximum": LAST_MINUTE_OF_DAY
                        },
                        "weekdays": {
                            "type": "array",
                            "description": "ISO weekdays, 1 for Monday through 7 for Sunday",
                            "minItems": 1,
                            "maxItems": DAYS_PER_WEEK,
                            "uniqueItems": true,
                            "items": { "type": "integer", "minimum": 1, "maximum": DAYS_PER_WEEK }
                        },
                        "timeZone": {
                            "type": "string", "minLength": 1, "maxLength": MAX_TIME_ZONE_BYTES
                        }
                    },
                    "required": ["kind", "localMinute", "weekdays", "timeZone"],
                    "additionalProperties": false
                }
            ]
        },
        "reason": { "type": "string", "maxLength": MAX_PROPOSAL_REASON_CHARS },
        "clientRequestId": {
            "type": "string", "minLength": 1, "maxLength": MAX_CLIENT_REQUEST_ID_CHARS
        }
    })
}

/// Posts a schedule proposal card in the reply thread for a person to confirm or dismiss.
pub(super) async fn propose(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let proposal = parse_proposal(arguments)?;
    let client_request_id = client_request_id(arguments)?;
    let now = now_timestamp()?;
    let mut transaction = context
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    record_host_schedule_proposal(
        &mut transaction,
        host_call(context, client_request_id),
        &proposal,
        |proposal| validate_proposed_schedule(proposal, &now),
        now.as_str(),
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(json!({ "status": "proposed" }))
}

/// The proposal the arguments describe, with its text trimmed and bounded. Its time and rule
/// are checked against the clock and zone data when it is recorded.
fn parse_proposal(arguments: &Map<String, Value>) -> ChatResult<ScheduleProposal> {
    let first_at = arguments
        .get("firstAt")
        .and_then(Value::as_str)
        .ok_or_else(|| ScheduleProposalRefusal::TimeInvalid.into_error())?;
    let proposal = ScheduleProposal {
        message: text(arguments, "message")?,
        first_at: first_at.to_owned(),
        recurrence: parse_recurrence(arguments.get("recurrence"))?,
        reason: text(arguments, "reason")?,
    };
    normalize_proposal_text(proposal).map_err(ScheduleProposalRefusal::into_error)
}

/// A text argument. Its length is checked with the proposal so an oversized text gets the
/// typed refusal; a missing value or a control character is denied generically.
fn text(arguments: &Map<String, Value>, name: &str) -> ChatResult<String> {
    let value = arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(generic_denial_error)?;
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(generic_denial_error());
    }
    Ok(value.to_owned())
}

/// The repeat rule, or `None` for a one-time proposal. An interval uses the device zone for the
/// times shown, and every proposed series waits out quiet periods.
fn parse_recurrence(value: Option<&Value>) -> ChatResult<Option<RecurrenceSpec>> {
    let invalid = || ScheduleProposalRefusal::RecurrenceInvalid.into_error();
    let fields = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(fields)) => fields,
        Some(_) => return Err(invalid()),
    };
    let integer = |name: &str| fields.get(name).and_then(Value::as_u64).ok_or_else(invalid);
    let (rule, time_zone) = match fields.get("kind").and_then(Value::as_str) {
        Some("interval") if fields.len() == 2 => {
            let minutes = u32::try_from(integer("minutes")?).map_err(|_| invalid())?;
            (RecurrenceRule::Interval { minutes }, device_time_zone())
        }
        Some("fixed_time") if fields.len() == 4 => {
            let local_minute = u16::try_from(integer("localMinute")?).map_err(|_| invalid())?;
            let weekdays = parse_weekdays(fields.get("weekdays")).ok_or_else(invalid)?;
            let time_zone = fields
                .get("timeZone")
                .and_then(Value::as_str)
                .filter(|zone| !zone.is_empty() && zone.len() <= MAX_TIME_ZONE_BYTES)
                .ok_or_else(invalid)?;
            (
                RecurrenceRule::FixedTime {
                    local_minute,
                    weekdays,
                },
                time_zone.to_owned(),
            )
        }
        _ => return Err(invalid()),
    };
    Ok(Some(RecurrenceSpec {
        rule,
        time_zone,
        quiet_policy: QuietPolicy::Defer,
    }))
}

/// The weekdays as ISO numbers, 1 for Monday through 7 for Sunday: at least one, none repeated.
fn parse_weekdays(value: Option<&Value>) -> Option<WeekdaySet> {
    let days = value?.as_array()?;
    let mut mask = 0_u8;
    for day in days {
        let day = day
            .as_u64()
            .filter(|day| (1..=DAYS_PER_WEEK).contains(day))?;
        let bit = 1_u8 << (day - 1);
        if mask & bit != 0 {
            return None;
        }
        mask |= bit;
    }
    WeekdaySet::from_mask(mask).filter(|set| !set.is_empty())
}

fn device_time_zone() -> String {
    ganbaru_civil_time::system_zone()
        .ok()
        .and_then(|zone| zone.iana_name().map(str::to_owned))
        .unwrap_or_else(|| FALLBACK_TIME_ZONE.to_owned())
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat schedule proposal could not be saved",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    fn refusal(value: Value) -> Option<ScheduleProposalRefusal> {
        ScheduleProposalRefusal::from_error(&parse_proposal(&arguments(value)).unwrap_err())
    }

    #[test]
    fn a_fixed_time_proposal_reads_iso_weekdays_into_the_monday_first_mask() {
        let proposal = parse_proposal(&arguments(json!({
            "message": " Post the summary ",
            "firstAt": "2026-11-13T08:00:00.000Z",
            "recurrence": {
                "kind": "fixed_time", "localMinute": 480, "weekdays": [1, 5, 7],
                "timeZone": "Europe/Madrid"
            },
            "reason": "Asked for it"
        })))
        .unwrap();
        assert_eq!(proposal.message, "Post the summary");
        let spec = proposal.recurrence.unwrap();
        assert_eq!(
            spec.rule,
            RecurrenceRule::FixedTime {
                local_minute: 480,
                weekdays: WeekdaySet::from_mask(0b101_0001).unwrap(),
            }
        );
        assert_eq!(spec.time_zone, "Europe/Madrid");
        assert_eq!(spec.quiet_policy, QuietPolicy::Defer);
    }

    #[test]
    fn interval_and_one_time_proposals_parse() {
        let interval = parse_proposal(&arguments(json!({
            "message": "Check the build",
            "firstAt": "2026-11-13T08:00:00.000Z",
            "recurrence": { "kind": "interval", "minutes": 60 },
            "reason": ""
        })))
        .unwrap();
        let spec = interval.recurrence.unwrap();
        assert_eq!(spec.rule, RecurrenceRule::Interval { minutes: 60 });
        assert!(!spec.time_zone.is_empty());
        let once = parse_proposal(&arguments(json!({
            "message": "Check the build",
            "firstAt": "2026-11-13T08:00:00.000Z",
            "reason": "Release day"
        })))
        .unwrap();
        assert_eq!(once.recurrence, None);
    }

    #[test]
    fn malformed_rules_get_the_recurrence_refusal() {
        for recurrence in [
            json!({ "kind": "fixed_time", "localMinute": 480, "weekdays": [], "timeZone": "UTC" }),
            json!({ "kind": "fixed_time", "localMinute": 480, "weekdays": [1, 1], "timeZone": "UTC" }),
            json!({ "kind": "fixed_time", "localMinute": 480, "weekdays": [0], "timeZone": "UTC" }),
            json!({ "kind": "fixed_time", "localMinute": 480, "weekdays": [8], "timeZone": "UTC" }),
            json!({ "kind": "fixed_time", "localMinute": 480, "weekdays": [1] }),
            json!({ "kind": "interval", "minutes": -5 }),
            json!({ "kind": "interval", "minutes": 60, "timeZone": "UTC" }),
            json!({ "kind": "monthly" }),
            json!("daily"),
        ] {
            assert_eq!(
                refusal(json!({
                    "message": "Nudge",
                    "firstAt": "2026-11-13T08:00:00.000Z",
                    "recurrence": recurrence.clone(),
                    "reason": "Asked"
                })),
                Some(ScheduleProposalRefusal::RecurrenceInvalid),
                "{recurrence}"
            );
        }
    }

    #[test]
    fn text_bounds_get_the_size_refusal_and_control_characters_are_denied() {
        let first_at = "2026-11-13T08:00:00.000Z";
        assert_eq!(
            refusal(json!({ "message": "  ", "firstAt": first_at, "reason": "" })),
            Some(ScheduleProposalRefusal::TooLarge)
        );
        assert_eq!(
            refusal(json!({
                "message": "x".repeat(MAX_PROPOSED_MESSAGE_CHARS + 1), "firstAt": first_at,
                "reason": ""
            })),
            Some(ScheduleProposalRefusal::TooLarge)
        );
        assert_eq!(
            refusal(json!({
                "message": "Nudge", "firstAt": first_at,
                "reason": "r".repeat(MAX_PROPOSAL_REASON_CHARS + 1)
            })),
            Some(ScheduleProposalRefusal::TooLarge)
        );
        assert_eq!(
            refusal(json!({ "message": "Nudge", "reason": "" })),
            Some(ScheduleProposalRefusal::TimeInvalid)
        );
        assert_eq!(
            refusal(json!({ "message": "bell\u{7}", "firstAt": first_at, "reason": "" })),
            None
        );
    }
}
