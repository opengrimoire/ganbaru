//! Music across replicas: concurrent discovery and membership edits settle on one row per
//! derived identity, a playlist deleted while another device adds to it is offered for
//! recovery and can be restored, listening statistics keep one row per device, and device
//! observations stay on the device that made them.

use ganbaru_sync_contracts::derived_key;

use super::block_on;
use super::replica::{Replica, converge, device_id, quoted, rank};
use crate::manifest::TableSpec;
use crate::manifest::vault::music::{
    ASSIGNMENTS, COLLECTION_ITEMS, INTENDED_USES, INTENDED_USES_TABLE, MEMBERSHIPS,
    PLAYLISTS_TABLE, PRESET_SOUNDS, SIGNALS, STATISTICS,
};

const ITEM: &str = "music-item-shared";
const PLAYLIST: &str = "music-playlist-shared";
const NOW_MS: i64 = 1_800_000_000_000;

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 48).await);
    }
    replicas
}

/// The derived row key of `table` for `parts`, in its key input order.
fn key(table: &TableSpec, parts: &[&str]) -> String {
    let derived = table
        .derived_key
        .as_ref()
        .expect("table should have a derived key");
    let prefix = derived.prefix.expect("derived key should have a prefix");
    derived_key(prefix, parts)
        .expect("derived key parts should be valid")
        .into_string()
}

fn insert_item(id: &str, title: &str) -> String {
    format!(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title, discovered_at_ms, updated_at_ms)
         VALUES ({id}, {identity}, 'local-file', 'audio', {title}, {NOW_MS}, {NOW_MS});",
        id = quoted(id),
        identity = quoted(&format!("local:{id}")),
        title = quoted(title),
    )
}

fn insert_playlist(id: &str, name: &str, position: u32) -> String {
    format!(
        "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms, order_key)
         VALUES ({}, {}, {NOW_MS}, {NOW_MS}, {});",
        quoted(id),
        quoted(name),
        quoted(&rank(position)),
    )
}

fn insert_membership(playlist: &str, item: &str, position: u32, at_ms: i64) -> String {
    format!(
        "INSERT INTO music_playlist_memberships
            (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms)
         VALUES ({}, {}, {}, {}, {at_ms}, {at_ms});",
        quoted(&key(&MEMBERSHIPS, &[playlist, item])),
        quoted(playlist),
        quoted(item),
        quoted(&rank(position)),
    )
}

fn insert_intended_use(playlist: &str, intended_use: &str) -> String {
    format!(
        "INSERT INTO music_playlist_intended_uses (playlist_id, intended_use, sync_key)
         VALUES ({}, {}, {});",
        quoted(playlist),
        quoted(intended_use),
        quoted(&key(&INTENDED_USES, &[playlist, intended_use])),
    )
}

/// Every replica holding `sql` (a shared item and playlist), with operations exchanged.
async fn shared(count: usize, sql: &str) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0].exec(sql).await;
    converge(&mut replicas).await;
    replicas
}

async fn texts(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

async fn assert_converged(replicas: &mut [Replica]) {
    let first = replicas[0].domain_dump().await;
    for replica in replicas.iter_mut().skip(1) {
        assert_eq!(
            replica.domain_dump().await,
            first,
            "replica {} diverged",
            replica.index
        );
    }
}

async fn memberships(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT playlist_id || '>' || item_id || ':' || weight
         FROM music_playlist_memberships ORDER BY order_key, id",
    )
    .await
}

#[test]
fn music_concurrent_discovery_of_one_file_settles_on_one_item() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_item(ITEM, "Desk copy")).await;
        replicas[1].exec(&insert_item(ITEM, "Laptop copy")).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM music_library_items")
                    .await,
                1
            );
        }
    });
}

#[test]
fn music_concurrent_add_of_one_membership_settles_on_its_derived_key() {
    block_on(async {
        let setup = format!(
            "{}{}",
            insert_item(ITEM, "Song"),
            insert_playlist(PLAYLIST, "Focus", 10)
        );
        let mut replicas = shared(3, &setup).await;
        replicas[0]
            .exec(&insert_membership(PLAYLIST, ITEM, 0, NOW_MS + 1))
            .await;
        replicas[1]
            .exec(&insert_membership(PLAYLIST, ITEM, 1, NOW_MS + 2))
            .await;
        replicas[2]
            .exec(&format!(
                "{}{}",
                insert_membership(PLAYLIST, ITEM, 2, NOW_MS + 3),
                "UPDATE music_playlist_memberships SET weight = 'more-often';"
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let expected = key(&MEMBERSHIPS, &[PLAYLIST, ITEM]);
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT id FROM music_playlist_memberships").await,
                [expected.as_str()]
            );
            assert_eq!(
                memberships(replica).await,
                [format!("{PLAYLIST}>{ITEM}:more-often")]
            );
        }
    });
}

