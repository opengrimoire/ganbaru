//! Removal exclusion: operations a removed device sealed past its cutoff lose their effects
//! everywhere, the versions they superseded come back, honest operations that saw them keep
//! theirs, anchor marks release retained versions, and the late changes they leave are derived
//! the same way on every replica and decided once.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_DEVICE;
use ganbaru_sync_contracts::hlc::MAX_ADOPTION_AHEAD_MS;
use ganbaru_sync_contracts::{
    Change, ChangeAction, Entry, Envelope, Field, GroupValue, Hlc, LateResolution, LateRow,
    ResolveLate, RowKey, Value, WriterId, derived_key,
};

use super::authorization::{admin_entry, remove_entry, superseded};
use super::block_on;
use super::engine::{text_field, with_group};
use super::holds::sealed_create;
use super::replica::{
    Replica, converge, device_admin, forge_change, forge_changes, quoted, rank, sealed_change,
};
use crate::acknowledge::ACKNOWLEDGEMENT_INTERVAL_MS;
use crate::domains::themes::PALETTE_SIZE;
use crate::manifest::vault::chat::{PARTICIPANTS_TABLE, PROFILES_TABLE};
use crate::manifest::vault::contacts::{CARD_KEY, CARD_TABLE, CONTACTS_TABLE};
use crate::manifest::vault::distractions::{
    HOST_RULE_KEY_PREFIX, HOST_RULES_TABLE, SETTINGS_KEY, SETTINGS_TABLE, host_group,
};
use crate::manifest::vault::music::PLAYLISTS_TABLE;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use crate::provenance::row_creating_devices;
use crate::{HoldReason, LateChange, LateKind, MAX_EXCLUDED_PER_WRITER, StoreOutcome, SyncError};
use crate::{hydrate, log};

const CREATED_AT: &str = "2027-01-01T00:00:00.000Z";
const EDITED_AT: &str = "2027-01-02T00:00:00.000Z";
const AT_MS: i64 = 1_800_000_000_000;
const PEER_KEY: &str = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE";

/// The anchor, the device the tests remove, and an honest member.
async fn trio(salt: u64) -> (Replica, Replica, Replica) {
    (
        Replica::anchor(0, salt).await,
        Replica::new(1, salt).await,
        Replica::new(2, salt).await,
    )
}

pub(super) async fn edit(replica: &mut Replica, sql: &str) {
    replica.exec(sql).await;
    replica.seal().await.unwrap();
}

/// Delivers what `from` has to `to` and applies it.
pub(super) async fn deliver(to: &mut Replica, from: &mut Replica) {
    to.pull(from).await;
}

/// Counts the kept displaced versions, held Creates aside, checking that the status reports the
/// same count.
pub(super) async fn displaced(replica: &mut Replica) -> i64 {
    let rows = replica
        .count(
            "SELECT count(*) FROM sync_displaced_versions
             WHERE NOT (writer_id = displaced_by_writer AND seq = displaced_by_seq)",
        )
        .await;
    let status = replica
        .engine
        .status(&mut replica.conn, &replica.ctx)
        .await
        .unwrap();
    assert_eq!(i64::try_from(status.displaced).unwrap(), rows);
    rows
}

pub(super) async fn late_changes(replica: &mut Replica) -> Vec<LateChange> {
    replica
        .engine
        .late_changes(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
}

/// Late changes as `(row, kind)`, in their listed order.
async fn late_rows(replica: &mut Replica) -> Vec<(String, LateKind)> {
    late_changes(replica)
        .await
        .into_iter()
        .map(|late| (late.row_key, late.kind))
        .collect()
}

pub(super) async fn title(replica: &mut Replica, id: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT title FROM quick_notes WHERE id = {}",
            quoted(id)
        ))
        .await
}

/// How many versions of the Immutable creation group a note holds.
pub(super) async fn created_versions(replica: &mut Replica, id: &str) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_register_versions
             WHERE table_id = {} AND row_key = {} AND group_id = {}",
            NOTES_TABLE.0,
            quoted(id),
            note_group::CREATED.get()
        ))
        .await
}

pub(super) fn note(id: &str, title: &str, position: u32) -> String {
    format!(
        "INSERT INTO quick_notes (id, title, order_key) VALUES ({}, {}, {});",
        quoted(id),
        quoted(title),
        quoted(&rank(position)),
    )
}

/// One row in every domain, written by the honest member.
fn shared_rows() -> String {
    format!(
        "{kept}{gone}
         INSERT INTO vault_preferences (key, value, updated_at)
         VALUES ('preferences.language', '\"en\"', '{CREATED_AT}');
         INSERT INTO themes
             (id, display_name, blend_canvas, seed_blend_canvas, derivation_engine_version,
              created_at_ms, updated_at_ms, icon_label, seed_icon_label)
         VALUES ('theme-shared', 'Ocean', '#101010', '#101010', 1, {AT_MS}, {AT_MS}, 'dark',
                 'dark');
         WITH RECURSIVE slot(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM slot WHERE n < {last})
         INSERT INTO theme_event_palette (theme_id, slot, value)
         SELECT 'theme-shared', n, '#202020' FROM slot;
         WITH RECURSIVE slot(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM slot WHERE n < {last})
         INSERT INTO theme_seed_event_palette (theme_id, slot, value)
         SELECT 'theme-shared', n, '#202020' FROM slot;
         INSERT INTO contacts (id, public_key, display_name, state, created_at, updated_at)
         VALUES ('contact-{PEER_KEY}', '{PEER_KEY}', 'Ana', 'active', '{CREATED_AT}', '{CREATED_AT}');
         INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms, order_key)
         VALUES ('playlist-shared', 'Focus', {AT_MS}, {AT_MS}, {first});
         INSERT INTO project_groups (id, name, created_at, updated_at, order_key)
         VALUES ('group-shared', 'Work', '{CREATED_AT}', '{CREATED_AT}', {first});",
        kept = note("note-kept", "Draft", 0),
        gone = note("note-gone", "Old", 1),
        first = quoted(&rank(0)),
        last = PALETTE_SIZE - 1,
    )
}

/// What the removed device does past its cutoff: a value in every domain, a deletion, and a
/// created row.
fn late_edits() -> String {
    let later = AT_MS + 60_000;
    format!(
        "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-kept';
         DELETE FROM quick_notes WHERE id = 'note-gone';
         UPDATE vault_preferences SET value = '\"es\"', updated_at = '{EDITED_AT}'
         WHERE key = 'preferences.language';
         UPDATE themes SET display_name = 'Late', updated_at_ms = {later}
         WHERE id = 'theme-shared';
         UPDATE contacts SET display_name = 'Late', updated_at = '{EDITED_AT}'
         WHERE id = 'contact-{PEER_KEY}';
         UPDATE music_playlists SET name = 'Late', updated_at_ms = {later}
         WHERE id = 'playlist-shared';
         UPDATE project_groups SET name = 'Late', updated_at = '{EDITED_AT}'
         WHERE id = 'group-shared';
         {created}",
        created = note("note-late", "Late", 2),
    )
}

/// Every replica stores and applies what the others hold, without the removed device.
async fn spread(anchor: &mut Replica, honest: &mut Replica) {
    deliver(anchor, honest).await;
    deliver(honest, anchor).await;
}

#[test]
fn excluding_applied_operations_restores_what_they_superseded_in_every_domain() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(201).await;
        edit(&mut honest, &shared_rows()).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        let before = honest.domain_dump().await;
        assert_eq!(anchor.domain_dump().await, before);

        // The honest member applies the late edits before the anchor stores them, so it keeps
        // what they superseded.
        edit(&mut removed, &late_edits()).await;
        deliver(&mut honest, &mut removed).await;
        assert_eq!(
            title(&mut honest, "note-kept").await.as_deref(),
            Some("Late")
        );
        assert_eq!(
            title(&mut honest, "note-late").await.as_deref(),
            Some("Late")
        );
        assert!(displaced(&mut honest).await > 0);

        let device = removed.device_id.clone();
        anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        assert_eq!(
            honest.domain_dump().await,
            before,
            "excluding applied operations leaves what never applying them leaves"
        );
        assert_eq!(displaced(&mut honest).await, 0);

        // The anchor stores the late operations, held and excluded, and changes nothing.
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut anchor, &mut honest).await;
        assert_eq!(anchor.domain_dump().await, before);
        assert_eq!(displaced(&mut anchor).await, 0);
        let mut observer = Replica::new(3, 201).await;
        deliver(&mut observer, &mut anchor).await;
        assert_eq!(observer.domain_dump().await, before);
        for replica in [&mut anchor, &mut honest, &mut observer] {
            replica.check_invariants().await;
            let rows = late_rows(replica).await;
            assert_eq!(rows.len(), 8, "one late change per row: {rows:?}");
            assert!(rows.contains(&("note-gone".to_owned(), LateKind::Tombstone)));
            assert!(rows.contains(&("note-late".to_owned(), LateKind::Created)));
            assert!(rows.contains(&("note-kept".to_owned(), LateKind::Values)));
        }
        let listed = late_changes(&mut anchor).await;
        assert_eq!(late_changes(&mut honest).await, listed);
        assert!(listed.iter().all(|late| late.device_id == device));
        let kept = listed
            .iter()
            .find(|late| late.row_key == "note-kept")
            .unwrap();
        assert_eq!(kept.presentation.title, "Late");
    });
}

#[test]
fn an_honest_edit_of_a_late_row_keeps_its_effects_and_a_restore_brings_the_row_back() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(202).await;
        edit(&mut honest, &note("note-shared", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;

        // The removed device creates two rows past its cutoff; the honest member applies them
        // and retitles one, which sees the late create.
        edit(
            &mut removed,
            &format!(
                "{}{}",
                note("note-late", "Late", 1),
                note("note-dropped", "Dropped", 2)
            ),
        )
        .await;
        deliver(&mut honest, &mut removed).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-late'",
        )
        .await;
        let device = removed.device_id.clone();
        let removal = anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut honest).await;
        assert_eq!(
            anchor.applied_vector().await.get(&honest.writer()),
            honest.stored_vector().await.get(&honest.writer()),
            "the honest edit of a late row applies on the anchor"
        );
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-late").await,
                None,
                "a late row stays hidden"
            );
            assert_eq!(title(replica, "note-dropped").await, None);
            assert_eq!(
                late_rows(replica).await,
                vec![
                    ("note-dropped".to_owned(), LateKind::Created),
                    ("note-late".to_owned(), LateKind::Created),
                ]
            );
        }

        // An admin restores one row and discards the other.
        let listed = late_changes(&mut anchor).await;
        let late = listed
            .iter()
            .find(|late| late.row_key == "note-late")
            .unwrap()
            .clone();
        let report = anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &late,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
        assert!(report.sealed_ops > 0);
        let decide = |key: &str, resolution| LateRow {
            table: NOTES_TABLE,
            row: RowKey::new(key.to_owned()).unwrap(),
            resolution,
        };
        let resolve = |rows| {
            Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows,
            })
        };
        anchor
            .sequence(resolve(vec![
                decide("note-dropped", LateResolution::Discard),
                decide("note-late", LateResolution::Restore),
            ]))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        let mut observer = Replica::new(3, 202).await;
        deliver(&mut observer, &mut anchor).await;
        let settled = anchor.domain_dump().await;
        for replica in [&mut anchor, &mut honest, &mut observer] {
            assert_eq!(
                title(replica, "note-late").await.as_deref(),
                Some("Honest"),
                "a restore keeps the newer honest value"
            );
            assert_eq!(title(replica, "note-dropped").await, None);
            assert!(late_changes(replica).await.is_empty());
            assert_eq!(replica.domain_dump().await, settled);
        }

        // Deciding again changes nothing: the first decision on a row stays.
        let again = anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &late,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(again.sealed_ops, 0);
        anchor
            .sequence(resolve(vec![
                decide("note-dropped", LateResolution::Restore),
                decide("note-late", LateResolution::Discard),
            ]))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(replica.domain_dump().await, settled);
            replica.check_invariants().await;
        }
    });
}

