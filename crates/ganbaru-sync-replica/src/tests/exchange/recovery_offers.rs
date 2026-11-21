//! Recovery offers across replicas: a resolved conflict closes everywhere, concurrent theme
//! saves surface one conflict, and restoring or discarding an offer settles on every replica.

use super::{
    HUB_DEVICE, PHONE_DEVICE, Replica, converge, edit_sql, hub, music_key, music_texts,
    recovery_titles, restorers,
};
use crate::recovery::{
    RecoveredRow, RecoveryAction, RecoveryChoice, RecoveryRestorers, RestoreFuture,
};
use crate::tests::{NOW_MS, VAULT_ID, block_on};
use ganbaru_sync::manifest::vault::quick_notes::NOTES_TABLE;
use ganbaru_sync::{Engine, SpaceContext};
use ganbaru_sync_contracts::SpaceId;
use sqlx::SqliteConnection;
use std::collections::HashMap;

async fn note_json(replica: &Replica, id: &str) -> serde_json::Value {
    let note = ganbaru_quick_notes::load_note_from_pool(&replica.pool, id)
        .await
        .expect("load note");
    serde_json::to_value(note).expect("serialize note")
}

#[test]
fn a_resolved_title_conflict_closes_on_both_replicas() {
    block_on(async {
        let mut hub_replica = Replica::new("conflict-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("conflict-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-1").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit(
                "UPDATE quick_notes SET title = 'Hub title' WHERE id = ?",
                "note-1",
            )
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Phone title' WHERE id = ?",
                "note-1",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let hub_note = note_json(&hub_replica, "note-1").await;
        let phone_note = note_json(&phone, "note-1").await;
        assert_eq!(hub_note["hasConflict"], true);
        assert_eq!(phone_note["hasConflict"], true);
        assert_eq!(hub_note["title"], phone_note["title"]);

        let conflict = ganbaru_quick_notes::conflict_from_pool(
            &phone.pool,
            VAULT_ID,
            &ganbaru_sync::DeviceNames::new(PHONE_DEVICE, HashMap::new()),
            "note-1",
        )
        .await
        .expect("read conflict");
        let conflict = serde_json::to_value(conflict).expect("serialize conflict");
        let groups = conflict["groups"].as_array().expect("groups");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["field"], "title");
        let versions = groups[0]["versions"].as_array().expect("versions");
        assert_eq!(versions.len(), 2);
        let displayed: Vec<_> = versions
            .iter()
            .filter(|version| version["displayed"] == true)
            .collect();
        assert_eq!(displayed.len(), 1);
        assert_eq!(displayed[0]["title"], phone_note["title"]);
        let own = versions
            .iter()
            .find(|version| version["device"]["ownDevice"] == true)
            .expect("own version");
        assert_eq!(own["title"], "Phone title");
        let other = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("hidden version");

        let resolution = serde_json::from_value(serde_json::json!({
            "id": "note-1",
            "field": "title",
            "choice": { "kind": "version", "version": other["version"] },
        }))
        .expect("resolution");
        let resolved =
            ganbaru_quick_notes::resolve_from_pool(&phone.pool, VAULT_ID, resolution, NOW_MS + 1)
                .await
                .expect("resolve conflict");
        let resolved = serde_json::to_value(resolved).expect("serialize resolution");
        assert_eq!(resolved["note"]["title"], other["title"]);
        assert_eq!(resolved["note"]["hasConflict"], false);
        assert_eq!(resolved["copy"], serde_json::Value::Null);

        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let note = note_json(replica, "note-1").await;
            assert_eq!(note["title"], other["title"]);
            assert_eq!(note["hasConflict"], false);
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

/// A user theme write whose live content uses `blend_canvas` and whose seed never varies.
fn theme_write(id: &str, blend_canvas: &str) -> ganbaru_themes::UserThemeWrite {
    let palette: Vec<_> = (0..32)
        .map(|slot| serde_json::json!({ "slot": slot, "value": "#aaaaaa" }))
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": id,
        "displayName": "Forest",
        "iconLabel": "dark",
        "seedIconLabel": "dark",
        "blendCanvas": blend_canvas,
        "seedBlendCanvas": "#101010",
        "derivationEngineVersion": 2,
        "calendarDefaultMode": "app-canvas",
        "calendarDefaultCustom": "#202020",
        "seedCalendarDefaultMode": "app-canvas",
        "seedCalendarDefaultCustom": "#202020",
        "tokens": [{ "kind": "source", "key": "canvas", "value": blend_canvas, "isolated": false }],
        "palette": palette,
        "seedTokens": [{ "kind": "source", "key": "canvas", "value": "#101010", "isolated": false }],
        "seedPalette": palette,
    }))
    .expect("theme write")
}

