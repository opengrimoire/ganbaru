//! Contacts adapter: the value checks that mirror the `contacts`, `contact_requests`, and
//! `contact_card` table checks, and the priorities of their coupled groups.
//!
//! A block outranks an active standing, so blocking a person on one device is never undone by a
//! concurrent accept on another. A request answer outranks a pending state, and acceptance
//! outranks every other answer. A rotated card outranks every card with a lower revision.

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::contacts::{
    ADAPTER, CARD_TABLE, CONTACTS_TABLE, REQUESTS_TABLE, card_group, contact_group, request_group,
};

/// Characters of a person public key in its text form.
pub const PUBLIC_KEY_CHARS: usize = 43;
/// Characters of a display name after trimming.
pub const MAX_DISPLAY_NAME_CHARS: usize = 160;
/// Largest palette slot of a contact color.
pub const MAX_COLOR: i64 = 31;
/// Fewest characters of a stored timestamp, as the tables check it.
pub const MIN_TIMESTAMP_CHARS: usize = 20;
/// Characters of a request id, local or the peer's.
pub const MAX_REQUEST_ID_CHARS: usize = 160;
/// Characters of a signed card in its text form.
pub const MAX_CARD_CHARS: usize = 1024;
/// Characters of a hex SHA-256 digest or coordinator fingerprint.
pub const DIGEST_CHARS: usize = 64;
/// Characters of an endpoint hint.
pub const MAX_ENDPOINT_HINT_CHARS: usize = 64;
/// Characters of an encoded card nonce.
pub const CARD_NONCE_CHARS: usize = 22;

/// Contact standings, lowest priority first. A removed contact keeps its row, so a removal
/// merges against concurrent edits as a standing rather than as a deletion.
pub const CONTACT_STATES: [&str; 3] = ["active", "removed", "blocked"];
/// Characters of the longest contact standing.
pub const MAX_CONTACT_STATE_CHARS: usize = 7;
/// Trust kinds a contact or request may carry, narrowest first.
pub const TRUST_KINDS: [&str; 5] = [
    "not_allowed",
    "once",
    "seven_days",
    "thirty_days",
    "until_revoked",
];
/// Characters of the longest trust kind.
pub const MAX_TRUST_CHARS: usize = 13;
/// Request directions.
pub const REQUEST_DIRECTIONS: [&str; 2] = ["sent", "received"];
/// Characters of the longest request direction.
pub const MAX_DIRECTION_CHARS: usize = 8;
/// Request states, lowest priority first.
pub const REQUEST_STATES: [&str; 4] = ["pending", "expired", "declined", "accepted"];
/// Characters of the longest request state.
pub const MAX_REQUEST_STATE_CHARS: usize = 8;

fn text(field: &Field) -> Result<&str, AdapterError> {
    match field {
        Field::Text(text) => Ok(text),
        _ => Err(AdapterError("contacts text field")),
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
        _ => Err(AdapterError("contacts integer field")),
    }
}

fn one_of(field: &Field, allowed: &[&str], what: &'static str) -> Result<(), AdapterError> {
    if allowed.contains(&text(field)?) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn timestamp(field: &Field) -> Result<(), AdapterError> {
    if text(field)?.chars().count() >= MIN_TIMESTAMP_CHARS {
        Ok(())
    } else {
        Err(AdapterError("contacts timestamp is too short"))
    }
}

fn optional_timestamp(field: &Field) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => timestamp(field),
    }
}

fn display_name(field: &Field) -> Result<(), AdapterError> {
    let trimmed = text(field)?.trim_matches(' ').chars().count();
    if (1..=MAX_DISPLAY_NAME_CHARS).contains(&trimmed) {
        Ok(())
    } else {
        Err(AdapterError("contacts display name"))
    }
}

fn color(field: &Field) -> Result<(), AdapterError> {
    if (0..=MAX_COLOR).contains(&integer(field)?) {
        Ok(())
    } else {
        Err(AdapterError("contacts color"))
    }
}

