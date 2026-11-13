//! Chat channel notes across replicas: a note arrives whole with or without its proposal
//! provenance and at its longest body, concurrent wording edits surface a conflict, a deletion
//! wins over a concurrent edit, two devices that accept one proposal write one row, an accept
//! against a dismissal of one proposal ends deleted, and deleting the channel removes its notes.

use super::*;
use ganbaru_sync_contracts::chat_channel_notes::{
    CHANNEL_NOTE_PROPOSAL_KEY_PREFIX, MAX_CHANNEL_NOTE_CHARS,
};

/// A note a person wrote.
const NOTE: &str = "channel-note:style";

/// The teammate update a proposal came from.
const PROPOSAL: &str = "item:proposal";

/// Seeds of the delete against edit scenario.
const DELETE_SEEDS: u64 = 8;

/// Rounds of one delete against edit scenario.
const DELETE_ROUNDS: usize = 4;

/// The id of the note settled from [`PROPOSAL`].
fn proposal_note() -> String {
    key(CHANNEL_NOTE_PROPOSAL_KEY_PREFIX, &[PROPOSAL])
}

/// SQL saving note `id` in [`CHANNEL`] with `body` by [`OWNER`], with `provenance` naming the
/// source item and the proposing teammate when given, deleted when `deleted_at` is given.
fn note_sql(
    id: &str,
    body: &str,
    provenance: Option<(&str, &str)>,
    deleted_at: Option<&str>,
) -> String {
    format!(
        "INSERT INTO chat_channel_notes
            (id, channel_id, source_item_id, proposed_by_participant_id, body,
             edited_by_participant_id, deleted_at, created_at, updated_at)
         VALUES ({id}, {channel}, {source}, {proposer}, {body}, {owner}, {deleted_at},
                 {created}, {updated});",
        id = quoted(id),
        channel = quoted(CHANNEL),
        source = optional(provenance.map(|(source, _)| source)),
        proposer = optional(provenance.map(|(_, proposer)| proposer)),
        body = quoted(body),
        owner = quoted(OWNER),
        deleted_at = optional(deleted_at),
        created = quoted(CREATED),
        updated = quoted(deleted_at.unwrap_or(CREATED)),
    )
}

/// SQL editing note `id` to `body` at `at`.
fn edit_note_sql(id: &str, body: &str, at: &str) -> String {
    format!(
        "UPDATE chat_channel_notes
         SET body = {body}, edited_by_participant_id = {owner}, revision = revision + 1,
             updated_at = {at}
         WHERE id = {id};",
        id = quoted(id),
        body = quoted(body),
        owner = quoted(OWNER),
        at = quoted(at),
    )
}

/// SQL deleting note `id` at `at`.
fn delete_note_sql(id: &str, at: &str) -> String {
    format!(
        "UPDATE chat_channel_notes
         SET deleted_at = {at}, edited_by_participant_id = {owner}, revision = revision + 1,
             updated_at = {at}
         WHERE id = {id};",
        id = quoted(id),
        owner = quoted(OWNER),
        at = quoted(at),
    )
}

/// Note `id`'s body and whether it is deleted, or `None` when the row is absent.
async fn note_state(replica: &mut Replica, id: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT body || ' ' || (deleted_at IS NOT NULL) FROM chat_channel_notes
             WHERE id = {}",
            quoted(id)
        ))
        .await
}

async fn conflicts(replica: &mut Replica) -> i64 {
    replica
        .count("SELECT count(*) FROM sync_rows WHERE conflict_mask <> 0")
        .await
}

/// Replicas sharing the conversation rows and the teammate update [`PROPOSAL`] names.
async fn proposal_replicas(count: usize) -> Vec<Replica> {
    let mut replicas = conversation_replicas(count).await;
    replicas[0]
        .exec(&message_sql(PROPOSAL, None, "c001", "Remember the style"))
        .await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_channel_notes_round_trip() {
    block_on(async {
        let mut replicas = proposal_replicas(2).await;
        let longest = "n".repeat(MAX_CHANNEL_NOTE_CHARS);
        replicas[0]
            .exec(&format!(
                "{}{}",
                note_sql(NOTE, &longest, None, None),
                note_sql(
                    &proposal_note(),
                    "Prefer short replies",
                    Some((PROPOSAL, TEAMMATE)),
                    None
                ),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(&format!(
                    "SELECT length(body) || ' ' || coalesce(source_item_id, '-') || ' '
                            || coalesce(proposed_by_participant_id, '-') || ' '
                            || edited_by_participant_id
                     FROM chat_channel_notes WHERE id = {}",
                    quoted(NOTE)
                ))
                .await,
            Some(format!("{MAX_CHANNEL_NOTE_CHARS} - - {OWNER}"))
        );
        assert_eq!(
            reader
                .scalar_text(&format!(
                    "SELECT body || ' ' || source_item_id || ' ' || proposed_by_participant_id
                     FROM chat_channel_notes WHERE id = {}",
                    quoted(&proposal_note())
                ))
                .await,
            Some(format!("Prefer short replies {PROPOSAL} {TEAMMATE}"))
        );
    });
}

/// Two devices reword one note while apart: one wording wins everywhere and the row surfaces a
/// conflict so the person can take the other.
#[test]
fn chat_concurrent_channel_note_edits_surface_a_conflict() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&note_sql(NOTE, "Reply in English", None, None))
            .await;
        converge(&mut replicas).await;

        replicas[0]
            .exec(&edit_note_sql(NOTE, "Reply in Spanish", &at(1, 0)))
            .await;
        replicas[1]
            .exec(&edit_note_sql(NOTE, "Reply in French", &at(1, 30)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(conflicts(replica).await, 1);
            let state = note_state(replica, NOTE).await.expect("the note exists");
            assert!(
                state == "Reply in Spanish 0" || state == "Reply in French 0",
                "{state}"
            );
        }
    });
}