/// A removed device deletes a row no operation may delete, past its cutoff: the deletion lists no
/// late change, a restore of a listing kept for it is refused, and the admin's writer keeps
/// applying on its peers, which hold such a deletion as invalid.
#[test]
fn a_late_tombstone_of_a_never_deleted_row_is_neither_listed_nor_restored() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(215).await;
        edit(
            &mut removed,
            "UPDATE distractions_settings SET desktop_enabled = 0",
        )
        .await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        let device = removed.device_id.clone();
        let removal = anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;

        let forged = forge_change(
            &mut removed,
            Change {
                table: SETTINGS_TABLE,
                row: RowKey::new(SETTINGS_KEY).unwrap(),
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: None,
                cause: None,
            },
        )
        .await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                replica.receive(std::slice::from_ref(&forged)).await,
                vec![StoreOutcome::Stored {
                    held: Some(HoldReason::Invalid)
                }]
            );
            assert_eq!(late_rows(replica).await, Vec::new());
        }

        // The seal boundary refuses a listing that reaches it anyway.
        anchor
            .exec(&format!(
                "INSERT INTO sync_late_changes (removal_revision, table_id, row_key, change, mask)
                 VALUES ({removal}, {table}, {key}, 'tombstone', 0)",
                table = SETTINGS_TABLE.0,
                key = quoted(SETTINGS_KEY),
            ))
            .await;
        let listed = late_changes(&mut anchor).await;
        assert_eq!(listed.len(), 1);
        let restored = anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &listed[0],
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await;
        assert!(
            matches!(restored, Err(SyncError::InvalidRequest(_))),
            "{restored:?}"
        );
        assert_eq!(
            anchor.count("SELECT count(*) FROM sync_tombstones").await,
            0
        );

        // The decision still sequences and clears the listing.
        anchor
            .sequence(Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows: vec![LateRow {
                    table: SETTINGS_TABLE,
                    row: RowKey::new(SETTINGS_KEY).unwrap(),
                    resolution: LateResolution::Restore,
                }],
            }))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();

        edit(&mut anchor, &note("note-after", "After", 0)).await;
        deliver(&mut honest, &mut anchor).await;
        assert_eq!(
            honest.applied_vector().await.get(&anchor.writer()),
            anchor.stored_vector().await.get(&anchor.writer()),
            "operations after the decision apply on peers"
        );
        assert_eq!(
            title(&mut honest, "note-after").await.as_deref(),
            Some("After")
        );
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM distractions_settings")
                    .await,
                1
            );
            assert!(late_changes(replica).await.is_empty());
            replica.check_invariants().await;
        }
    });
}

#[test]
fn late_operations_stored_after_their_removal_list_their_rows_until_the_next_engine_write() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(209).await;
        edit(&mut honest, &note("note-kept", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        let device = removed.device_id.clone();
        anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        let before = honest.domain_dump().await;

        // The removal applied before the late operations arrive, so storing them excludes them
        // and lists the rows they write for the next engine write.
        edit(
            &mut removed,
            &format!(
                "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-kept';{}",
                note("note-late", "Late", 1)
            ),
        )
        .await;
        let known = honest.stored_vector().await;
        let late = removed.ops_since(&known).await;
        assert!(!late.is_empty());
        let outcomes = honest.receive(&late).await;
        assert!(outcomes.iter().all(|outcome| *outcome
            == StoreOutcome::Stored {
                held: Some(HoldReason::Invalid)
            }));
        let listed = "SELECT count(*) FROM sync_stale_rows
                      WHERE row_key IN ('note-kept', 'note-late')";
        assert_eq!(honest.count(listed).await, 2);
        assert_eq!(honest.domain_dump().await, before);

        honest.apply_all().await.unwrap();
        assert_eq!(
            honest.count("SELECT count(*) FROM sync_stale_rows").await,
            0
        );
        assert_eq!(honest.domain_dump().await, before, "late rows stay hidden");
        assert_eq!(
            late_rows(&mut honest).await,
            vec![
                ("note-kept".to_owned(), LateKind::Values),
                ("note-late".to_owned(), LateKind::Created),
            ]
        );
        honest.check_invariants().await;
    });
}

/// The anchor, a removed device, and an honest member sharing a note, a contact, and the seeded
/// rows the device wrote, with the removal `salt` selects, known to all but the removed device.
async fn after_removal(salt: u64) -> (Replica, Replica, Replica, u64) {
    let (mut anchor, mut removed, mut honest) = trio(salt).await;
    let now = removed.now_ms;
    removed
        .engine
        .seed_captures(&mut removed.conn, now)
        .await
        .unwrap();
    edit(
        &mut removed,
        &format!(
            "{note}
             INSERT INTO contact_card (key, card_nonce, updated_at)
             VALUES ('{CARD_KEY}', 'AAAAAAAAAAAAAAAAAAAAAA', '{CREATED_AT}');
             INSERT INTO contacts (id, public_key, display_name, state, created_at, updated_at)
             VALUES ('contact-{PEER_KEY}', '{PEER_KEY}', 'Ana', 'active', '{CREATED_AT}',
                     '{CREATED_AT}');",
            note = note("note-kept", "Draft", 0),
        ),
    )
    .await;
    deliver(&mut anchor, &mut removed).await;
    deliver(&mut honest, &mut anchor).await;
    let device = removed.device_id.clone();
    let removal = anchor.remove(&device).await.unwrap();
    deliver(&mut honest, &mut anchor).await;
    (anchor, removed, honest, removal)
}

/// A write of the note's Immutable creation group, which only a re-add of a derived-key row may
/// carry.
async fn immutable_write(removed: &mut Replica, row: &str) -> Change {
    let mut change = sealed_create(removed, NOTES_TABLE, "note-kept").await;
    change.row = RowKey::new(row).unwrap();
    change.action = ChangeAction::Write;
    change
        .groups
        .retain(|value| value.group == note_group::CREATED);
    change
}

/// A write of a note's title holding `field`.
fn title_write(row: &str, field: Field) -> Change {
    Change {
        table: NOTES_TABLE,
        row: RowKey::new(row).unwrap(),
        action: ChangeAction::Write,
        groups: vec![GroupValue::new(
            NOTES_TABLE,
            note_group::TITLE,
            Value::new(vec![field]).unwrap(),
        )],
        replaced_by: None,
        cause: None,
    }
}

/// Changes every replica holds as invalid, named for the rule each breaks.
async fn refused_changes(removed: &mut Replica) -> Vec<(&'static str, Change)> {
    let mut refused_key = sealed_create(removed, CARD_TABLE, CARD_KEY).await;
    refused_key.row = RowKey::new("x").unwrap();
    let mut builtin_claim =
        sealed_create(removed, PROFILES_TABLE, "access-profile:read-only").await;
    builtin_claim.row = RowKey::new("access-profile:aaa").unwrap();
    vec![
        ("row key", refused_key),
        ("column value", title_write("note-kept", Field::Integer(1))),
        (
            "derived-key tombstone",
            Change {
                table: CONTACTS_TABLE,
                row: RowKey::new(PEER_KEY).unwrap(),
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: None,
                cause: None,
            },
        ),
        (
            "immutable group write",
            immutable_write(removed, "note-kept").await,
        ),
        ("built-in profile identity", builtin_claim),
    ]
}

/// A removed device signs operations every replica holds as invalid, past its cutoff: none lists
/// a late change, and whatever a replica did list is restored without stalling the admin's writer
/// on its peers.
#[test]
fn operations_every_replica_refuses_neither_list_nor_stall_a_restore() {
    block_on(async {
        const SALT: u64 = 220;
        let (_, mut sample, _, _) = after_removal(SALT).await;
        let mut listed = Vec::new();
        let mut stalled = Vec::new();
        for (name, change) in refused_changes(&mut sample).await {
            let (mut anchor, mut removed, mut honest, removal) = after_removal(SALT).await;
            let forged = forge_change(&mut removed, change).await;
            for replica in [&mut anchor, &mut honest] {
                assert_eq!(
                    replica.receive(std::slice::from_ref(&forged)).await,
                    vec![StoreOutcome::Stored {
                        held: Some(HoldReason::Invalid)
                    }],
                    "{name}"
                );
                replica.apply_all().await.unwrap();
            }
            if !late_rows(&mut anchor).await.is_empty() {
                listed.push(name);
                restore_all(&mut anchor, removal).await;
            }

            edit(&mut anchor, &note("note-after", "After", 1)).await;
            deliver(&mut honest, &mut anchor).await;
            if honest.applied_vector().await.get(&anchor.writer())
                != anchor.stored_vector().await.get(&anchor.writer())
            {
                stalled.push(name);
            }
        }
        let nothing = Vec::<&str>::new();
        assert_eq!(
            (listed, stalled),
            (nothing.clone(), nothing),
            "refused operations that listed, and that stalled the admin's writer"
        );
    });
}

/// A member stores an operation it refuses before it learns of the removal that excludes it:
/// deriving the late changes again lists the valid late edit and not the refused operation.
#[test]
fn a_refused_operation_stored_before_the_removal_stays_unlisted_after_it() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(221).await;
        edit(
            &mut removed,
            &format!(
                "{}{}",
                note("note-kept", "Draft", 0),
                note("note-other", "Other", 1)
            ),
        )
        .await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;

        edit(
            &mut removed,
            "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-kept'",
        )
        .await;
        let known = honest.stored_vector().await;
        let late = removed.ops_since(&known).await;
        assert!(!late.is_empty());
        honest.receive(&late).await;
        let change = immutable_write(&mut removed, "note-other").await;
        let forged = forge_change(&mut removed, change).await;
        assert_eq!(
            honest.receive(&[forged]).await,
            vec![StoreOutcome::Stored {
                held: Some(HoldReason::Invalid)
            }]
        );

        let device = removed.device_id.clone();
        anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;

        assert_eq!(
            late_rows(&mut honest).await,
            vec![("note-kept".to_owned(), LateKind::Values)]
        );
        honest.check_invariants().await;
    });
}