fn exact_chars(field: &Field, chars: usize, what: &'static str) -> Result<(), AdapterError> {
    if text(field)?.chars().count() == chars {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn trust(field: &Field) -> Result<(), AdapterError> {
    one_of(field, &TRUST_KINDS, "contacts trust kind")
}

fn validate_contact(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (contact_group::CREATED | contact_group::UPDATED, [at]) => timestamp(at),
        (contact_group::PROFILE, [name, palette]) => {
            display_name(name)?;
            color(palette)
        }
        (
            contact_group::STANDING,
            [
                state,
                accepted_at,
                blocked_at,
                invite,
                invite_expires_at,
                message,
                message_expires_at,
            ],
        ) => {
            one_of(state, &CONTACT_STATES, "contact standing")?;
            optional_timestamp(accepted_at)?;
            optional_timestamp(blocked_at)?;
            trust(invite)?;
            optional_timestamp(invite_expires_at)?;
            trust(message)?;
            optional_timestamp(message_expires_at)
        }
        (contact_group::EXISTS, _) => Ok(()),
        _ => Err(AdapterError("contact value")),
    }
}

fn validate_request(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            request_group::CREATED,
            [
                direction,
                public_key,
                peer_request_id,
                name,
                palette,
                card,
                card_digest,
                endpoint_hint,
                fingerprint,
                expires_at,
                created_at,
            ],
        ) => {
            one_of(direction, &REQUEST_DIRECTIONS, "contact request direction")?;
            exact_chars(public_key, PUBLIC_KEY_CHARS, "contact request public key")?;
            if optional_text(peer_request_id)?
                .is_some_and(|id| !(1..=MAX_REQUEST_ID_CHARS).contains(&id.chars().count()))
            {
                return Err(AdapterError("contact request peer id"));
            }
            display_name(name)?;
            color(palette)?;
            if !(1..=MAX_CARD_CHARS).contains(&text(card)?.chars().count()) {
                return Err(AdapterError("contact request card"));
            }
            exact_chars(card_digest, DIGEST_CHARS, "contact request card digest")?;
            if text(endpoint_hint)?.chars().count() > MAX_ENDPOINT_HINT_CHARS {
                return Err(AdapterError("contact request endpoint hint"));
            }
            if !matches!(text(fingerprint)?.chars().count(), 0 | DIGEST_CHARS) {
                return Err(AdapterError("contact request fingerprint"));
            }
            timestamp(expires_at)?;
            timestamp(created_at)
        }
        (request_group::STATE, [state, invite, message]) => {
            one_of(state, &REQUEST_STATES, "contact request state")?;
            trust(invite)?;
            trust(message)
        }
        (request_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("contact request value")),
    }
}

fn validate_card(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (card_group::CARD, [nonce, revision]) => {
            exact_chars(nonce, CARD_NONCE_CHARS, "contact card nonce")?;
            if integer(revision)? >= 1 {
                Ok(())
            } else {
                Err(AdapterError("contact card revision"))
            }
        }
        (card_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("contact card value")),
    }
}

/// Position of `field`'s text in `ranked`, lowest first, or 0 for anything else.
fn rank(field: Option<&Field>, ranked: &[&str]) -> u8 {
    let Some(Field::Text(text)) = field else {
        return 0;
    };
    ranked
        .iter()
        .position(|candidate| candidate == text)
        .and_then(|position| u8::try_from(position).ok())
        .unwrap_or(0)
}

/// Weight of one standing rank above any sum of trust narrowness.
const STANDING_RANK_WEIGHT: u8 = 16;

/// How narrow a trust kind is: `until_revoked` is 0 and `not_allowed` is the widest gap.
fn narrowness(field: Option<&Field>) -> u8 {
    let Some(Field::Text(text)) = field else {
        return 0;
    };
    TRUST_KINDS
        .iter()
        .rev()
        .position(|candidate| candidate == text)
        .and_then(|position| u8::try_from(position).ok())
        .unwrap_or(0)
}

/// Priority of a standing value: the standing rank first, then the summed narrowness of the
/// invite and message trust, so a concurrent narrowing beats a concurrent widening.
fn standing_priority(fields: &[Field]) -> u8 {
    let state = rank(fields.first(), &CONTACT_STATES);
    let narrow = narrowness(fields.get(3)) + narrowness(fields.get(5));
    state * STANDING_RANK_WEIGHT + narrow
}

/// Adapter of the contacts domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct ContactsAdapter;

impl DomainAdapter for ContactsAdapter {
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

    /// Blocked outranks removed, which outranks active; within one standing the narrower
    /// trust wins, so concurrent edits never widen what a person may do. An answered request
    /// outranks a pending one with acceptance highest, and a card outranks every card with a
    /// lower revision. Revisions past `u8::MAX` share the top priority and fall back to the
    /// clock.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        let first = value.fields().first();
        if table == CONTACTS_TABLE && group == contact_group::STANDING {
            standing_priority(value.fields())
        } else if table == REQUESTS_TABLE && group == request_group::STATE {
            rank(first, &REQUEST_STATES)
        } else if table == CARD_TABLE && group == card_group::CARD {
            match value.fields().get(1) {
                Some(Field::Integer(revision)) => {
                    u8::try_from((*revision).clamp(0, i64::from(u8::MAX))).unwrap_or(u8::MAX)
                }
                _ => 0,
            }
        } else {
            0
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        let fields = value.fields();
        if table == CONTACTS_TABLE {
            validate_contact(group, fields)
        } else if table == REQUESTS_TABLE {
            validate_request(group, fields)
        } else if table == CARD_TABLE {
            validate_card(group, fields)
        } else {
            Ok(())
        }
    }