#[test]
fn music_removed_membership_can_be_added_again_on_another_device() {
    block_on(async {
        let setup = format!(
            "{}{}{}",
            insert_item(ITEM, "Song"),
            insert_playlist(PLAYLIST, "Focus", 10),
            insert_membership(PLAYLIST, ITEM, 0, NOW_MS),
        );
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec("DELETE FROM music_playlist_memberships")
            .await;
        converge(&mut replicas).await;
        assert!(memberships(&mut replicas[1]).await.is_empty());

        replicas[1]
            .exec(&insert_membership(PLAYLIST, ITEM, 3, NOW_MS + 10))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                memberships(replica).await,
                [format!("{PLAYLIST}>{ITEM}:normal")]
            );
        }
    });
}

#[test]
fn music_concurrent_reorders_converge() {
    block_on(async {
        let mut setup = insert_playlist(PLAYLIST, "Focus", 10);
        for index in 0..4u32 {
            let item = format!("{ITEM}-{index}");
            setup.push_str(&insert_item(&item, &format!("Song {index}")));
            setup.push_str(&insert_membership(PLAYLIST, &item, index, NOW_MS));
        }
        let mut replicas = shared(2, &setup).await;
        let membership = |index: u32| key(&MEMBERSHIPS, &[PLAYLIST, &format!("{ITEM}-{index}")]);
        replicas[0]
            .exec(&format!(
                "UPDATE music_playlist_memberships SET order_key = {} WHERE id = {}",
                quoted(&rank(9)),
                quoted(&membership(0)),
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE music_playlist_memberships SET order_key = {} WHERE id = {}",
                quoted(&rank(8)),
                quoted(&membership(1)),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let order = texts(
            &mut replicas[0],
            "SELECT item_id FROM music_playlist_memberships ORDER BY order_key, id",
        )
        .await;
        assert_eq!(
            order,
            [
                format!("{ITEM}-2"),
                format!("{ITEM}-3"),
                format!("{ITEM}-1"),
                format!("{ITEM}-0"),
            ]
        );
    });
}

#[test]
fn music_playlist_deleted_while_another_device_adds_to_it_is_offered_and_restored() {
    block_on(async {
        let setup = format!(
            "{}{}",
            insert_item(ITEM, "Song"),
            insert_playlist(PLAYLIST, "Focus", 10)
        );
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM music_playlists WHERE id = {}",
                quoted(PLAYLIST)
            ))
            .await;
        replicas[1]
            .exec(&insert_membership(PLAYLIST, ITEM, 0, NOW_MS + 5))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count(&format!(
                        "SELECT count(*) FROM music_playlists WHERE id = {}",
                        quoted(PLAYLIST)
                    ))
                    .await,
                0
            );
            assert!(memberships(replica).await.is_empty());
        }

        let adder = &mut replicas[1];
        let entries = adder
            .engine
            .recovery_entries(&mut adder.conn, &adder.ctx)
            .await
            .unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.table == PLAYLISTS_TABLE && entry.row_key == PLAYLIST)
            .unwrap_or_else(|| panic!("playlist should be offered for recovery: {entries:?}"));
        assert_eq!(entry.descendants, 1);
        let entry = entry.clone();

        let restored = "music-playlist-restored";
        adder
            .exec(&format!(
                "{}{}",
                insert_playlist(restored, &entry.presentation.title, 10),
                insert_membership(restored, ITEM, 0, NOW_MS + 20),
            ))
            .await;
        adder
            .engine
            .close_recovery(
                &mut adder.conn,
                &adder.ctx,
                entry.table,
                &entry.row_key,
                &mut adder.identity.local(),
                adder.now_ms,
            )
            .await
            .unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                memberships(replica).await,
                [format!("{restored}>{ITEM}:normal")]
            );
            assert!(
                replica
                    .engine
                    .recovery_entries(&mut replica.conn, &replica.ctx)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    });
}