/// Each late write of a row nothing created is valid alone, but restoring it would seal a create
/// of a part of the row: the restore is refused with nothing sealed, and a discard still clears
/// the listing.
#[test]
fn restoring_a_late_write_of_a_row_nothing_created_is_refused_and_can_be_discarded() {
    block_on(async {
        let (mut anchor, mut removed, mut honest, removal) = after_removal(222).await;
        let forged =
            forge_change(&mut removed, title_write("note-ghost", text_field("Ghost"))).await;
        for replica in [&mut anchor, &mut honest] {
            replica.receive(std::slice::from_ref(&forged)).await;
            replica.apply_all().await.unwrap();
        }
        let listed = late_changes(&mut anchor).await;
        assert_eq!(listed.len(), 1);
        let before = anchor.stored_vector().await.get(&anchor.writer());

        let restored = anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &listed[0],
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await;
        assert!(
            matches!(restored, Err(SyncError::InvalidRequest(_))),
            "{restored:?}"
        );
        assert_eq!(anchor.stored_vector().await.get(&anchor.writer()), before);

        anchor
            .sequence(Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows: vec![LateRow {
                    table: NOTES_TABLE,
                    row: RowKey::new("note-ghost").unwrap(),
                    resolution: LateResolution::Discard,
                }],
            }))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        edit(&mut anchor, &note("note-after", "After", 1)).await;
        deliver(&mut honest, &mut anchor).await;
        assert_eq!(
            honest.applied_vector().await.get(&anchor.writer()),
            anchor.stored_vector().await.get(&anchor.writer()),
            "operations after the discard apply on peers"
        );
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(title(replica, "note-ghost").await, None);
            assert!(late_changes(replica).await.is_empty());
            replica.check_invariants().await;
        }
    });
}

/// The key of the host rule the re-add tests share.
fn host_rule_key() -> String {
    derived_key(HOST_RULE_KEY_PREFIX, &["blocked", "", "example.com"])
        .unwrap()
        .as_str()
        .to_owned()
}

/// A late re-add of a derived-key row writes every group, its Immutable key group among them. An
/// honest edit newer than one of those groups leaves a partial write, which carries no Immutable
/// group, so the restore applies on peers and keeps the honest value.
#[test]
fn restoring_a_late_readd_past_a_newer_honest_edit_leaves_the_immutable_groups() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(223).await;
        edit(
            &mut honest,
            &format!(
                "INSERT INTO distractions_host_rules (id, list_kind, scope, host, created_at_ms)
                 VALUES ({key}, 'blocked', '', 'example.com', {AT_MS})",
                key = quoted(&host_rule_key()),
            ),
        )
        .await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;

        edit(&mut removed, "DELETE FROM distractions_host_rules").await;
        edit(
            &mut removed,
            &format!(
                "INSERT INTO distractions_host_rules (id, list_kind, scope, host, created_at_ms)
                 VALUES ({key}, 'blocked', '', 'example.com', {at})",
                key = quoted(&host_rule_key()),
                at = AT_MS + 1,
            ),
        )
        .await;
        let readd = sealed_change(
            &mut removed,
            HOST_RULES_TABLE,
            &host_rule_key(),
            ChangeAction::Write,
        )
        .await;
        assert!(
            readd
                .groups
                .iter()
                .any(|value| value.group == host_group::RULE)
        );

        let removal = anchor.remove(&removed.device_id.clone()).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut removed).await;
        honest.now_ms += ACKNOWLEDGEMENT_INTERVAL_MS;
        edit(
            &mut honest,
            "UPDATE distractions_host_rules SET enabled = 0",
        )
        .await;
        deliver(&mut anchor, &mut honest).await;

        assert_eq!(late_rows(&mut anchor).await.len(), 1);
        restore_all(&mut anchor, removal).await;
        deliver(&mut honest, &mut anchor).await;
        assert_eq!(
            honest.applied_vector().await.get(&anchor.writer()),
            anchor.stored_vector().await.get(&anchor.writer()),
            "the restored write applies on peers"
        );
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM distractions_host_rules WHERE enabled = 0")
                    .await,
                1,
                "the honest edit stands"
            );
            assert!(late_changes(replica).await.is_empty());
            replica.check_invariants().await;
        }
    });
}

/// An operation of a newer manifest may be valid there, and nothing derives the late changes again
/// when this version updates, so it stays listed and restores what passes this manifest.
#[test]
fn a_late_operation_of_a_newer_manifest_stays_listed_and_restores() {
    block_on(async {
        let (mut anchor, mut removed, mut honest, removal) = after_removal(224).await;
        let forged = forge_change(&mut removed, title_write("note-kept", text_field("Late"))).await;
        let mut newer = Envelope::decode(&forged).unwrap().operation().unwrap();
        newer.header.manifest_version += 1;
        let newer = newer.seal(&removed.identity.key).unwrap().into_bytes();
        for replica in [&mut anchor, &mut honest] {
            replica.receive(std::slice::from_ref(&newer)).await;
            replica.apply_all().await.unwrap();
            assert_eq!(
                late_rows(replica).await,
                vec![("note-kept".to_owned(), LateKind::Values)]
            );
        }

        restore_all(&mut anchor, removal).await;
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Late"));
            assert!(late_changes(replica).await.is_empty());
        }
    });
}

/// SQL creating a teammate participant with no revisions.
fn participant(id: &str) -> String {
    format!(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES ({}, 'ai_teammate', 'Late', '{CREATED_AT}', '{CREATED_AT}');",
        quoted(id),
    )
}

/// Exclusion removes what a removed device created in a table no operation deletes from, both a
/// row nothing else wrote and one an honest member edited after seeing it.
#[test]
fn excluded_writer_row_of_a_never_deleted_table_stays_removable() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(208).await;
        edit(&mut honest, &note("note-shared", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;

        edit(
            &mut removed,
            &format!(
                "{}{}",
                participant("participant:dropped"),
                participant("participant:edited")
            ),
        )
        .await;
        deliver(&mut honest, &mut removed).await;
        edit(
            &mut honest,
            &format!(
                "UPDATE chat_participants SET display_name = 'Honest', updated_at = '{EDITED_AT}'
                 WHERE id = 'participant:edited'"
            ),
        )
        .await;
        let device = removed.device_id.clone();
        anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;

        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM chat_participants
                         WHERE id IN ('participant:dropped', 'participant:edited')"
                    )
                    .await,
                0,
                "replica {} kept a row of the removed device",
                replica.index
            );
        }
    });
}

/// A removed device creates, past its cutoff, rows of never-deleted tables that honest members
/// already created: the late creates list, but the rows stay with their honest values whether
/// the late change is undecided or discarded.
#[test]
fn a_late_create_of_an_honestly_created_never_deleted_row_keeps_it() {
    const PARTICIPANT: &str = "participant:honest";
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(216).await;
        edit(
            &mut honest,
            &format!(
                "UPDATE distractions_settings SET desktop_enabled = 0;{}",
                participant(PARTICIPANT)
            ),
        )
        .await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        edit(&mut removed, &note("note-early", "Early", 0)).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        let settings = sealed_create(&mut honest, SETTINGS_TABLE, SETTINGS_KEY).await;
        let member = sealed_create(&mut honest, PARTICIPANTS_TABLE, PARTICIPANT).await;
        let device = removed.device_id.clone();
        let removal = anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;

        let forged = forge_changes(&mut removed, vec![settings, member]).await;
        for replica in [&mut anchor, &mut honest] {
            replica.receive(std::slice::from_ref(&forged)).await;
            replica.apply_all().await.unwrap();
            assert_eq!(
                late_rows(replica).await,
                vec![
                    (SETTINGS_KEY.to_owned(), LateKind::Values),
                    (PARTICIPANT.to_owned(), LateKind::Values),
                ]
            );
        }
        let survives = async |replica: &mut Replica| {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM distractions_settings WHERE desktop_enabled = 0")
                    .await,
                1,
                "replica {} lost the honest settings",
                replica.index
            );
            assert_eq!(
                replica
                    .count(&format!(
                        "SELECT count(*) FROM chat_participants WHERE id = {}",
                        quoted(PARTICIPANT)
                    ))
                    .await,
                1,
                "replica {} lost the honest participant",
                replica.index
            );
        };
        for replica in [&mut anchor, &mut honest] {
            survives(replica).await;
        }

        let discard = |table, key: &str| LateRow {
            table,
            row: RowKey::new(key.to_owned()).unwrap(),
            resolution: LateResolution::Discard,
        };
        let mut rows = vec![
            discard(SETTINGS_TABLE, SETTINGS_KEY),
            discard(PARTICIPANTS_TABLE, PARTICIPANT),
        ];
        rows.sort_by(|a, b| (a.table, a.row.as_str()).cmp(&(b.table, b.row.as_str())));
        anchor
            .sequence(Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows,
            }))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            survives(replica).await;
            replica.check_invariants().await;
        }
    });
}

/// The color a late create writes, unlike the default.
const LATE_COLOR: i64 = 5;
/// A note only the removed device creates.
const LATE_CREATED: &str = "note-late";

/// The anchor, a device removed after its first operation, and an honest member that created
/// `note-kept`, which every replica knows, with the removal revision.
async fn removed_after_honest_row(salt: u64) -> (Replica, Replica, Replica, u64) {
    let (mut anchor, mut removed, mut honest) = trio(salt).await;
    edit(&mut honest, &note("note-kept", "Draft", 0)).await;
    spread(&mut anchor, &mut honest).await;
    deliver(&mut removed, &mut honest).await;
    edit(&mut removed, &note("note-early", "Early", 1)).await;
    deliver(&mut anchor, &mut removed).await;
    deliver(&mut honest, &mut anchor).await;
    let removal = anchor.remove(&removed.device_id.clone()).await.unwrap();
    deliver(&mut honest, &mut anchor).await;
    (anchor, removed, honest, removal)
}

/// A Create of `row` past the removed device's cutoff, built from the honest create of
/// `note-kept`, with the title `Late` and another color.
pub(super) async fn late_create(removed: &mut Replica, honest: &mut Replica, row: &str) -> Vec<u8> {
    let mut create = sealed_create(honest, NOTES_TABLE, "note-kept").await;
    create.row = RowKey::new(row).unwrap();
    let create = with_group(create, note_group::TITLE, vec![text_field("Late")]);
    let create = with_group(create, note_group::COLOR, vec![Field::Integer(LATE_COLOR)]);
    forge_change(removed, create).await
}

/// Stores `forged` on every replica in `replicas` and applies it.
async fn take_late(replicas: [&mut Replica; 2], forged: &[u8]) {
    for replica in replicas {
        replica.receive(&[forged.to_vec()]).await;
        replica.apply_all().await.unwrap();
    }
}

