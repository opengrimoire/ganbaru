//! Removal exclusion: operations a removed device sealed past its cutoff lose their effects
//! everywhere, the versions they superseded come back, honest operations that saw them keep
//! theirs, anchor marks release retained versions, and the late changes they leave are derived
//! the same way on every replica and decided once.

use ganbaru_sync_contracts::{Entry, LateResolution, LateRow, ResolveLate, RowKey};

use super::authorization::admin_entry;
use super::block_on;
use super::replica::{Replica, device_admin, quoted, rank};
use crate::acknowledge::ACKNOWLEDGEMENT_INTERVAL_MS;
use crate::domains::themes::PALETTE_SIZE;
use crate::manifest::vault::music::PLAYLISTS_TABLE;
use crate::manifest::vault::quick_notes::NOTES_TABLE;
use crate::{HoldReason, LateChange, LateKind, StoreOutcome};

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

pub(super) async fn displaced(replica: &mut Replica) -> i64 {
    replica
        .count("SELECT count(*) FROM sync_displaced_versions")
        .await
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
async fn restore_all(anchor: &mut Replica, removal: u64) {
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