#[test]
fn music_statistics_from_three_devices_keep_one_row_per_device() {
    block_on(async {
        let mut replicas = shared(3, &insert_item(ITEM, "Song")).await;
        for (index, replica) in replicas.iter_mut().enumerate() {
            let device = device_id(index);
            let plays = index as i64 + 1;
            replica
                .exec(&format!(
                    "INSERT INTO music_listening_statistics
                        (device_id, item_id, play_count, updated_at_ms, sync_key)
                     VALUES ({}, {}, {plays}, {NOW_MS}, {})",
                    quoted(&device),
                    quoted(ITEM),
                    quoted(&key(&STATISTICS, &[&device, ITEM])),
                ))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT sum(play_count) FROM music_listening_statistics")
                    .await,
                6
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM music_listening_statistics")
                    .await,
                3
            );
        }

        replicas[2]
            .exec(&format!(
                "UPDATE music_listening_statistics SET play_count = play_count + 4
                 WHERE device_id = {}",
                quoted(&device_id(2))
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            replicas[0]
                .count("SELECT sum(play_count) FROM music_listening_statistics")
                .await,
            10
        );
    });
}

#[test]
fn music_signals_added_on_two_devices_settle_on_one_row() {
    block_on(async {
        let mut replicas = shared(2, &insert_item(ITEM, "Song")).await;
        for (offset, replica) in replicas.iter_mut().enumerate() {
            replica
                .exec(&format!(
                    "INSERT INTO music_item_signals (item_id, signal, created_at_ms, sync_key)
                     VALUES ({}, 'calm', {}, {})",
                    quoted(ITEM),
                    NOW_MS + offset as i64,
                    quoted(&key(&SIGNALS, &[ITEM, "calm"])),
                ))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM music_item_signals")
                .await,
            1
        );
    });
}

#[test]
fn music_playlist_rename_against_icon_change_keeps_both() {
    block_on(async {
        let mut replicas = shared(2, &insert_playlist(PLAYLIST, "Focus", 10)).await;
        let id = quoted(PLAYLIST);
        replicas[0]
            .exec(&format!(
                "UPDATE music_playlists SET name = 'Deep focus' WHERE id = {id}"
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE music_playlists SET icon = 'lucide:brain' WHERE id = {id}"
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    &format!("SELECT name || '|' || icon FROM music_playlists WHERE id = {id}")
                )
                .await,
                ["Deep focus|lucide:brain"]
            );
        }
    });
}

fn insert_collection(id: &str, playlist: &str) -> String {
    format!(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, youtube_playlist_id, created_at_ms, updated_at_ms)
         VALUES ({id}, 'youtube-playlist', {identity}, 'Followed', {playlist}, {NOW_MS}, {NOW_MS});",
        id = quoted(id),
        identity = quoted(&format!("youtube-playlist:{playlist}")),
        playlist = quoted(playlist),
    )
}

#[test]
fn music_rows_deleted_on_one_device_are_added_again_under_one_successor_key() {
    block_on(async {
        let collection = "music-collection-shared";
        let setup = format!(
            "{}{}",
            insert_item(ITEM, "Song"),
            insert_collection(collection, "PLshared")
        );
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM music_source_collections;
                 DELETE FROM music_library_items WHERE id = {};",
                quoted(ITEM)
            ))
            .await;
        converge(&mut replicas).await;
        assert!(
            !replicas[1].try_exec(&insert_item(ITEM, "Song")).await,
            "a published key stays retired"
        );
        assert!(
            !replicas[1]
                .try_exec(&insert_collection(collection, "PLshared"))
                .await
        );

        // The successor key `creatable_key` in the Music library derives for both rows.
        let successor = format!("{ITEM}-2");
        let collection_successor = format!("{collection}-2");
        for replica in &mut replicas {
            replica
                .exec(&format!(
                    "{}{}",
                    insert_item(&successor, "Song"),
                    insert_collection(&collection_successor, "PLshared")
                ))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT id FROM music_library_items").await,
                [successor.as_str()]
            );
            assert_eq!(
                texts(replica, "SELECT id FROM music_source_collections").await,
                [collection_successor.as_str()]
            );
        }
    });
}