/// Sequences `resolution` of the late change of `row` from the anchor and spreads it.
async fn decide(
    anchor: &mut Replica,
    honest: &mut Replica,
    removal: u64,
    row: &str,
    resolution: LateResolution,
) {
    if resolution == LateResolution::Restore {
        let late = late_changes(anchor)
            .await
            .into_iter()
            .find(|late| late.row_key == row)
            .expect("the late change is listed");
        anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &late,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
    }
    anchor
        .sequence(Entry::ResolveLate(ResolveLate {
            removal_revision: removal,
            rows: vec![LateRow {
                table: NOTES_TABLE,
                row: RowKey::new(row).unwrap(),
                resolution,
            }],
        }))
        .await
        .unwrap()
        .unwrap();
    anchor.apply_all().await.unwrap();
    deliver(honest, anchor).await;
}

/// The color a replica shows for a note.
async fn color(replica: &mut Replica, id: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT CAST(color AS TEXT) FROM quick_notes WHERE id = {}",
            quoted(id)
        ))
        .await
}

/// A removed device signs a Create of a row an honest member created, past its cutoff: the
/// honest row stays on every replica and the late change lists as values.
#[test]
fn excluded_create_leaves_an_honest_row_visible() {
    block_on(async {
        let (mut anchor, mut removed, mut honest, _) = removed_after_honest_row(240).await;
        let forged = late_create(&mut removed, &mut honest, "note-kept").await;
        take_late([&mut anchor, &mut honest], &forged).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-kept").await.as_deref(),
                Some("Draft"),
                "replica {} hid the honest row",
                replica.index
            );
            assert_eq!(
                late_rows(replica).await,
                vec![("note-kept".to_owned(), LateKind::Values)]
            );
            replica.check_invariants().await;
        }
    });
}

/// Discarding a late Create of an honestly created row keeps the row, also after a later
/// honest edit.
#[test]
fn discarding_an_excluded_create_keeps_the_honest_row() {
    block_on(async {
        let (mut anchor, mut removed, mut honest, removal) = removed_after_honest_row(241).await;
        let forged = late_create(&mut removed, &mut honest, "note-kept").await;
        take_late([&mut anchor, &mut honest], &forged).await;
        decide(
            &mut anchor,
            &mut honest,
            removal,
            "note-kept",
            LateResolution::Discard,
        )
        .await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-kept").await.as_deref(),
                Some("Draft"),
                "replica {} lost the honest row",
                replica.index
            );
            assert!(late_changes(replica).await.is_empty());
        }
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-kept';",
        )
        .await;
        spread(&mut anchor, &mut honest).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Edited"));
            replica.check_invariants().await;
        }
    });
}

/// Restoring a late Create of an honestly created row writes it as values: a group an honest
/// edit wrote later keeps the honest value, and a group it did not touch takes the late value.
#[test]
fn excluded_create_restores_as_values_below_honest_edits() {
    block_on(async {
        let (mut anchor, mut removed, mut honest, removal) = removed_after_honest_row(242).await;
        let forged = late_create(&mut removed, &mut honest, "note-kept").await;
        honest.now_ms += 10_000;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-kept';",
        )
        .await;
        spread(&mut anchor, &mut honest).await;
        take_late([&mut anchor, &mut honest], &forged).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-kept").await.as_deref(),
                Some("Edited"),
                "replica {} hid the honest row before the decision",
                replica.index
            );
        }
        anchor.now_ms += 20_000;
        decide(
            &mut anchor,
            &mut honest,
            removal,
            "note-kept",
            LateResolution::Restore,
        )
        .await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-kept").await.as_deref(),
                Some("Edited"),
                "replica {} lost the honest title",
                replica.index
            );
            assert_eq!(
                color(replica, "note-kept").await,
                Some(LATE_COLOR.to_string()),
                "replica {} left out the late color",
                replica.index
            );
            assert!(late_changes(replica).await.is_empty());
            replica.check_invariants().await;
        }
    });
}

/// A row only the removed device created stays hidden as a late create: Restore brings it back
/// and Discard keeps it gone.
#[test]
fn late_row_without_honest_creation_stays_hidden() {
    block_on(async {
        for (salt, resolution, shown) in [
            (243, LateResolution::Restore, Some("Late")),
            (244, LateResolution::Discard, None),
        ] {
            let (mut anchor, mut removed, mut honest, removal) =
                removed_after_honest_row(salt).await;
            let forged = late_create(&mut removed, &mut honest, LATE_CREATED).await;
            take_late([&mut anchor, &mut honest], &forged).await;
            for replica in [&mut anchor, &mut honest] {
                assert_eq!(title(replica, LATE_CREATED).await, None);
                assert_eq!(
                    late_rows(replica).await,
                    vec![(LATE_CREATED.to_owned(), LateKind::Created)]
                );
            }
            decide(&mut anchor, &mut honest, removal, LATE_CREATED, resolution).await;
            for replica in [&mut anchor, &mut honest] {
                assert_eq!(
                    title(replica, LATE_CREATED).await.as_deref(),
                    shown,
                    "{resolution:?} on replica {}",
                    replica.index
                );
                assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Draft"));
                replica.check_invariants().await;
            }
        }
    });
}

/// A late value whose clock runs far ahead of the removal does not beat an honest edit sealed
/// after the removal entry by a device that had not seen it yet.
#[test]
fn restore_caps_a_forged_late_clock_at_the_removal() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(245).await;
        edit(&mut honest, &note("note-kept", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        edit(&mut removed, &note("note-early", "Early", 1)).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        let removal = anchor.remove(&removed.device_id.clone()).await.unwrap();

        honest.now_ms += 10_000;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-kept';",
        )
        .await;
        removed.now_ms += 24 * 3_600_000;
        let forged = forge_change(&mut removed, title_write("note-kept", text_field("Late"))).await;
        spread(&mut anchor, &mut honest).await;
        take_late([&mut anchor, &mut honest], &forged).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Honest"));
        }
        anchor.now_ms += 20_000;
        decide(
            &mut anchor,
            &mut honest,
            removal,
            "note-kept",
            LateResolution::Restore,
        )
        .await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-kept").await.as_deref(),
                Some("Honest"),
                "replica {} let a forged late clock win",
                replica.index
            );
            replica.check_invariants().await;
        }
    });
}

#[test]
fn restoring_late_values_and_a_late_deletion_writes_them_as_the_admin() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(203).await;
        edit(
            &mut honest,
            &format!(
                "{}{}{}",
                note("note-kept", "Draft", 0),
                note("note-gone", "Old", 1),
                format_args!(
                    "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms, order_key)
                     VALUES ('playlist-shared', 'Focus', {AT_MS}, {AT_MS}, {});",
                    quoted(&rank(0))
                ),
            ),
        )
        .await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        edit(
            &mut removed,
            &format!(
                "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-kept';
                 DELETE FROM quick_notes WHERE id = 'note-gone';
                 UPDATE music_playlists SET name = 'Late', updated_at_ms = {}
                 WHERE id = 'playlist-shared';",
                AT_MS + 1
            ),
        )
        .await;
        let removal = anchor.remove(&removed.device_id.clone()).await.unwrap();
        deliver(&mut anchor, &mut removed).await;
        let listed = late_changes(&mut anchor).await;
        assert_eq!(listed.len(), 3);
        assert!(listed.iter().all(|late| late.restorable));
        let mut rows = Vec::new();
        for late in &listed {
            anchor
                .engine
                .restore_late_change(
                    &mut anchor.conn,
                    &anchor.ctx,
                    late,
                    &mut anchor.identity.local(),
                    anchor.now_ms,
                )
                .await
                .unwrap();
            rows.push(LateRow {
                table: late.table,
                row: RowKey::new(late.row_key.clone()).unwrap(),
                resolution: LateResolution::Restore,
            });
        }
        rows.sort_by(|a, b| (a.table, a.row.as_str()).cmp(&(b.table, b.row.as_str())));
        anchor
            .sequence(Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows,
            }))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Late"));
            assert_eq!(title(replica, "note-gone").await, None);
            assert_eq!(
                replica
                    .scalar_text("SELECT name FROM music_playlists WHERE id = 'playlist-shared'")
                    .await
                    .as_deref(),
                Some("Late")
            );
            assert!(late_changes(replica).await.is_empty());
        }
        assert!(listed.iter().any(|late| late.table == PLAYLISTS_TABLE));
    });
}

/// Restores every late change `anchor` lists for `removal` and sequences the decision.
pub(super) async fn restore_all(anchor: &mut Replica, removal: u64) {
    let mut rows = Vec::new();
    for late in late_changes(anchor).await {
        anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &late,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
        rows.push(LateRow {
            table: late.table,
            row: RowKey::new(late.row_key.clone()).unwrap(),
            resolution: LateResolution::Restore,
        });
    }
    anchor
        .sequence(Entry::ResolveLate(ResolveLate {
            removal_revision: removal,
            rows,
        }))
        .await
        .unwrap()
        .unwrap();
    anchor.apply_all().await.unwrap();
}

#[test]
fn an_anchor_a_handover_installed_still_restores_late_values_of_a_removal() {
    block_on(async {
        let (mut charter, mut removed, mut honest) = trio(205).await;
        charter.sequence(admin_entry(20)).await.unwrap().unwrap();
        let mut anchor = Replica::new(20, 205).await;
        edit(&mut honest, &note("note-kept", "Draft", 0)).await;
        spread(&mut charter, &mut honest).await;
        deliver(&mut anchor, &mut charter).await;
        deliver(&mut removed, &mut charter).await;
        let signed = anchor.handover_proposal(&device_admin(20)).await;
        let vector = charter.stored_vector().await;
        anchor
            .seal_handover(&signed, Some(vector))
            .await
            .unwrap()
            .unwrap();
        edit(
            &mut removed,
            "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-kept';",
        )
        .await;
        let removal = anchor.remove(&removed.device_id.clone()).await.unwrap();
        deliver(&mut anchor, &mut removed).await;
        assert_eq!(
            title(&mut anchor, "note-kept").await.as_deref(),
            Some("Draft")
        );
        assert_eq!(late_changes(&mut anchor).await.len(), 1);

        restore_all(&mut anchor, removal).await;
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Late"));
            assert!(late_changes(replica).await.is_empty());
        }
    });
}

/// The charter anchor applies a late edit of the removed device, releasing the value it
/// displaced; a handover then installs a successor that never stored the edit, and the successor
/// removes the device below it. Returns the charter, the successor, the honest member, and the
/// removed device.
async fn former_anchor_released_late_edit(salt: u64) -> [Replica; 4] {
    let (mut charter, mut removed, mut honest) = trio(salt).await;
    charter.sequence(admin_entry(20)).await.unwrap().unwrap();
    let mut successor = Replica::new(20, salt).await;
    edit(&mut honest, &note("note-kept", "Draft", 0)).await;
    spread(&mut charter, &mut honest).await;
    deliver(&mut successor, &mut charter).await;
    deliver(&mut removed, &mut charter).await;
    let before_late = charter.stored_vector().await;
    edit(
        &mut removed,
        "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-kept';",
    )
    .await;
    deliver(&mut charter, &mut removed).await;
    assert_eq!(
        title(&mut charter, "note-kept").await.as_deref(),
        Some("Late")
    );
    assert_eq!(
        displaced(&mut charter).await,
        0,
        "the anchor released the value"
    );
    assert_eq!(
        charter
            .count("SELECT count(*) FROM sync_released_marks")
            .await,
        2,
        "the anchor records what the honest and removed writers applied"
    );
    let signed = successor.handover_proposal(&device_admin(20)).await;
    successor
        .seal_handover(&signed, Some(before_late))
        .await
        .unwrap()
        .unwrap();
    successor.remove(&removed.device_id.clone()).await.unwrap();
    [charter, successor, honest, removed]
}