/// Devices reword a note while others delete it at random; a deletion anywhere ends the note on
/// every device whatever the exchange order, and without one every device shows a live note.
#[test]
fn chat_simulated_channel_note_delete_against_edit_keeps_the_delete() {
    block_on(async {
        for seed in 0..DELETE_SEEDS {
            let mut rng = Rng(seed ^ 0x0e7e);
            let mut replicas = conversation_replicas(3).await;
            replicas[0]
                .exec(&note_sql(NOTE, "Reply in English", None, None))
                .await;
            converge(&mut replicas).await;
            let mut deleted = false;
            for round in 0..DELETE_ROUNDS {
                for replica in &mut replicas {
                    let live = note_state(replica, NOTE)
                        .await
                        .is_some_and(|state| state.ends_with(" 0"));
                    if !live {
                        continue;
                    }
                    if rng.chance(20) {
                        replica
                            .exec(&delete_note_sql(NOTE, &at(round + 1, rng.below(60))))
                            .await;
                        deleted = true;
                    } else if rng.chance(60) {
                        let body = format!("Wording {} {round}", replica.index);
                        replica
                            .exec(&edit_note_sql(NOTE, &body, &at(round + 1, rng.below(60))))
                            .await;
                    }
                }
                if rng.chance(50) {
                    converge(&mut replicas).await;
                }
            }
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                let state = note_state(replica, NOTE).await.expect("the note row stays");
                assert_eq!(
                    state.ends_with(" 1"),
                    deleted,
                    "seed {seed} replica {}: {state}",
                    replica.index
                );
            }
        }
    });
}

/// Two devices accept one proposal with the same wording while apart: the derived id and the
/// proposal's creation time make one row with no conflict.
#[test]
fn chat_two_accepts_of_one_proposal_give_one_note() {
    block_on(async {
        let mut replicas = proposal_replicas(2).await;
        let accepted = note_sql(
            &proposal_note(),
            "Prefer short replies",
            Some((PROPOSAL, TEAMMATE)),
            None,
        );
        replicas[0].exec(&accepted).await;
        replicas[1].exec(&accepted).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM chat_channel_notes")
                    .await,
                1
            );
            assert_eq!(conflicts(replica).await, 0);
            assert_eq!(
                note_state(replica, &proposal_note()).await.as_deref(),
                Some("Prefer short replies 0")
            );
        }
    });
}

/// One device accepts a proposal with an edited wording while another dismisses it: the
/// dismissal's deletion wins, so the proposal ends unsaved everywhere.
#[test]
fn chat_accept_against_dismiss_of_one_proposal_ends_deleted() {
    block_on(async {
        let mut replicas = proposal_replicas(2).await;
        replicas[0]
            .exec(&note_sql(
                &proposal_note(),
                "Prefer short replies in English",
                Some((PROPOSAL, TEAMMATE)),
                None,
            ))
            .await;
        replicas[1]
            .exec(&note_sql(
                &proposal_note(),
                "Prefer short replies",
                Some((PROPOSAL, TEAMMATE)),
                Some(&at(1, 0)),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            let state = note_state(replica, &proposal_note())
                .await
                .expect("the settled row exists");
            assert!(state.ends_with(" 1"), "{state}");
        }
    });
}

/// Deleting a channel removes its notes on every device.
#[test]
fn chat_deleting_a_channel_removes_its_notes() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&note_sql(NOTE, "Reply in English", None, None))
            .await;
        converge(&mut replicas).await;

        replicas[1]
            .exec(&format!(
                "DELETE FROM chat_channels WHERE id = {}",
                quoted(CHANNEL)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM chat_channel_notes")
                    .await,
                0
            );
        }
    });
}