#[test]
fn music_playlist_deleted_while_another_device_adds_an_intended_use_is_offered_with_it() {
    block_on(async {
        let mut replicas = shared(2, &insert_playlist(PLAYLIST, "Focus", 10)).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM music_playlists WHERE id = {}",
                quoted(PLAYLIST)
            ))
            .await;
        replicas[1]
            .exec(&insert_intended_use(PLAYLIST, "focus"))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        for replica in &mut replicas {
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            let entry = entries
                .iter()
                .find(|entry| entry.table == PLAYLISTS_TABLE && entry.row_key == PLAYLIST)
                .unwrap_or_else(|| panic!("playlist should be offered: {entries:?}"));
            assert_eq!(entry.descendants, 1);
            let descendants = replica
                .engine
                .recovery_descendants(&mut replica.conn, PLAYLISTS_TABLE, PLAYLIST)
                .await
                .unwrap()
                .expect("the playlist is offered");
            assert_eq!(descendants.len(), 1);
            assert_eq!(descendants[0].table, INTENDED_USES_TABLE);
        }
    });
}

/// A YouTube item as a device writes it: `resolved` items carry fetched metadata and the
/// `ready` state, unresolved ones the placeholder written while resolution is pending.
fn insert_video(id: &str, video: &str, resolved: bool) -> String {
    let (title, flag, state) = if resolved {
        ("Endless Embrace", 1, "ready")
    } else {
        ("", 0, "resolving")
    };
    format!(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, youtube_video_id, media_kind, original_title,
             metadata_resolved, youtube_resolution_state, discovered_at_ms, updated_at_ms)
         VALUES ({id}, {identity}, 'youtube-video', {video}, 'video', {title}, {flag}, {state},
                 {NOW_MS}, {NOW_MS});",
        id = quoted(id),
        identity = quoted(&format!("youtube:{video}")),
        video = quoted(video),
        title = quoted(title),
        state = quoted(state),
    )
}

#[test]
fn music_resolved_youtube_metadata_and_terminal_states_beat_concurrent_placeholders() {
    block_on(async {
        let video = "music-item-video";
        let id = quoted(video);
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&insert_video(video, "abcDEF_1234", true))
            .await;
        // The placeholder is written later, so a plain register would let it win.
        replicas[1]
            .exec(&insert_video(video, "abcDEF_1234", false))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let row = format!(
            "SELECT original_title || '|' || metadata_resolved || '|' || youtube_resolution_state
             FROM music_library_items WHERE id = {id}"
        );
        for replica in &mut replicas {
            assert_eq!(texts(replica, &row).await, ["Endless Embrace|1|ready"]);
        }

        replicas[0]
            .exec(&format!(
                "UPDATE music_library_items SET youtube_resolution_state = 'unavailable'
                 WHERE id = {id}"
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE music_library_items SET youtube_resolution_state = 'resolving'
                 WHERE id = {id}"
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let state =
            format!("SELECT youtube_resolution_state FROM music_library_items WHERE id = {id}");
        for replica in &mut replicas {
            assert_eq!(texts(replica, &state).await, ["unavailable"]);
        }
    });
}

fn insert_collection_item(collection: &str, item: &str, position: u32) -> String {
    format!(
        "INSERT INTO music_source_collection_items
            (collection_id, item_id, source_position, first_discovered_at_ms, sync_key)
         VALUES ({}, {}, {position}, {NOW_MS}, {});",
        quoted(collection),
        quoted(item),
        quoted(&key(&COLLECTION_ITEMS, &[collection, item])),
    )
}

/// Open conflicts and recovery offers on `replica`, which a merge without user input leaves at
/// zero.
async fn surfaced(replica: &mut Replica) -> (i64, usize) {
    let conflicts = replica
        .count("SELECT count(*) FROM sync_rows WHERE conflict_mask <> 0")
        .await;
    let offers = replica
        .engine
        .recovery_entries(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .len();
    (conflicts, offers)
}

#[test]
fn music_same_youtube_playlist_added_offline_on_two_devices_keeps_one_row_per_table() {
    block_on(async {
        let collection = "music-collection-followed";
        let videos = [
            ("music-item-video-a", "abcDEF_0001"),
            ("music-item-video-b", "abcDEF_0002"),
        ];
        let mut replicas = replicas(2).await;
        for (index, replica) in replicas.iter_mut().enumerate() {
            let mut sql = insert_collection(collection, "PLfollowed");
            for (position, (id, video)) in videos.iter().enumerate() {
                // Device 0 resolved metadata before going offline; device 1 still has placeholders.
                sql.push_str(&insert_video(id, video, index == 0));
                sql.push_str(&insert_collection_item(collection, id, position as u32));
            }
            replica.exec(&sql).await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            for (table, expected) in [
                ("music_source_collections", 1),
                ("music_library_items", 2),
                ("music_source_collection_items", 2),
            ] {
                assert_eq!(
                    replica
                        .count(&format!("SELECT count(*) FROM {table}"))
                        .await,
                    expected,
                    "{table} on replica {}",
                    replica.index
                );
            }
            assert_eq!(surfaced(replica).await, (0, 0));
        }
    });
}

#[test]
fn music_different_weights_on_one_membership_settle_on_one_value_without_an_offer() {
    block_on(async {
        let setup = format!(
            "{}{}{}",
            insert_item(ITEM, "Song"),
            insert_playlist(PLAYLIST, "Focus", 10),
            insert_membership(PLAYLIST, ITEM, 0, NOW_MS),
        );
        let mut replicas = shared(3, &setup).await;
        replicas[1]
            .exec("UPDATE music_playlist_memberships SET weight = 'more-often'")
            .await;
        replicas[2]
            .exec("UPDATE music_playlist_memberships SET weight = 'less-often'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let weight = texts(
            &mut replicas[0],
            "SELECT weight FROM music_playlist_memberships",
        )
        .await;
        assert_eq!(weight.len(), 1);
        assert!(
            ["more-often", "less-often"].contains(&weight[0].as_str()),
            "{weight:?}"
        );
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT weight FROM music_playlist_memberships").await,
                weight
            );
            assert_eq!(surfaced(replica).await, (0, 0));
        }
    });
}