/// Converges the replicas still in the space and checks that each keeps the row with the same
/// title and the same domain rows.
async fn assert_row_survives(replicas: &mut [Replica]) {
    converge(replicas).await;
    let first = replicas[0].domain_dump().await;
    let kept = title(&mut replicas[0], "note-kept").await;
    assert!(kept.is_some(), "the row survives");
    for replica in replicas.iter_mut() {
        assert_eq!(
            title(replica, "note-kept").await,
            kept,
            "{}",
            replica.device_id
        );
        assert_eq!(replica.domain_dump().await, first, "{}", replica.device_id);
        replica.check_invariants().await;
    }
}

/// The highest stored sequence of `replica`'s own writer.
pub(super) async fn own_head(replica: &mut Replica) -> u64 {
    let writer = replica.writer();
    replica.stored_vector().await.get(&writer)
}

/// Whether sealing publishes a new operation of `replica`'s own writer, such as a re-seal.
async fn seals_own(replica: &mut Replica) -> bool {
    let before = own_head(replica).await;
    replica.seal().await.unwrap();
    own_head(replica).await > before
}

#[test]
fn a_former_anchor_reseals_the_groups_it_released_when_a_later_removal_excludes_them() {
    block_on(async {
        let [mut charter, mut successor, honest, _removed] =
            former_anchor_released_late_edit(206).await;
        // Only the former anchor stored the late edit, and it kept nothing the edit displaced.
        assert!(!seals_own(&mut successor).await);
        let before = own_head(&mut charter).await;
        deliver(&mut charter, &mut successor).await;
        assert!(
            charter
                .count("SELECT count(*) FROM sync_ops WHERE excluded_revision IS NOT NULL")
                .await
                > 0
        );
        // The former anchor seals the groups again before the exclusion materializes.
        assert!(
            own_head(&mut charter).await > before,
            "the former anchor re-seals"
        );
        assert_eq!(
            title(&mut charter, "note-kept").await.as_deref(),
            Some("Late"),
            "the current value is kept"
        );
        assert!(
            charter
                .count("SELECT count(*) FROM sync_restated_groups")
                .await
                > 0,
            "the re-seal is recorded as a restatement"
        );
        // The successor, as anchor of a handed over epoch, re-seals its own value once it stores
        // the late edit, so the replicas settle on one of the two re-seals.
        assert_row_survives(&mut [charter, successor, honest]).await;
    });
}

#[test]
fn anchor_marks_from_a_direct_exchange_and_relayed_acknowledgements_release_displaced_versions() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(204).await;
        edit(&mut honest, &note("note-shared", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;

        // A direct exchange: the anchor reports storing the superseding operation.
        edit(
            &mut removed,
            "UPDATE quick_notes SET title = 'First' WHERE id = 'note-shared'",
        )
        .await;
        deliver(&mut honest, &mut removed).await;
        assert!(displaced(&mut honest).await > 0);
        deliver(&mut anchor, &mut removed).await;
        let stored = anchor.stored_vector().await;
        let anchor_device = anchor.device_id.clone();
        assert!(
            !honest
                .engine
                .record_anchor_marks(&mut honest.conn, &honest.ctx, "device-other", &stored)
                .await
                .unwrap(),
            "only the anchor's report counts"
        );
        assert!(displaced(&mut honest).await > 0);
        assert!(
            honest
                .engine
                .record_anchor_marks(&mut honest.conn, &honest.ctx, &anchor_device, &stored)
                .await
                .unwrap()
        );
        assert_eq!(displaced(&mut honest).await, 0);

        // A relayed acknowledgement: the anchor applied the superseding operation, so its
        // acknowledgement confirms storing it.
        edit(
            &mut removed,
            "UPDATE quick_notes SET title = 'Second' WHERE id = 'note-shared'",
        )
        .await;
        deliver(&mut honest, &mut removed).await;
        assert!(displaced(&mut honest).await > 0);
        deliver(&mut anchor, &mut removed).await;
        anchor.now_ms += ACKNOWLEDGEMENT_INTERVAL_MS;
        let acknowledged = anchor
            .engine
            .acknowledge(
                &mut anchor.conn,
                &anchor.ctx,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(acknowledged.sealed_ops, 1);
        deliver(&mut honest, &mut anchor).await;
        assert_eq!(displaced(&mut honest).await, 0);

        // Both edits are within the cutoff, so a removal now keeps them.
        anchor.remove(&removed.device_id.clone()).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(
                title(replica, "note-shared").await.as_deref(),
                Some("Second")
            );
            assert!(late_changes(replica).await.is_empty());
        }
    });
}

#[test]
fn the_anchor_and_the_removed_device_retain_nothing_and_keep_their_own_operations() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(205).await;
        edit(&mut honest, &note("note-shared", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        deliver(&mut removed, &mut anchor).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-shared'",
        )
        .await;
        deliver(&mut anchor, &mut honest).await;
        assert_eq!(
            displaced(&mut anchor).await,
            0,
            "the anchor retains nothing"
        );

        edit(
            &mut removed,
            "UPDATE quick_notes SET title = 'Late' WHERE id = 'note-shared'",
        )
        .await;
        anchor.remove(&removed.device_id.clone()).await.unwrap();
        deliver(&mut removed, &mut anchor).await;
        assert!(removed.device_removed().await);
        assert_eq!(
            removed
                .count("SELECT count(*) FROM sync_ops WHERE excluded_revision IS NOT NULL")
                .await,
            0,
            "the removed device never excludes its own operations"
        );
        assert_eq!(
            removed.applied_vector().await.get(&removed.writer()),
            removed.stored_vector().await.get(&removed.writer())
        );
    });
}

#[test]
fn status_counts_own_changes_until_the_anchor_confirms_storing_them() {
    block_on(async {
        let (mut anchor, _removed, mut honest) = trio(207).await;
        spread(&mut anchor, &mut honest).await;
        edit(&mut honest, &note("note-first", "Draft", 0)).await;
        edit(&mut honest, &note("note-second", "Draft", 1)).await;
        let status = honest
            .engine
            .status(&mut honest.conn, &honest.ctx)
            .await
            .unwrap();
        assert_eq!(status.unconfirmed, 2);
        assert!(status.oldest_unconfirmed_ms.is_some());

        deliver(&mut anchor, &mut honest).await;
        let stored = anchor.stored_vector().await;
        let anchor_device = anchor.device_id.clone();
        assert!(
            honest
                .engine
                .record_anchor_marks(&mut honest.conn, &honest.ctx, &anchor_device, &stored)
                .await
                .unwrap()
        );
        let status = honest
            .engine
            .status(&mut honest.conn, &honest.ctx)
            .await
            .unwrap();
        assert_eq!(status.unconfirmed, 0);
        assert_eq!(status.oldest_unconfirmed_ms, None);

        edit(&mut anchor, &note("note-anchor", "Draft", 2)).await;
        let status = anchor
            .engine
            .status(&mut anchor.conn, &anchor.ctx)
            .await
            .unwrap();
        assert_eq!(status.unconfirmed, 0, "the anchor confirms its own changes");
    });
}

#[test]
fn republish_fills_an_immutable_group_left_without_a_version() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(214).await;
        edit(&mut honest, &note("note-shared", "Draft", 0)).await;
        spread(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;

        // The honest edit keeps the row in the merge state after the late create leaves it.
        edit(&mut removed, &note("note-late", "Late", 1)).await;
        let created_at = removed
            .scalar_text("SELECT created_at FROM quick_notes WHERE id = 'note-late'")
            .await;
        deliver(&mut honest, &mut removed).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-late'",
        )
        .await;
        let device = removed.device_id.clone();
        let removal = anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(created_versions(replica, "note-late").await, 0);
        }

        let late = late_changes(&mut anchor).await.remove(0);
        anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &late,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
        anchor
            .sequence(Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows: vec![LateRow {
                    table: NOTES_TABLE,
                    row: RowKey::new("note-late".to_owned()).unwrap(),
                    resolution: LateResolution::Restore,
                }],
            }))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        let mut observer = Replica::new(3, 214).await;
        deliver(&mut observer, &mut anchor).await;
        for replica in [&mut anchor, &mut honest, &mut observer] {
            assert_eq!(created_versions(replica, "note-late").await, 1);
            assert_eq!(
                replica
                    .scalar_text("SELECT created_at FROM quick_notes WHERE id = 'note-late'")
                    .await,
                created_at
            );
        }
    });
}

const HELD_NOTE: &str = "note-held";
const SECOND_CREATED_AT: &str = "2027-01-03T00:00:00.000Z";

/// The anchor, two writers the anchor has not heard from, a writer of further Creates, and the
/// two replicas whose delivery orders the tests compare.
struct Orders {
    anchor: Replica,
    first: Replica,
    second: Replica,
    third: Replica,
    before: Replica,
    after: Replica,
}

impl Orders {
    async fn new(salt: u64) -> Self {
        let (mut anchor, mut first, mut second) = trio(salt).await;
        let (mut third, mut before, mut after) = (
            Replica::new(3, salt).await,
            Replica::new(4, salt).await,
            Replica::new(5, salt).await,
        );
        edit(&mut second, &note("note-shared", "Draft", 0)).await;
        spread(&mut anchor, &mut second).await;
        for replica in [&mut first, &mut third, &mut before, &mut after] {
            deliver(replica, &mut anchor).await;
        }
        Self {
            anchor,
            first,
            second,
            third,
            before,
            after,
        }
    }

    /// Removes `device` on the anchor and delivers the removal to `replica`.
    async fn remove(anchor: &mut Replica, replica: &mut Replica, device: &str) {
        anchor.remove(device).await.unwrap();
        deliver(replica, anchor).await;
    }
}

/// The Create of [`HELD_NOTE`] that `creator` sealed, signed again by `writer` with `created_at`.
/// A forged operation sees what the writer's head saw, so `writer` first seals an edit.
async fn create_again(creator: &mut Replica, writer: &mut Replica, created_at: &str) -> Vec<u8> {
    edit(
        writer,
        &format!(
            "UPDATE quick_notes SET title = {} WHERE id = 'note-shared'",
            quoted(created_at)
        ),
    )
    .await;
    let create = sealed_create(creator, NOTES_TABLE, HELD_NOTE).await;
    let create = with_group(create, note_group::CREATED, vec![text_field(created_at)]);
    forge_change(writer, create).await
}

/// Stores `operations` on `replica` and applies them.
async fn take(replica: &mut Replica, operations: &[Vec<u8>]) {
    replica.receive(operations).await;
    replica.apply_all().await.unwrap();
}

