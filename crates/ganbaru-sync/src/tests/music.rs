//! Music across replicas: concurrent discovery and membership edits settle on one row per
//! derived identity, a playlist deleted while another device adds to it is offered for
//! recovery and can be restored, and listening statistics keep one row per device.

use ganbaru_sync_contracts::derived_key;

use super::block_on;
use super::replica::{Replica, converge, device_id, quoted, rank};
use crate::manifest::TableSpec;
use crate::manifest::vault::music::{MEMBERSHIPS, PLAYLISTS_TABLE, SIGNALS, STATISTICS};

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