fn insert_assignment(owner: &str, playlist: &str) -> String {
    format!(
        "INSERT INTO music_context_assignments
            (owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind,
             updated_at_ms, sync_key)
         VALUES ('project-default', {owner}, 'focus', 'play-automatically', {playlist},
             'explicit', {NOW_MS}, {key});",
        owner = quoted(owner),
        playlist = quoted(playlist),
        key = quoted(&key(&ASSIGNMENTS, &["project-default", owner, "focus"])),
    )
}

#[test]
fn music_focus_playlist_deleted_with_a_replacement_against_a_repoint_never_keeps_the_deleted_one() {
    block_on(async {
        let replacement = "music-playlist-replacement";
        let chosen = "music-playlist-chosen";
        // Either edit may carry the later clock, so both orders are checked.
        for later in [0_usize, 1] {
            let setup = format!(
                "{}{}{}{}",
                insert_playlist(PLAYLIST, "Deep work", 10),
                insert_playlist(replacement, "Replacement", 20),
                insert_playlist(chosen, "Chosen", 30),
                insert_assignment("project-shared", PLAYLIST),
            );
            let mut replicas = shared(2, &setup).await;
            // The playlist delete with a replacement rewrites assignments before deleting.
            replicas[0]
                .exec(&format!(
                    "UPDATE music_context_assignments SET playlist_id = {}, version = version + 1
                     WHERE playlist_id = {};
                     DELETE FROM music_playlists WHERE id = {};",
                    quoted(replacement),
                    quoted(PLAYLIST),
                    quoted(PLAYLIST),
                ))
                .await;
            replicas[1]
                .exec(&format!(
                    "UPDATE music_context_assignments SET playlist_id = {}, version = version + 1",
                    quoted(chosen),
                ))
                .await;
            replicas[later].now_ms += 60_000;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            let expected = if later == 0 { replacement } else { chosen };
            for replica in &mut replicas {
                assert_eq!(
                    texts(replica, "SELECT playlist_id FROM music_context_assignments").await,
                    [expected],
                    "later edit from replica {later}"
                );
                assert_eq!(
                    replica
                        .count(&format!(
                            "SELECT count(*) FROM music_playlists WHERE id = {}",
                            quoted(PLAYLIST)
                        ))
                        .await,
                    0
                );
            }
        }
    });
}