/// The creation times of the versions `replica` holds for [`HELD_NOTE`], sorted. They come from
/// the merge state, so they include versions of a hidden row and versions a visible row elides
/// in favor of its domain row.
async fn held_versions(replica: &mut Replica) -> Vec<String> {
    type StoredVersion = (Vec<u8>, i64, Vec<u8>, Option<Vec<u8>>);
    let versions: Vec<StoredVersion> = sqlx::query_as(
        "SELECT writer_id, seq, value_hash, value FROM sync_register_versions
         WHERE table_id = ? AND row_key = ? AND group_id = ?",
    )
    .bind(i64::from(NOTES_TABLE.0))
    .bind(HELD_NOTE)
    .bind(i64::from(note_group::CREATED.get()))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    let table = replica.engine.manifest().table(NOTES_TABLE).unwrap();
    let mut times = Vec::new();
    for (writer, seq, hash, value) in versions {
        let value = match value {
            Some(value) => Value::decode(&value).unwrap(),
            None => hydrate::elided_value(
                &replica.engine,
                &mut replica.conn,
                table,
                HELD_NOTE,
                note_group::CREATED,
                log::writer_id(&writer).unwrap(),
                u64::try_from(seq).unwrap(),
                &log::digest(&hash).unwrap(),
            )
            .await
            .unwrap(),
        };
        match value.fields() {
            [Field::Text(text)] => times.push(text.clone()),
            fields => panic!("a creation value holds one text field, not {fields:?}"),
        }
    }
    times.sort();
    times
}

/// Kept rows that hold back a Create of [`HELD_NOTE`].
async fn held_rows(replica: &mut Replica) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_displaced_versions WHERE row_key = {} AND group_id = {}",
            quoted(HELD_NOTE),
            note_group::CREATED.get()
        ))
        .await
}

/// Asserts that both delivery orders hold the creation times `expected`, in sorted order, and
/// end in the same state.
async fn assert_same_end(orders: &mut Orders, expected: &[&str]) {
    let Orders { before, after, .. } = orders;
    assert_eq!(held_versions(before).await, expected, "the first order");
    assert_eq!(held_versions(after).await, expected, "the second order");
    assert_eq!(before.dump().await, after.dump().await);
}

/// A Create that saw a version the removal excludes ends the same whether it arrived before or
/// after the removal, and its creation time stands.
#[test]
fn covering_create_and_exclusion_converge_in_either_order() {
    block_on(async {
        let mut orders = Orders::new(215).await;
        let Orders {
            anchor,
            first,
            second,
            before,
            after,
            ..
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "Late", 1)).await;
        deliver(second, first).await;
        let covering = create_again(first, second, EDITED_AT).await;

        deliver(before, second).await;
        take(before, std::slice::from_ref(&covering)).await;
        assert_eq!(created_versions(before, HELD_NOTE).await, 1);
        let device = first.device_id.clone();
        Orders::remove(anchor, before, &device).await;

        deliver(after, anchor).await;
        deliver(after, second).await;
        take(after, std::slice::from_ref(&covering)).await;

        assert_same_end(&mut orders, &[EDITED_AT]).await;
    });
}

/// The devices `replica` names as creators of [`HELD_NOTE`].
async fn held_note_creators(replica: &mut Replica) -> Vec<String> {
    let manifest = replica.engine.manifest();
    row_creating_devices(&mut replica.conn, manifest, NOTES_TABLE, HELD_NOTE)
        .await
        .unwrap()
}

/// A held Create names its device as a creator once an exclusion lets its value in, and not
/// before.
#[test]
fn held_create_names_its_device_once_its_value_enters() {
    block_on(async {
        let mut orders = Orders::new(224).await;
        let Orders {
            anchor,
            first,
            second,
            before,
            ..
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "Late", 1)).await;
        deliver(second, first).await;
        let covering = create_again(first, second, EDITED_AT).await;

        deliver(before, second).await;
        take(before, std::slice::from_ref(&covering)).await;
        assert_eq!(
            held_note_creators(before).await,
            std::slice::from_ref(&first.device_id)
        );

        let device = first.device_id.clone();
        Orders::remove(anchor, before, &device).await;
        assert_eq!(held_versions(before).await, [EDITED_AT]);
        assert_eq!(
            held_note_creators(before).await,
            std::slice::from_ref(&second.device_id)
        );
    });
}

/// A held Create stays kept once the anchor confirms the version it saw: after a handover or a
/// fork a cutoff can still fall below that mark, and a re-seal never carries an Immutable group,
/// so only the kept Create can fill the group again.
#[test]
fn a_held_create_outlasts_the_anchor_mark_of_the_version_it_saw() {
    block_on(async {
        let mut orders = Orders::new(219).await;
        let Orders {
            anchor,
            first,
            second,
            before,
            after,
            ..
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "Late", 1)).await;
        deliver(second, first).await;
        let covering = create_again(first, second, EDITED_AT).await;
        deliver(before, second).await;
        take(before, std::slice::from_ref(&covering)).await;
        assert_eq!(held_rows(before).await, 1);

        // A report that lists the first writer's Create, as one from an anchor installation
        // before a handover can, raises its mark above the cutoff the removal then sets.
        let confirmed = before.stored_vector().await;
        let anchor_device = anchor.device_id.clone();
        assert!(
            before
                .engine
                .record_anchor_marks(&mut before.conn, &before.ctx, &anchor_device, &confirmed)
                .await
                .unwrap()
        );
        assert_eq!(held_rows(before).await, 1);

        let device = first.device_id.clone();
        Orders::remove(anchor, before, &device).await;
        deliver(after, anchor).await;
        deliver(after, second).await;
        take(after, std::slice::from_ref(&covering)).await;
        assert_same_end(&mut orders, &[EDITED_AT]).await;
    });
}

/// A Create stays out while another version it saw remains, and comes in once none does.
#[test]
fn excluding_one_of_two_covered_versions_keeps_the_other() {
    block_on(async {
        let mut orders = Orders::new(216).await;
        let Orders {
            anchor,
            first,
            second,
            third,
            before,
            after,
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "One", 1)).await;
        edit(second, &note(HELD_NOTE, "Two", 1)).await;
        let kept = second
            .scalar_text("SELECT created_at FROM quick_notes WHERE id = 'note-held'")
            .await
            .unwrap();
        deliver(third, first).await;
        deliver(third, second).await;
        let covering = create_again(first, third, EDITED_AT).await;

        deliver(before, third).await;
        take(before, std::slice::from_ref(&covering)).await;
        let (removed, removed_too) = (first.device_id.clone(), second.device_id.clone());
        Orders::remove(anchor, before, &removed).await;

        deliver(after, anchor).await;
        deliver(after, third).await;
        take(after, std::slice::from_ref(&covering)).await;

        assert_same_end(&mut orders, &[kept.as_str()]).await;

        let Orders {
            anchor,
            before,
            after,
            ..
        } = &mut orders;
        Orders::remove(anchor, before, &removed_too).await;
        deliver(after, anchor).await;
        assert_same_end(&mut orders, &[EDITED_AT]).await;
    });
}

/// A version the Create did not see stays beside it, as it would had the Create arrived after
/// the removal, and the two compete for the Immutable minimum.
#[test]
fn held_create_competes_with_a_version_it_did_not_see() {
    block_on(async {
        let mut orders = Orders::new(217).await;
        let Orders {
            anchor,
            first,
            second,
            third,
            before,
            after,
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "One", 1)).await;
        edit(second, &note(HELD_NOTE, "Two", 1)).await;
        let unseen = second
            .scalar_text("SELECT created_at FROM quick_notes WHERE id = 'note-held'")
            .await
            .unwrap();
        deliver(third, first).await;
        let covering = create_again(first, third, EDITED_AT).await;

        deliver(before, third).await;
        deliver(before, second).await;
        take(before, std::slice::from_ref(&covering)).await;
        let device = first.device_id.clone();
        Orders::remove(anchor, before, &device).await;

        deliver(after, anchor).await;
        deliver(after, third).await;
        deliver(after, second).await;
        take(after, std::slice::from_ref(&covering)).await;

        assert_same_end(&mut orders, &[unseen.as_str(), EDITED_AT]).await;
    });
}

/// Of two Creates where the later saw the earlier, the earlier comes in, and the later stays
/// recoverable from it through a second removal.
#[test]
fn chained_creates_restore_in_causal_order() {
    block_on(async {
        let mut orders = Orders::new(218).await;
        let Orders {
            anchor,
            first,
            second,
            third,
            before,
            after,
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "Late", 1)).await;
        deliver(second, first).await;
        let earlier = create_again(first, second, EDITED_AT).await;
        deliver(third, first).await;
        deliver(third, second).await;
        take(third, std::slice::from_ref(&earlier)).await;
        let later = create_again(first, third, SECOND_CREATED_AT).await;
        let chain = [earlier, later];

        deliver(before, second).await;
        deliver(before, third).await;
        take(before, &chain).await;
        let (removed, removed_too) = (first.device_id.clone(), second.device_id.clone());
        Orders::remove(anchor, before, &removed).await;

        deliver(after, anchor).await;
        deliver(after, second).await;
        deliver(after, third).await;
        take(after, &chain).await;

        assert_same_end(&mut orders, &[EDITED_AT]).await;

        let Orders {
            anchor,
            before,
            after,
            ..
        } = &mut orders;
        Orders::remove(anchor, before, &removed_too).await;
        deliver(after, anchor).await;
        assert_same_end(&mut orders, &[SECOND_CREATED_AT]).await;
    });
}

/// Two removals settled in one batch leave the Creates that saw only their versions as removals
/// settled one at a time do, whichever of the removed writers sorts first: of a Create that saw
/// one removed version and a Create that saw it, both removed versions, and the first Create,
/// only the first comes in.
#[test]
fn removals_settled_together_restore_held_creates_in_any_writer_order() {
    block_on(async {
        let mut orders = Orders::new(227).await;
        let mut witness = Replica::new(6, 227).await;
        deliver(&mut witness, &mut orders.anchor).await;
        let Orders {
            anchor,
            first,
            second,
            third,
            before,
            after,
        } = &mut orders;
        // A removed writer restores in writer order, so the writer of the version both Creates
        // saw sorts last, the order that brought both Creates in.
        let (seen, other) = if first.writer() > second.writer() {
            (first, second)
        } else {
            (second, first)
        };
        edit(seen, &note(HELD_NOTE, "Seen", 1)).await;
        edit(other, &note(HELD_NOTE, "Other", 1)).await;
        deliver(third, seen).await;
        let earlier = create_again(seen, third, EDITED_AT).await;
        deliver(&mut witness, seen).await;
        deliver(&mut witness, other).await;
        deliver(&mut witness, third).await;
        take(&mut witness, std::slice::from_ref(&earlier)).await;
        let later = create_again(seen, &mut witness, SECOND_CREATED_AT).await;
        let chain = [earlier, later];

        deliver(before, &mut witness).await;
        take(before, &chain).await;
        let (removed, removed_too) = (seen.device_id.clone(), other.device_id.clone());
        anchor.remove(&removed).await.unwrap();
        anchor.remove(&removed_too).await.unwrap();
        deliver(before, anchor).await;

        deliver(after, anchor).await;
        deliver(after, &mut witness).await;
        take(after, &chain).await;
        assert_same_end(&mut orders, &[EDITED_AT]).await;
    });
}