async fn theme_json(replica: &Replica, id: &str) -> serde_json::Value {
    let theme = ganbaru_themes::load_one(&replica.pool, id)
        .await
        .expect("load theme");
    serde_json::to_value(theme).expect("serialize theme")
}

#[test]
fn concurrent_theme_saves_surface_one_conflict_that_closes_on_both_replicas() {
    block_on(async {
        let mut hub_replica = Replica::new("theme-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("theme-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        ganbaru_themes::insert(&hub_replica.pool, &theme_write("theme-1", "#111111"))
            .await
            .expect("insert theme");
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        ganbaru_themes::replace_content(&hub_replica.pool, &theme_write("theme-1", "#222222"))
            .await
            .expect("hub save");
        ganbaru_themes::record_dismissal(&hub_replica.pool, "theme-1", 5)
            .await
            .expect("hub dismissal");
        hub_replica.seal().await;
        ganbaru_themes::replace_content(&phone.pool, &theme_write("theme-1", "#333333"))
            .await
            .expect("phone save");
        ganbaru_themes::record_dismissal(&phone.pool, "theme-1", 3)
            .await
            .expect("phone dismissal");
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let hub_theme = theme_json(&hub_replica, "theme-1").await;
        let phone_theme = theme_json(&phone, "theme-1").await;
        assert_eq!(hub_theme["theme"]["has_conflict"], true);
        assert_eq!(phone_theme["theme"]["has_conflict"], true);
        assert_eq!(hub_theme["theme"], phone_theme["theme"]);
        assert_eq!(hub_theme["tokens"], phone_theme["tokens"]);
        assert_eq!(phone_theme["theme"]["seed_blend_canvas"], "#101010");
        assert_eq!(phone_theme["seedTokens"][0]["value"], "#101010");
        for replica in [&hub_replica, &phone] {
            let dismissals = ganbaru_themes::load_dismissals(&replica.pool)
                .await
                .expect("load dismissals");
            let dismissals = serde_json::to_value(dismissals).expect("serialize dismissals");
            assert_eq!(dismissals[0]["engine_version"], 5);
        }

        let conflict = ganbaru_themes::conflict_from_pool(
            &phone.pool,
            VAULT_ID,
            &ganbaru_sync::DeviceNames::new(PHONE_DEVICE, HashMap::new()),
            "theme-1",
        )
        .await
        .expect("read conflict");
        let conflict = serde_json::to_value(conflict).expect("serialize conflict");
        let versions = conflict["versions"].as_array().expect("versions");
        assert_eq!(versions.len(), 2);
        let own = versions
            .iter()
            .find(|version| version["device"]["ownDevice"] == true)
            .expect("own version");
        assert_eq!(own["blendCanvas"], "#333333");
        assert_eq!(own["palette"].as_array().map(Vec::len), Some(32));
        let displayed = versions
            .iter()
            .find(|version| version["displayed"] == true)
            .expect("displayed version");
        assert_eq!(
            displayed["blendCanvas"],
            phone_theme["theme"]["blend_canvas"]
        );
        let other = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("hidden version");

        let resolution = serde_json::from_value(serde_json::json!({
            "id": "theme-1",
            "choice": { "kind": "version", "version": other["version"] },
        }))
        .expect("resolution");
        let resolved =
            ganbaru_themes::resolve_from_pool(&phone.pool, VAULT_ID, resolution, NOW_MS + 1)
                .await
                .expect("resolve conflict");
        let resolved = serde_json::to_value(resolved).expect("serialize resolution");
        assert_eq!(resolved["theme"]["blend_canvas"], other["blendCanvas"]);
        assert_eq!(resolved["theme"]["has_conflict"], false);

        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let theme = theme_json(replica, "theme-1").await;
            assert_eq!(theme["theme"]["blend_canvas"], other["blendCanvas"]);
            assert_eq!(theme["tokens"][0]["value"], other["blendCanvas"]);
            assert_eq!(theme["theme"]["has_conflict"], false);
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn restoring_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere() {
    block_on(async {
        let mut hub_replica = Replica::new("recovery-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("recovery-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-2").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-2")
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Kept edit' WHERE id = ?",
                "note-2",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let offer = vec![("note-2".to_string(), "Kept edit".to_string())];
        for replica in [&hub_replica, &phone] {
            assert!(!replica.has_note("note-2").await);
            assert_eq!(recovery_titles(replica).await, offer);
        }

        let restored = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &phone.ctx,
            &mut phone.writer,
            &restorers(),
            &RecoveryChoice {
                table: NOTES_TABLE,
                row_key: "note-2".to_string(),
                action: RecoveryAction::Restore,
            },
            NOW_MS + 1,
        )
        .await
        .expect("restore")
        .expect("restored note id");
        assert_eq!(note_json(&phone, &restored).await["title"], "Kept edit");

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert!(replica.has_note(&restored).await);
            assert!(!replica.has_note("note-2").await);
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_restore_whose_close_fails_leaves_no_restored_row_and_the_offer_open() {
    block_on(async {
        let mut hub_replica = Replica::new("atomic-restore-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("atomic-restore-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-4").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-4")
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Kept edit' WHERE id = ?",
                "note-4",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        let offer = vec![("note-4".to_string(), "Kept edit".to_string())];
        assert_eq!(recovery_titles(&phone).await, offer);

        let choice = RecoveryChoice {
            table: NOTES_TABLE,
            row_key: "note-4".to_string(),
            action: RecoveryAction::Restore,
        };
        let foreign = SpaceContext {
            space: SpaceId::personal("another-vault"),
            ..phone.ctx.clone()
        };
        let error = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &foreign,
            &mut phone.writer,
            &restorers(),
            &choice,
            NOW_MS + 1,
        )
        .await
        .expect_err("closing in another space fails");
        assert!(error.contains("close recovery offer"), "{error}");
        let notes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quick_notes")
            .fetch_one(&phone.pool)
            .await
            .expect("count notes");
        assert_eq!(notes, 0);
        assert_eq!(recovery_titles(&phone).await, offer);

        let restored = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &phone.ctx,
            &mut phone.writer,
            &restorers(),
            &choice,
            NOW_MS + 2,
        )
        .await
        .expect("restore")
        .expect("restored note id");
        let notes: Vec<String> = sqlx::query_scalar("SELECT id FROM quick_notes")
            .fetch_all(&phone.pool)
            .await
            .expect("list notes");
        assert_eq!(notes, [restored]);
        assert!(recovery_titles(&phone).await.is_empty());
        hub_replica.close().await;
        phone.close().await;
    });
}

/// Restores a recovered Music playlist through its domain persistence, as the app does.
fn restore_music_playlist(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        ganbaru_music_library::recovery::restore_recovered_playlist(
            conn,
            &row.values,
            &row.descendants,
            i64::try_from(NOW_MS + 1).expect("restore time"),
        )
        .await
        .map_err(|error| error.to_string())
    })
}

#[test]
fn a_restored_music_playlist_keeps_concurrent_membership_skip_range_and_intended_use_edits() {
    block_on(async {
        use ganbaru_sync::manifest::vault::music::{INTENDED_USES, MEMBERSHIPS, PLAYLISTS};
        let mut hub_replica = Replica::new("music-recovery-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("music-recovery-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let playlist = "music-playlist-recovered";
        let item = "music-item-recovered";
        let membership = music_key(&MEMBERSHIPS, &[playlist, item]);
        let order_key = ganbaru_sync_contracts::OrderKey::rank(0)
            .expect("order key")
            .into_string();
        edit_sql(
            &mut hub_replica,
            format!(
                "INSERT INTO music_library_items
                    (id, identity_key, source_kind, media_kind, original_title, discovered_at_ms,
                     updated_at_ms)
                 VALUES ('{item}', 'local:{item}', 'local-file', 'audio', 'Song', {NOW_MS}, {NOW_MS});
                 INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms, order_key)
                 VALUES ('{playlist}', 'Focus mix', {NOW_MS}, {NOW_MS}, '{order_key}');
                 INSERT INTO music_playlist_memberships
                    (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms)
                 VALUES ('{membership}', '{playlist}', '{item}', '{order_key}', {NOW_MS}, {NOW_MS});
                 INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
                 VALUES ('{membership}', 0, 1000, 2000);
                 INSERT INTO music_playlist_intended_uses (playlist_id, intended_use, sync_key)
                 VALUES ('{playlist}', 'focus', '{focus}');",
                focus = music_key(&INTENDED_USES, &[playlist, "focus"]),
            ),
        )
        .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        edit_sql(
            &mut hub_replica,
            format!("DELETE FROM music_playlists WHERE id = '{playlist}'"),
        )
        .await;
        edit_sql(
            &mut phone,
            format!(
                "UPDATE music_playlist_memberships SET weight = 'more-often'
                 WHERE id = '{membership}';
                 INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
                 VALUES ('{membership}', 1, 5000, 6000);
                 INSERT INTO music_playlist_intended_uses (playlist_id, intended_use, sync_key)
                 VALUES ('{playlist}', 'reading', '{reading}');",
                reading = music_key(&INTENDED_USES, &[playlist, "reading"]),
            ),
        )
        .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                recovery_titles(replica).await,
                [(playlist.to_string(), "Focus mix".to_string())]
            );
        }

        let restored = crate::recovery::close(
            &Engine::vault(),
            &hub_replica.pool,
            &hub_replica.ctx,
            &mut hub_replica.writer,
            &RecoveryRestorers::new().with(&PLAYLISTS, restore_music_playlist),
            &RecoveryChoice {
                table: PLAYLISTS.id,
                row_key: playlist.to_string(),
                action: RecoveryAction::Restore,
            },
            NOW_MS + 1,
        )
        .await
        .expect("restore")
        .expect("restored playlist id");
        assert_ne!(restored, playlist);

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT name || '|' || icon FROM music_playlists WHERE id = '{restored}'"
                    )
                )
                .await,
                ["Focus mix|lucide:list-music"]
            );
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT item_id || '|' || weight FROM music_playlist_memberships
                         WHERE playlist_id = '{restored}'"
                    )
                )
                .await,
                [format!("{item}|more-often")]
            );
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT skip.start_ms || '-' || skip.end_ms
                         FROM music_membership_skip_ranges AS skip
                         JOIN music_playlist_memberships AS membership
                           ON membership.id = skip.membership_id
                         WHERE membership.playlist_id = '{restored}'
                         ORDER BY skip.ordinal"
                    )
                )
                .await,
                ["1000-2000", "5000-6000"]
            );
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT intended_use FROM music_playlist_intended_uses
                         WHERE playlist_id = '{restored}' ORDER BY intended_use"
                    )
                )
                .await,
                ["focus", "reading"]
            );
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn discarding_a_recovery_offer_keeps_the_deletion() {
    block_on(async {
        let mut hub_replica = Replica::new("discard-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("discard-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-3").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        phone
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-3")
            .await;
        hub_replica
            .edit(
                "UPDATE quick_notes SET title = 'Late edit' WHERE id = ?",
                "note-3",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        assert_eq!(recovery_titles(&hub_replica).await.len(), 1);

        let restored = crate::recovery::close(
            &Engine::vault(),
            &hub_replica.pool,
            &hub_replica.ctx,
            &mut hub_replica.writer,
            &restorers(),
            &RecoveryChoice {
                table: NOTES_TABLE,
                row_key: "note-3".to_string(),
                action: RecoveryAction::Discard,
            },
            NOW_MS + 1,
        )
        .await
        .expect("discard");
        assert_eq!(restored, None);

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert!(!replica.has_note("note-3").await);
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}