#[test]
fn music_soundscape_deleted_while_a_preset_sound_level_is_edited_cascades_everywhere() {
    block_on(async {
        let soundscape = "music-soundscape-rain";
        let kept = "music-soundscape-wind";
        let preset = "music-preset-shared";
        let mut setup = format!(
            "INSERT INTO music_soundscape_presets (id, name, icon, created_at_ms, updated_at_ms)
             VALUES ({}, 'Evening', 'lucide:moon', {NOW_MS}, {NOW_MS});",
            quoted(preset),
        );
        for (position, id) in [soundscape, kept].into_iter().enumerate() {
            setup.push_str(&format!(
                "INSERT INTO music_soundscapes
                    (id, source_kind, name, availability, created_at_ms, updated_at_ms)
                 VALUES ({id}, 'local-loop', {id}, 'available', {NOW_MS}, {NOW_MS});
                 INSERT INTO music_soundscape_preset_sounds
                    (preset_id, soundscape_id, order_key, level_milli, sync_key)
                 VALUES ({preset}, {id}, {order}, 800, {key});",
                id = quoted(id),
                preset = quoted(preset),
                order = quoted(&rank(position as u32)),
                key = quoted(&key(&PRESET_SOUNDS, &[preset, id])),
            ));
        }
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM music_soundscapes WHERE id = {}",
                quoted(soundscape)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE music_soundscape_preset_sounds SET level_milli = 300 WHERE preset_id = {}",
                quoted(preset)
            ))
            .await;
        replicas[1].now_ms += 60_000;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    &format!(
                        "SELECT soundscape_id || ':' || level_milli
                         FROM music_soundscape_preset_sounds WHERE preset_id = {}
                         ORDER BY order_key",
                        quoted(preset)
                    )
                )
                .await,
                [format!("{kept}:300")]
            );
            assert_eq!(
                replica
                    .count(&format!(
                        "SELECT count(*) FROM music_soundscape_presets WHERE id = {}",
                        quoted(preset)
                    ))
                    .await,
                1
            );
        }
    });
}

const ROOT: &str = "music-root-shared";
const LOCATION: &str = "music-location-shared";

/// Device observation columns of the shared location and its item.
const OBSERVATIONS: &str = "SELECT location.availability || '|'
        || coalesce(location.file_size_bytes, '-') || '|'
        || coalesce(location.modified_at_ms, '-') || '|'
        || coalesce(location.last_seen_generation, '-') || '|' || item.availability
    FROM music_local_locations AS location
    JOIN music_library_items AS item ON item.id = location.item_id";

#[test]
fn music_new_file_reaches_other_devices_unobserved_and_their_observations_stay_local() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec(&format!(
                "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
                 VALUES ({root}, 'Music', {NOW_MS}, {NOW_MS});
                 {item}
                 UPDATE music_library_items SET availability = 'available';
                 INSERT INTO music_local_locations
                    (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
                     availability, last_seen_generation, first_seen_at_ms, updated_at_ms)
                 VALUES ({location}, {item_id}, {root}, 'Album/song.flac', 4096, {NOW_MS},
                     'available', 3, {NOW_MS}, {NOW_MS});",
                root = quoted(ROOT),
                item = insert_item(ITEM, "Song"),
                location = quoted(LOCATION),
                item_id = quoted(ITEM),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            texts(&mut replicas[0], OBSERVATIONS).await,
            [format!("available|4096|{NOW_MS}|3|available")]
        );
        for replica in &mut replicas[1..] {
            assert_eq!(
                texts(replica, OBSERVATIONS).await,
                ["unknown|-|-|-|unknown"]
            );
        }

        // Device 2 binds the root and refreshes: only device observation columns change.
        let pending = "SELECT count(*) FROM sync_capture";
        let before = replicas[2].count(pending).await;
        replicas[2]
            .exec(&format!(
                "UPDATE music_local_locations
                 SET availability = 'available', file_size_bytes = 4096,
                     modified_at_ms = {NOW_MS}, last_seen_generation = 1;
                 UPDATE music_library_items SET availability = 'available';"
            ))
            .await;
        assert_eq!(replicas[2].count(pending).await, before);
        assert_eq!(replicas[2].seal().await.unwrap().sealed_ops, 0);
        let device_one = replicas[1].dump().await;
        converge(&mut replicas).await;
        assert_eq!(replicas[1].dump().await, device_one);
        assert_eq!(
            texts(&mut replicas[2], OBSERVATIONS).await,
            [format!("available|4096|{NOW_MS}|1|available")]
        );
        assert_eq!(
            texts(&mut replicas[1], OBSERVATIONS).await,
            ["unknown|-|-|-|unknown"]
        );
    });
}