/// A Create that saw several versions keeps one held row, not one per version it saw, so held
/// rows stay bounded by the writers of a group.
#[test]
fn a_create_that_saw_several_versions_keeps_one_held_row() {
    block_on(async {
        let mut orders = Orders::new(228).await;
        let Orders {
            first,
            second,
            third,
            before,
            ..
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "One", 1)).await;
        edit(second, &note(HELD_NOTE, "Two", 1)).await;
        deliver(third, first).await;
        deliver(third, second).await;
        let covering = create_again(first, third, EDITED_AT).await;
        take(third, std::slice::from_ref(&covering)).await;
        let again = create_again(first, third, SECOND_CREATED_AT).await;

        deliver(before, third).await;
        take(before, &[covering, again]).await;
        assert_eq!(created_versions(before, HELD_NOTE).await, 2);
        assert_eq!(held_rows(before).await, 1);
    });
}

/// The displaced count in sync status leaves out held Creates: they are merge state that no
/// anchor mark releases, not versions waiting for the anchor.
#[test]
fn status_counts_displaced_versions_without_held_creates() {
    block_on(async {
        let mut orders = Orders::new(229).await;
        let Orders {
            first,
            second,
            before,
            ..
        } = &mut orders;
        edit(first, &note(HELD_NOTE, "Late", 1)).await;
        deliver(second, first).await;
        let covering = create_again(first, second, EDITED_AT).await;
        deliver(before, second).await;
        take(before, std::slice::from_ref(&covering)).await;
        assert_eq!(held_rows(before).await, 1);

        let waiting = before
            .count(&format!(
                "SELECT count(*) FROM sync_displaced_versions
                 WHERE NOT (row_key = {} AND group_id = {})",
                quoted(HELD_NOTE),
                note_group::CREATED.get()
            ))
            .await;
        let status = before
            .engine
            .status(&mut before.conn, &before.ctx)
            .await
            .unwrap();
        assert_eq!(i64::try_from(status.displaced).unwrap(), waiting);
    });
}

/// How far ahead of the others the clock of the fast device runs, more than a remote clock is
/// ever adopted ahead.
const FAST_CLOCK_MS: u64 = 2 * MAX_ADOPTION_AHEAD_MS;

/// The creation time `replica` shows for [`HELD_NOTE`].
async fn note_created_at(replica: &mut Replica) -> Option<String> {
    replica
        .scalar_text("SELECT created_at FROM quick_notes WHERE id = 'note-held'")
        .await
}

/// The clock `bytes` was sealed at.
fn sealed_clock(bytes: &[u8]) -> Hlc {
    Envelope::decode(bytes)
        .unwrap()
        .operation()
        .unwrap()
        .header
        .clock
}

/// A Create and a title edit that saw a Create of a device with a fast clock carry the lower
/// clock, and still settle over it the same way on a replica that applied them in causal order
/// and on one that applied them first and the fast Create again once a reselected chain lifted
/// its exclusion: the earlier Create stays the version and the later edit the title.
#[test]
fn a_lifted_exclusion_settles_versions_that_saw_a_fast_clock_like_causal_order() {
    block_on(async {
        let mut space = superseded(230).await;
        let fast = &mut space.member;
        fast.now_ms += FAST_CLOCK_MS;
        edit(fast, &note(HELD_NOTE, "Fast", 1)).await;
        let created_at = note_created_at(fast).await.unwrap();
        let fast_create = fast.own_ops().await.pop().unwrap();

        let mut honest = Replica::new(2, 230).await;
        honest.pull(&mut space.hub).await;
        honest.pull(&mut space.member).await;
        edit(&mut honest, &note("note-shared", "Draft", 0)).await;
        let covering = create_again(&mut space.member, &mut honest, EDITED_AT).await;
        take(&mut honest, std::slice::from_ref(&covering)).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Seen' WHERE id = 'note-held'",
        )
        .await;
        let seen = honest.own_ops().await;
        assert!(
            seen.iter()
                .all(|bytes| sealed_clock(bytes) < sealed_clock(&fast_create))
        );

        space
            .hub
            .sequence(remove_entry(&space.member.device_id))
            .await
            .unwrap()
            .unwrap();
        let fast_ops = space.member.own_ops().await;
        let chain = space.contender.own_ops().await;

        let mut causal = Replica::new(3, 230).await;
        causal.pull(&mut space.hub).await;
        take(&mut causal, &chain).await;
        take(&mut causal, &fast_ops).await;
        take(&mut causal, &seen).await;

        let mut lifted = Replica::new(3, 230).await;
        lifted.pull(&mut space.hub).await;
        take(&mut lifted, &fast_ops).await;
        take(&mut lifted, &seen).await;
        assert_eq!(created_versions(&mut lifted, HELD_NOTE).await, 1);
        take(&mut lifted, &chain).await;

        for (order, replica) in [("causal", &mut causal), ("lifted", &mut lifted)] {
            assert_eq!(created_versions(replica, HELD_NOTE).await, 1, "{order}");
            assert_eq!(
                note_created_at(replica).await,
                Some(created_at.clone()),
                "{order}"
            );
            assert_eq!(
                title(replica, HELD_NOTE).await.as_deref(),
                Some("Seen"),
                "{order}"
            );
        }
        assert_eq!(causal.dump().await, lifted.dump().await);
    });
}

#[test]
fn context_past_a_surplus_genesis_becomes_ready() {
    block_on(async {
        let (mut anchor, mut removed, mut honest) = trio(232).await;
        let mut peer = Replica::new(3, 232).await;
        // The anchor never stores the device's writers, so its removal names none of them.
        let mut writers = Vec::new();
        for (position, id) in ["note-first", "note-second", "note-third"]
            .into_iter()
            .enumerate()
        {
            if position > 0 {
                removed.rotate();
            }
            edit(&mut removed, &note(id, "Late", 0)).await;
            edit(
                &mut removed,
                &format!("UPDATE quick_notes SET title = 'Later' WHERE id = '{id}'"),
            )
            .await;
            writers.push((removed.writer(), removed.own_ops().await));
        }
        writers.sort_by_key(|(writer, _)| *writer);
        let (kept, rest) = writers.split_at(2);
        let (surplus, surplus_ops) = &rest[0];

        // The honest device sees the surplus writer before the removal and edits past it.
        take(&mut honest, surplus_ops).await;
        edit(&mut honest, &note("note-honest", "Honest", 1)).await;
        let dependent = honest.own_ops().await;
        anchor.remove(&removed.device_id).await.unwrap();

        // The peer learns of the removal and the lower writers first, then the honest change.
        deliver(&mut peer, &mut anchor).await;
        for (_, ops) in kept {
            take(&mut peer, ops).await;
        }
        take(&mut peer, surplus_ops).await;
        take(&mut peer, &dependent).await;
        assert_eq!(
            peer.applied_vector().await.get(&honest.writer()),
            honest.stored_vector().await.get(&honest.writer()),
            "the honest change leaves the hold"
        );
        assert_eq!(
            peer.stored_vector().await.get(surplus),
            1,
            "the surplus genesis is stored"
        );
        assert_eq!(
            title(&mut peer, "note-honest").await.as_deref(),
            Some("Honest")
        );

        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut peer).await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut peer, &mut anchor).await;
        let expected = anchor.domain_dump().await;
        for replica in [&mut anchor, &mut honest, &mut peer] {
            assert_eq!(replica.domain_dump().await, expected);
            assert_eq!(replica.stored_vector().await.get(surplus), 1);
            assert_eq!(
                title(replica, "note-honest").await.as_deref(),
                Some("Honest")
            );
            replica.check_invariants().await;
        }
    });
}

/// Mints `count` writers of `removed`, each creating its own note, ordered by writer id with
/// their operations.
async fn removed_writers(removed: &mut Replica, count: usize) -> Vec<(WriterId, Vec<Vec<u8>>)> {
    let mut writers = Vec::new();
    for position in 0..count {
        if position > 0 {
            removed.rotate();
        }
        edit(removed, &note(&format!("note-{position}"), "Late", 0)).await;
        writers.push((removed.writer(), removed.own_ops().await));
    }
    writers.sort_by_key(|(writer, _)| *writer);
    writers
}

/// Writers of `writers` that `replica` stores.
async fn stored_writers(
    replica: &mut Replica,
    writers: &[(WriterId, Vec<Vec<u8>>)],
) -> BTreeSet<WriterId> {
    let stored = replica.stored_vector().await;
    writers
        .iter()
        .map(|(writer, _)| *writer)
        .filter(|writer| stored.get(writer) > 0)
        .collect()
}

/// A removed device whose removal names none of its writers mints one writer more than a
/// replica stores for it, and an honest device edits past the writer at `covered` in id order
/// before the removal. Two peers that learn of the removal first receive the writers in
/// ascending and in descending id order. Both keep the lowest writers, and the honest change
/// applies on both.
async fn covered_writer_at_the_device_limit(salt: u64, covered: usize) {
    let (mut anchor, mut removed, mut honest) = trio(salt).await;
    let mut ascending = Replica::new(3, salt).await;
    let mut descending = Replica::new(4, salt).await;
    let writers = removed_writers(&mut removed, MAX_WRITERS_PER_DEVICE + 1).await;
    let (covered, covered_ops) = writers[covered].clone();
    let (highest, _) = writers[MAX_WRITERS_PER_DEVICE].clone();

    take(&mut honest, &covered_ops).await;
    edit(&mut honest, &note("note-honest", "Honest", 1)).await;
    let dependent = honest.own_ops().await;
    anchor.remove(&removed.device_id).await.unwrap();

    deliver(&mut ascending, &mut anchor).await;
    deliver(&mut descending, &mut anchor).await;
    for (_, ops) in &writers {
        take(&mut ascending, ops).await;
    }
    for (_, ops) in writers.iter().rev() {
        take(&mut descending, ops).await;
    }
    for replica in [&mut ascending, &mut descending] {
        take(replica, &dependent).await;
    }

    let kept: BTreeSet<_> = writers[..MAX_WRITERS_PER_DEVICE]
        .iter()
        .map(|(writer, _)| *writer)
        .collect();
    for replica in [&mut ascending, &mut descending] {
        assert_eq!(
            stored_writers(replica, &writers).await,
            kept,
            "every replica keeps the lowest writers"
        );
        let stored = replica.stored_vector().await;
        assert_eq!(stored.get(&highest), 0, "the highest writer is not stored");
        if covered != highest {
            assert_eq!(
                stored.get(&covered),
                covered_ops.len() as u64,
                "a kept writer keeps its excluded operations"
            );
        }
        assert_eq!(
            replica.applied_vector().await.get(&honest.writer()),
            honest.stored_vector().await.get(&honest.writer()),
            "the honest change leaves the hold"
        );
        assert_eq!(
            title(replica, "note-honest").await.as_deref(),
            Some("Honest")
        );
        replica.check_invariants().await;
    }
    assert_eq!(
        late_changes(&mut ascending).await,
        late_changes(&mut descending).await
    );
    assert_eq!(
        ascending.domain_dump().await,
        descending.domain_dump().await
    );
}