    /// Contact conflicts are never surfaced, so rows present nothing.
    fn presentation(&self, _table: TableId, _values: &[(GroupId, Value)]) -> Presentation {
        Presentation::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const AT: &str = "2026-10-22T10:51:41.000Z";

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn t(text: &str) -> Field {
        Field::Text(text.to_owned())
    }

    fn standing(state: &str) -> Value {
        value(vec![
            t(state),
            Field::Null,
            Field::Null,
            t("not_allowed"),
            Field::Null,
            t("once"),
            Field::Null,
        ])
    }

    fn request_state(state: &str) -> Value {
        value(vec![t(state), t("not_allowed"), t("not_allowed")])
    }

    fn card(revision: i64) -> Value {
        value(vec![t("AAAAAAAAAAAAAAAAAAAAAA"), Field::Integer(revision)])
    }

    fn created_request(peer: Field, fingerprint: &str) -> Value {
        value(vec![
            t("received"),
            t(KEY),
            peer,
            t(" Ada "),
            Field::Integer(7),
            t("card"),
            t(&"0".repeat(DIGEST_CHARS)),
            t("127.0.0.1:43821"),
            t(fingerprint),
            t(AT),
            t(AT),
        ])
    }

    #[test]
    fn contacts_priorities_rank_block_answers_and_card_revisions() {
        let adapter = ContactsAdapter;
        let standing_priority =
            |state| adapter.priority(CONTACTS_TABLE, contact_group::STANDING, &standing(state));
        assert!(standing_priority("blocked") > standing_priority("removed"));
        assert!(standing_priority("removed") > standing_priority("active"));

        let trusted = |state: &str, invite: &str, message: &str| {
            let value = value(vec![
                t(state),
                Field::Null,
                Field::Null,
                t(invite),
                Field::Null,
                t(message),
                Field::Null,
            ]);
            adapter.priority(CONTACTS_TABLE, contact_group::STANDING, &value)
        };
        assert!(
            trusted("active", "not_allowed", "seven_days")
                > trusted("active", "until_revoked", "seven_days")
        );
        assert!(trusted("active", "once", "once") > trusted("active", "thirty_days", "once"));
        assert!(
            trusted("removed", "until_revoked", "until_revoked")
                > trusted("active", "not_allowed", "not_allowed")
        );
        assert!(
            trusted("blocked", "until_revoked", "until_revoked")
                > trusted("removed", "not_allowed", "not_allowed")
        );

        let answer =
            |state| adapter.priority(REQUESTS_TABLE, request_group::STATE, &request_state(state));
        assert!(answer("accepted") > answer("declined"));
        assert!(answer("declined") > answer("expired"));
        assert!(answer("expired") > answer("pending"));

        let card_priority =
            |revision| adapter.priority(CARD_TABLE, card_group::CARD, &card(revision));
        assert!(card_priority(3) > card_priority(2));
        assert_eq!(card_priority(255), u8::MAX);
        assert_eq!(card_priority(10_000), u8::MAX);
        assert_eq!(
            adapter.priority(CARD_TABLE, card_group::UPDATED, &value(vec![t(AT)])),
            0
        );
    }

    #[test]
    fn contacts_validation_mirrors_the_table_checks() {
        let adapter = ContactsAdapter;
        let contact = |group, value: &Value| adapter.validate(CONTACTS_TABLE, group, value);
        assert_eq!(
            contact(contact_group::STANDING, &standing("blocked")),
            Ok(())
        );
        assert!(contact(contact_group::STANDING, &standing("muted")).is_err());
        assert_eq!(
            contact(
                contact_group::PROFILE,
                &value(vec![t("Ada"), Field::Integer(31)])
            ),
            Ok(())
        );
        assert!(
            contact(
                contact_group::PROFILE,
                &value(vec![t("   "), Field::Integer(3)])
            )
            .is_err()
        );
        assert!(
            contact(
                contact_group::PROFILE,
                &value(vec![t("Ada"), Field::Integer(32)])
            )
            .is_err()
        );
        assert!(contact(contact_group::UPDATED, &value(vec![t("2026-10-22")])).is_err());

        let request = |group, value: &Value| adapter.validate(REQUESTS_TABLE, group, value);
        let fingerprint = "f".repeat(DIGEST_CHARS);
        assert_eq!(
            request(
                request_group::CREATED,
                &created_request(t("contact-peer"), &fingerprint)
            ),
            Ok(())
        );
        assert_eq!(
            request(request_group::CREATED, &created_request(Field::Null, "")),
            Ok(())
        );
        assert!(
            request(
                request_group::CREATED,
                &created_request(t(""), &fingerprint)
            )
            .is_err()
        );
        assert!(
            request(
                request_group::CREATED,
                &created_request(Field::Null, "short")
            )
            .is_err()
        );
        assert_eq!(
            request(request_group::STATE, &request_state("declined")),
            Ok(())
        );
        assert!(request(request_group::STATE, &request_state("lost")).is_err());

        let card_value = |group, value: &Value| adapter.validate(CARD_TABLE, group, value);
        assert_eq!(card_value(card_group::CARD, &card(1)), Ok(()));
        assert!(card_value(card_group::CARD, &card(0)).is_err());
        assert!(
            card_value(
                card_group::CARD,
                &value(vec![t("short"), Field::Integer(1)])
            )
            .is_err()
        );
    }
}