#[test]
fn a_lower_genesis_displaces_the_highest_surplus_genesis_at_the_device_writer_limit() {
    block_on(covered_writer_at_the_device_limit(233, 0));
}

#[test]
fn a_context_covering_a_surplus_writer_past_the_device_writer_limit_becomes_ready() {
    block_on(covered_writer_at_the_device_limit(
        234,
        MAX_WRITERS_PER_DEVICE,
    ));
}

#[test]
fn a_lower_unnamed_genesis_displaces_an_unnamed_writer_when_the_removal_names_the_rest() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 235).await;
        let mut removed = Replica::new(1, 235).await;
        let mut late_lowest = Replica::new(3, 235).await;
        let mut late_highest = Replica::new(4, 235).await;
        let writers = removed_writers(&mut removed, MAX_WRITERS_PER_DEVICE + 1).await;
        // The removal names every writer but the three lowest, which stay unnamed.
        let (unnamed, named) = writers.split_at(3);
        for (_, ops) in named {
            take(&mut anchor, ops).await;
        }
        anchor.remove(&removed.device_id).await.unwrap();

        // One peer fills the device's writer limit before the lowest unnamed writer arrives,
        // the other before the highest one does.
        deliver(&mut late_lowest, &mut anchor).await;
        deliver(&mut late_highest, &mut anchor).await;
        for (_, ops) in named.iter().chain(unnamed[1..].iter()) {
            take(&mut late_lowest, ops).await;
        }
        take(&mut late_lowest, &unnamed[0].1).await;
        for (_, ops) in unnamed[..2].iter().chain(named.iter()) {
            take(&mut late_highest, ops).await;
        }
        take(&mut late_highest, &unnamed[2].1).await;

        let kept: BTreeSet<_> = writers
            .iter()
            .map(|(writer, _)| *writer)
            .filter(|writer| *writer != unnamed[2].0)
            .collect();
        for replica in [&mut late_lowest, &mut late_highest] {
            assert_eq!(
                stored_writers(replica, &writers).await,
                kept,
                "every replica keeps the named writers and the lowest unnamed ones"
            );
            replica.check_invariants().await;
        }
        assert_eq!(
            late_changes(&mut late_lowest).await,
            late_changes(&mut late_highest).await
        );
        assert_eq!(
            late_lowest.domain_dump().await,
            late_highest.domain_dump().await
        );
    });
}

/// The anchor, a removed device that sealed `note-early` before its removal, an honest member
/// that created `note-kept`, a peer that stores what the removed device seals past its cutoff
/// before it learns of the removal, and a peer that learns of the removal first, with the
/// removed writer's cutoff. Every replica knows both notes.
async fn removed_before_a_burst(salt: u64) -> ([Replica; 5], u64) {
    let (mut anchor, mut removed, mut honest) = trio(salt).await;
    let mut early = Replica::new(3, salt).await;
    let mut never = Replica::new(4, salt).await;
    edit(&mut honest, &note("note-kept", "Draft", 0)).await;
    spread(&mut anchor, &mut honest).await;
    deliver(&mut removed, &mut anchor).await;
    edit(&mut removed, &note("note-early", "Early", 1)).await;
    deliver(&mut anchor, &mut removed).await;
    for replica in [&mut honest, &mut early, &mut never] {
        deliver(replica, &mut anchor).await;
    }
    let cutoff = anchor.stored_vector().await.get(&removed.writer());
    anchor.remove(&removed.device_id.clone()).await.unwrap();
    deliver(&mut honest, &mut anchor).await;
    deliver(&mut never, &mut anchor).await;
    ([anchor, removed, honest, early, never], cutoff)
}

/// Seals [`MAX_EXCLUDED_PER_WRITER`] edits of `note-early` on `removed`, which fill its
/// excluded bound past `cutoff`.
async fn fill_excluded_bound(removed: &mut Replica, cutoff: u64) {
    for index in 0..MAX_EXCLUDED_PER_WRITER {
        edit(
            removed,
            &format!("UPDATE quick_notes SET title = 'Early {index}' WHERE id = 'note-early'"),
        )
        .await;
    }
    assert_eq!(
        removed.stored_vector().await.get(&removed.writer()),
        cutoff + MAX_EXCLUDED_PER_WRITER
    );
}

/// The removed writer's operations past `cutoff`.
async fn past_cutoff(removed: &mut Replica, cutoff: u64) -> Vec<Vec<u8>> {
    removed.own_ops().await[usize::try_from(cutoff).unwrap()..].to_vec()
}

/// The replicated merge state a replica holds for a note: its row, versions, tombstones, and
/// creations. Displaced versions are left out, since the anchor releases them where a member
/// keeps them until the anchor confirms.
async fn note_merge_state(replica: &mut Replica, id: &str) -> Vec<String> {
    let key = quoted(id);
    let table = NOTES_TABLE.0;
    let mut state = Vec::new();
    for (name, sql) in [
        (
            "row",
            format!(
                "SELECT state || ':' || creation_pending FROM sync_rows
                 WHERE table_id = {table} AND row_key = {key}"
            ),
        ),
        (
            "version",
            format!(
                "SELECT group_id || ':' || hex(writer_id) || ':' || seq || ':' || hex(value_hash)
                 FROM sync_register_versions WHERE table_id = {table} AND row_key = {key}
                 ORDER BY 1"
            ),
        ),
        (
            "tombstone",
            format!(
                "SELECT hex(writer_id) || ':' || seq FROM sync_tombstones
                 WHERE table_id = {table} AND row_key = {key} ORDER BY 1"
            ),
        ),
        (
            "creation",
            format!(
                "SELECT hex(writer_id) || ':' || seq FROM sync_row_creations
                 WHERE table_id = {table} AND row_key = {key} ORDER BY 1"
            ),
        ),
    ] {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut replica.conn)
            .await
            .unwrap();
        state.extend(rows.into_iter().map(|row| format!("{name} {row}")));
    }
    state
}

/// A removed device fills its excluded bound and then signs a Create of a row an honest member
/// created. A peer that stored and applied all of it before learning of the removal keeps the
/// honest row with its merge state, applies a later honest edit, and ends where a peer that
/// never stored the operations ends.
#[test]
fn dropping_past_the_bound_keeps_an_honest_row() {
    block_on(async {
        let ([mut anchor, mut removed, mut honest, mut early, mut never], cutoff) =
            removed_before_a_burst(270).await;
        fill_excluded_bound(&mut removed, cutoff).await;
        let forged = late_create(&mut removed, &mut honest, "note-kept").await;
        let mut past = past_cutoff(&mut removed, cutoff).await;
        past.push(forged);
        assert_eq!(past.len() as u64, MAX_EXCLUDED_PER_WRITER + 1);

        take(&mut early, &past).await;
        deliver(&mut early, &mut anchor).await;
        assert_eq!(
            early.stored_vector().await.get(&removed.writer()),
            cutoff + MAX_EXCLUDED_PER_WRITER,
            "the operation past the bound is dropped"
        );
        assert_eq!(
            title(&mut early, "note-kept").await.as_deref(),
            Some("Draft"),
            "the honest row stays"
        );
        assert_eq!(
            note_merge_state(&mut early, "note-kept").await,
            note_merge_state(&mut never, "note-kept").await,
            "the honest row keeps its merge state"
        );

        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-kept'",
        )
        .await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut early, &mut anchor).await;
        deliver(&mut never, &mut anchor).await;
        let expected = never.domain_dump().await;
        let merged = note_merge_state(&mut never, "note-kept").await;
        for replica in [&mut anchor, &mut early, &mut never] {
            assert_eq!(title(replica, "note-kept").await.as_deref(), Some("Edited"));
            assert_eq!(replica.domain_dump().await, expected);
            assert_eq!(note_merge_state(replica, "note-kept").await, merged);
            replica.check_invariants().await;
        }
    });
}

/// A row only an operation past the bound created leaves the merge state and the domain table
/// on a peer that applied it before learning of the removal, as on a peer that never stored it.
#[test]
fn dropping_past_the_bound_purges_a_row_only_it_created() {
    block_on(async {
        let ([mut anchor, mut removed, _, mut early, mut never], cutoff) =
            removed_before_a_burst(271).await;
        fill_excluded_bound(&mut removed, cutoff).await;
        edit(&mut removed, &note("note-only", "Only", 2)).await;
        let past = past_cutoff(&mut removed, cutoff).await;
        assert_eq!(past.len() as u64, MAX_EXCLUDED_PER_WRITER + 1);

        take(&mut early, &past).await;
        assert_eq!(
            title(&mut early, "note-only").await.as_deref(),
            Some("Only")
        );
        deliver(&mut early, &mut anchor).await;
        let expected = never.domain_dump().await;
        for replica in [&mut anchor, &mut early, &mut never] {
            assert_eq!(title(replica, "note-only").await, None);
            assert!(
                note_merge_state(replica, "note-only").await.is_empty(),
                "replica {} keeps merge state of the purged row",
                replica.index
            );
            assert_eq!(replica.domain_dump().await, expected);
            replica.check_invariants().await;
        }
    });
}

/// The creation of a row past the bound is found when compaction dropped its body before the
/// removal arrived, so the row leaves as it does with the body kept.
#[test]
fn dropping_past_the_bound_finds_creations_of_compacted_bodies() {
    block_on(async {
        let ([mut anchor, mut removed, _, mut early, mut never], cutoff) =
            removed_before_a_burst(272).await;
        fill_excluded_bound(&mut removed, cutoff).await;
        edit(&mut removed, &note("note-only", "Only", 2)).await;
        edit(
            &mut removed,
            "UPDATE quick_notes SET title = 'Early last' WHERE id = 'note-early'",
        )
        .await;
        let past = past_cutoff(&mut removed, cutoff).await;
        assert_eq!(past.len() as u64, MAX_EXCLUDED_PER_WRITER + 2);

        take(&mut early, &past).await;
        let limit = early.stored_vector().await;
        early
            .compact(&limit, crate::COMPACTION_BATCH_OPS)
            .await
            .unwrap();
        let bodiless = early
            .count(&format!(
                "SELECT count(*) FROM sync_ops WHERE writer_id = x'{}' AND seq = {}
                    AND length(envelope) = 0",
                removed.writer().to_hex(),
                cutoff + MAX_EXCLUDED_PER_WRITER + 1
            ))
            .await;
        assert_eq!(bodiless, 1, "the creation past the bound is compacted");

        deliver(&mut early, &mut anchor).await;
        let expected = never.domain_dump().await;
        for replica in [&mut anchor, &mut early, &mut never] {
            assert_eq!(title(replica, "note-only").await, None);
            assert!(
                note_merge_state(replica, "note-only").await.is_empty(),
                "replica {} keeps merge state of the purged row",
                replica.index
            );
            assert_eq!(replica.domain_dump().await, expected);
            replica.check_invariants().await;
        }
    });
}
