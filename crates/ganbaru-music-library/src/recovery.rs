//! Restoring Music rows offered for sync recovery.
//!
//! A deleted playlist comes back as a new playlist with a key of its own, its retained name,
//! icon, and playback settings (the defaults of a new playlist when the icon or playback
//! settings were not retained), and copies of its deleted intended uses and memberships. A
//! membership whose item no longer exists is left out, and a break item that no longer exists
//! is cleared together with its break range.

use std::collections::BTreeSet;

use ganbaru_sync::RecoveryDescendant;
use ganbaru_sync::codec;
use ganbaru_sync::manifest::vault::music::{
    INTENDED_USES_TABLE, MEMBERSHIPS_TABLE, SKIP_RANGES, intended_use_group, membership_group,
    playlist_group,
};
use ganbaru_sync_contracts::{Field, GroupId, Value};
use ring::rand::{SecureRandom, SystemRandom};
use sqlx::{Connection, Sqlite, SqliteConnection, Transaction};

use crate::writes::{commit, insert_intended_use, map_database_error};
use crate::{
    DEFAULT_PLAYLIST_ICON, DEFAULT_PLAYLIST_REPEAT_MODE, MusicLibraryError, MusicLibraryResult,
    validate_icon, validate_name,
};

/// Retained playlist settings.
struct RecoveredPlaylist {
    name: String,
    icon: String,
    shuffle_enabled: i64,
    repeat_mode: String,
    mix_enabled: i64,
}

/// Retained values of one deleted membership.
#[derive(Debug, Default)]
struct RecoveredMembership {
    item_id: Option<String>,
    order_key: Option<String>,
    weight: Option<String>,
    enabled: Option<i64>,
    range: [Option<i64>; 4],
    break_item_id: Option<String>,
    break_range: [Option<i64>; 4],
    skips: Vec<(i64, i64)>,
}

/// A random version 4 UUID, the form the frontend gives new playlists.
fn new_playlist_id() -> MusicLibraryResult<String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new().fill(&mut bytes).map_err(|_| {
        MusicLibraryError::runtime(
            "create playlist id",
            "secure random generator is unavailable",
        )
    })?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Creates a new playlist from the retained values of a deleted playlist and its deleted
/// descendants, in one transaction, and returns its id.
pub async fn restore_recovered_playlist(
    conn: &mut SqliteConnection,
    values: &[(GroupId, Value)],
    descendants: &[RecoveryDescendant],
    now_ms: i64,
) -> MusicLibraryResult<String> {
    let new_playlist_id = new_playlist_id()?;
    let new_playlist_id = new_playlist_id.as_str();
    if now_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "restoredAt",
            "must be greater than zero",
        ));
    }
    let playlist = recovered_playlist(values)?;
    validate_name(&playlist.name)?;
    validate_icon(&playlist.icon)?;
    let mut intended_uses = BTreeSet::new();
    let mut memberships = Vec::new();
    for descendant in descendants {
        if descendant.table == INTENDED_USES_TABLE {
            intended_uses.insert(recovered_intended_use(&descendant.values)?);
        } else if descendant.table == MEMBERSHIPS_TABLE {
            memberships.push(recovered_membership(&descendant.values)?);
        } else {
            return Err(invalid(format!(
                "a recovered playlist has a descendant in table {}",
                descendant.table.0
            )));
        }
    }
    memberships.sort_by(|left, right| left.order_key.cmp(&right.order_key));

    let mut transaction = conn
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin restore music playlist", error))?;
    let tail = crate::order::playlist_tail(&mut transaction).await?;
    let order_key = crate::order::key_after(tail.as_deref())?;
    sqlx::query(
        "INSERT INTO music_playlists
            (id, name, icon, shuffle_enabled, mix_enabled, repeat_mode, order_key, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(new_playlist_id)
    .bind(playlist.name.trim())
    .bind(playlist.icon.trim())
    .bind(playlist.shuffle_enabled)
    .bind(playlist.mix_enabled)
    .bind(&playlist.repeat_mode)
    .bind(order_key.as_str())
    .bind(now_ms)
    .bind(now_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_database_error("restore music playlist", error))?;
    for intended_use in &intended_uses {
        insert_intended_use(&mut transaction, new_playlist_id, intended_use).await?;
    }
    let mut kept = Vec::with_capacity(memberships.len());
    for membership in memberships {
        let Some(item_id) = membership.item_id.as_deref() else {
            continue;
        };
        if item_exists(&mut transaction, item_id).await? {
            kept.push(membership);
        }
    }
    let order_keys = crate::order::rank_keys(kept.len())?;
    for (membership, order_key) in kept.iter().zip(order_keys) {
        insert_membership(
            &mut transaction,
            new_playlist_id,
            membership,
            order_key.as_str(),
            now_ms,
        )
        .await?;
    }
    crate::search::refresh_playlist(&mut transaction, new_playlist_id).await?;
    commit(transaction, "commit restore music playlist").await?;
    Ok(new_playlist_id.to_string())
}

fn invalid(message: String) -> MusicLibraryError {
    MusicLibraryError::runtime("read recovered music values", message)
}

fn recovered_playlist(values: &[(GroupId, Value)]) -> MusicLibraryResult<RecoveredPlaylist> {
    let mut name = None;
    let mut icon = None;
    let mut playback = None;
    for (group, value) in values {
        match (*group, value.fields()) {
            (playlist_group::NAME, [Field::Text(text)]) => name = Some(text.clone()),
            (playlist_group::ICON, [Field::Text(text)]) => icon = Some(text.clone()),
            (
                playlist_group::PLAYBACK,
                [
                    Field::Integer(shuffle),
                    Field::Text(repeat),
                    Field::Integer(mix),
                ],
            ) => playback = Some((*shuffle, repeat.clone(), *mix)),
            _ => {}
        }
    }
    let name = name.ok_or_else(|| invalid("a recovered playlist has no name".to_string()))?;
    let icon = icon.unwrap_or_else(|| DEFAULT_PLAYLIST_ICON.to_string());
    let (shuffle_enabled, repeat_mode, mix_enabled) =
        playback.unwrap_or_else(|| (0, DEFAULT_PLAYLIST_REPEAT_MODE.as_ref().to_string(), 0));
    Ok(RecoveredPlaylist {
        name,
        icon,
        shuffle_enabled,
        repeat_mode,
        mix_enabled,
    })
}

fn recovered_intended_use(values: &[(GroupId, Value)]) -> MusicLibraryResult<String> {
    values
        .iter()
        .find_map(|(group, value)| match (*group, value.fields()) {
            (intended_use_group::IDENTITY, [_, Field::Text(intended_use)]) => {
                Some(intended_use.clone())
            }
            _ => None,
        })
        .ok_or_else(|| invalid("a recovered intended use has no identity".to_string()))
}

fn optional_integer(field: &Field) -> MusicLibraryResult<Option<i64>> {
    match field {
        Field::Null => Ok(None),
        Field::Integer(value) => Ok(Some(*value)),
        _ => Err(invalid(
            "a recovered range bound is not an integer".to_string(),
        )),
    }
}

fn range(fields: &[Field]) -> MusicLibraryResult<[Option<i64>; 4]> {
    let [start, end, volume, rate] = fields else {
        return Err(invalid("a recovered range has the wrong shape".to_string()));
    };
    Ok([
        optional_integer(start)?,
        optional_integer(end)?,
        optional_integer(volume)?,
        optional_integer(rate)?,
    ])
}

fn skip_ranges(bytes: &[u8]) -> MusicLibraryResult<Vec<(i64, i64)>> {
    codec::decode(&SKIP_RANGES, bytes)
        .map_err(|reason| invalid(format!("decode recovered skip ranges: {reason}")))?
        .into_iter()
        .map(|row| match row.fields.as_slice() {
            [Field::Integer(start), Field::Integer(end)] => Ok((*start, *end)),
            _ => Err(invalid(
                "a recovered skip range has the wrong shape".to_string(),
            )),
        })
        .collect()
}

fn recovered_membership(values: &[(GroupId, Value)]) -> MusicLibraryResult<RecoveredMembership> {
    let mut membership = RecoveredMembership::default();
    for (group, value) in values {
        match (*group, value.fields()) {
            (membership_group::IDENTITY, [_, Field::Text(item_id)]) => {
                membership.item_id = Some(item_id.clone());
            }
            (membership_group::ORDER, [Field::Text(order_key)]) => {
                membership.order_key = Some(order_key.clone());
            }
            (membership_group::WEIGHT, [Field::Text(weight)]) => {
                membership.weight = Some(weight.clone());
            }
            (membership_group::ENABLED, [Field::Integer(enabled)]) => {
                membership.enabled = Some(*enabled);
            }
            (membership_group::RANGE, fields) => membership.range = range(fields)?,
            (membership_group::BREAK_ITEM, [Field::Text(item_id)]) => {
                membership.break_item_id = Some(item_id.clone());
            }
            (membership_group::BREAK_RANGE, fields) => membership.break_range = range(fields)?,
            (membership_group::SKIPS, [Field::Blob(bytes)]) => {
                membership.skips = skip_ranges(bytes)?;
            }
            _ => {}
        }
    }
    Ok(membership)
}

async fn item_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    item_id: &str,
) -> MusicLibraryResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_library_items WHERE id = ?)")
        .bind(item_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("check recovered membership item", error))
}

/// Inserts a copy of a recovered membership under the key derived from the new playlist.
async fn insert_membership(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    membership: &RecoveredMembership,
    order_key: &str,
    now_ms: i64,
) -> MusicLibraryResult<()> {
    let Some(item_id) = membership.item_id.as_deref() else {
        return Ok(());
    };
    let weight = membership
        .weight
        .as_deref()
        .ok_or_else(|| invalid("a recovered membership has no weight".to_string()))?;
    let enabled = membership
        .enabled
        .ok_or_else(|| invalid("a recovered membership has no enabled flag".to_string()))?;
    let break_item_id = match membership.break_item_id.as_deref() {
        Some(break_item_id) if item_exists(transaction, break_item_id).await? => {
            Some(break_item_id)
        }
        _ => None,
    };
    let break_range = if break_item_id.is_some() {
        membership.break_range
    } else {
        [None; 4]
    };
    let id = crate::identity::membership_id(playlist_id, item_id)?;
    let [start_ms, end_ms, volume_milli, rate_milli] = membership.range;
    let [
        break_start_ms,
        break_end_ms,
        break_volume_milli,
        break_rate_milli,
    ] = break_range;
    sqlx::query(
        "INSERT INTO music_playlist_memberships
            (id, playlist_id, item_id, order_key, weight, enabled, start_ms, end_ms,
             volume_milli, rate_milli, break_item_id, break_start_ms, break_end_ms,
             break_volume_milli, break_rate_milli, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(&id)
    .bind(playlist_id)
    .bind(item_id)
    .bind(order_key)
    .bind(weight)
    .bind(enabled)
    .bind(start_ms)
    .bind(end_ms)
    .bind(volume_milli)
    .bind(rate_milli)
    .bind(break_item_id)
    .bind(break_start_ms)
    .bind(break_end_ms)
    .bind(break_volume_milli)
    .bind(break_rate_milli)
    .bind(now_ms)
    .bind(now_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_database_error("restore playlist membership", error))?;
    for ((start_ms, end_ms), ordinal) in membership.skips.iter().zip(0_i64..) {
        sqlx::query(
            "INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(ordinal)
        .bind(start_ms)
        .bind(end_ms)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("restore membership skip range", error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ganbaru_sync::codec::OwnedRow;
    use ganbaru_sync::manifest::vault::music::PLAYLISTS_TABLE;

    use super::*;
    use crate::test_support::block_on;
    use crate::tests::{pool, seed_item};

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn text(value: &str) -> Field {
        Field::Text(value.to_string())
    }

    fn playlist_values() -> Vec<(GroupId, Value)> {
        vec![
            (playlist_group::CREATED, value(vec![Field::Integer(1)])),
            (playlist_group::NAME, value(vec![text("Deep work")])),
            (playlist_group::ICON, value(vec![text("lucide:laptop")])),
            (
                playlist_group::PLAYBACK,
                value(vec![Field::Integer(1), text("all"), Field::Integer(0)]),
            ),
        ]
    }

    fn membership(
        item_id: &str,
        order_key: &str,
        break_item_id: Option<&str>,
        skips: &[(i64, i64)],
    ) -> RecoveryDescendant {
        let rows: Vec<OwnedRow> = skips
            .iter()
            .map(|(start, end)| OwnedRow::flat(vec![Field::Integer(*start), Field::Integer(*end)]))
            .collect();
        let skips = codec::encode(&SKIP_RANGES, &rows).unwrap();
        let range = |start: i64| {
            value(vec![
                Field::Integer(start),
                Field::Null,
                Field::Integer(500),
                Field::Null,
            ])
        };
        RecoveryDescendant {
            table: MEMBERSHIPS_TABLE,
            row_key: format!("old:{item_id}"),
            values: vec![
                (
                    membership_group::IDENTITY,
                    value(vec![text("deleted-playlist"), text(item_id)]),
                ),
                (membership_group::ORDER, value(vec![text(order_key)])),
                (membership_group::WEIGHT, value(vec![text("more-often")])),
                (membership_group::ENABLED, value(vec![Field::Integer(0)])),
                (membership_group::RANGE, range(1_000)),
                (
                    membership_group::BREAK_ITEM,
                    value(vec![break_item_id.map_or(Field::Null, text)]),
                ),
                (membership_group::BREAK_RANGE, range(2_000)),
                (membership_group::SKIPS, value(vec![Field::Blob(skips)])),
            ],
        }
    }

    fn intended_use(intended_use: &str) -> RecoveryDescendant {
        RecoveryDescendant {
            table: INTENDED_USES_TABLE,
            row_key: format!("old:{intended_use}"),
            values: vec![(
                intended_use_group::IDENTITY,
                value(vec![text("deleted-playlist"), text(intended_use)]),
            )],
        }
    }

    #[test]
    fn recovery_restores_a_playlist_with_its_surviving_memberships_under_a_new_key() {
        block_on(async {
            let pool = pool().await;
            seed_item(&pool, "item-a", "local-file:a").await;
            seed_item(&pool, "item-b", "local-file:b").await;
            let descendants = vec![
                membership("item-b", "b", Some("item-a"), &[(10, 20), (30, 40)]),
                membership("item-gone", "c", None, &[]),
                membership("item-a", "a", Some("item-gone"), &[]),
                intended_use("focus"),
                intended_use("reading"),
            ];
            let id = restore_recovered_playlist(
                &mut pool.acquire().await.unwrap(),
                &playlist_values(),
                &descendants,
                42,
            )
            .await
            .unwrap();
            assert_ne!(id, "deleted-playlist");

            let playlist: (String, String, i64, String, i64, i64) = sqlx::query_as(
                "SELECT name, icon, shuffle_enabled, repeat_mode, mix_enabled, created_at_ms
                 FROM music_playlists WHERE id = ?",
            )
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                playlist,
                (
                    "Deep work".to_string(),
                    "lucide:laptop".to_string(),
                    1,
                    "all".to_string(),
                    0,
                    42
                )
            );
            let uses: Vec<String> = sqlx::query_scalar(
                "SELECT intended_use FROM music_playlist_intended_uses WHERE playlist_id = ?
                 ORDER BY intended_use",
            )
            .bind(&id)
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(uses, vec!["focus", "reading"]);

            type Row = (
                String,
                String,
                String,
                i64,
                Option<i64>,
                Option<i64>,
                Option<String>,
                Option<i64>,
            );
            let rows: Vec<Row> = sqlx::query_as(
                "SELECT id, item_id, weight, enabled, start_ms, volume_milli, break_item_id,
                        break_start_ms
                 FROM music_playlist_memberships WHERE playlist_id = ? ORDER BY order_key",
            )
            .bind(&id)
            .fetch_all(&pool)
            .await
            .unwrap();
            let expected = |item: &str, break_item: Option<&str>, break_start: Option<i64>| {
                (
                    crate::identity::membership_id(&id, item).unwrap(),
                    item.to_string(),
                    "more-often".to_string(),
                    0,
                    Some(1_000),
                    Some(500),
                    break_item.map(str::to_string),
                    break_start,
                )
            };
            assert_eq!(
                rows,
                vec![
                    expected("item-a", None, None),
                    expected("item-b", Some("item-a"), Some(2_000)),
                ]
            );
            let skips: Vec<(i64, i64, i64)> = sqlx::query_as(
                "SELECT ordinal, start_ms, end_ms FROM music_membership_skip_ranges
                 WHERE membership_id = ? ORDER BY ordinal",
            )
            .bind(crate::identity::membership_id(&id, "item-b").unwrap())
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(skips, vec![(0, 10, 20), (1, 30, 40)]);
        });
    }

    #[test]
    fn recovery_restores_new_playlist_defaults_when_icon_and_playback_were_not_retained() {
        block_on(async {
            let pool = pool().await;
            let name_only: Vec<_> = playlist_values()
                .into_iter()
                .filter(|(group, _)| {
                    *group != playlist_group::ICON && *group != playlist_group::PLAYBACK
                })
                .collect();
            let id = restore_recovered_playlist(
                &mut pool.acquire().await.unwrap(),
                &name_only,
                &[intended_use("focus")],
                42,
            )
            .await
            .unwrap();
            let (name, icon, shuffle, repeat, mix): (String, String, i64, String, i64) =
                sqlx::query_as(
                    "SELECT name, icon, shuffle_enabled, repeat_mode, mix_enabled
                     FROM music_playlists WHERE id = ?",
                )
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                (name.as_str(), icon.as_str(), shuffle, repeat.as_str(), mix),
                ("Deep work", DEFAULT_PLAYLIST_ICON, 0, "all", 0)
            );
        });
    }

    #[test]
    fn recovery_refuses_a_playlist_without_a_retained_name_or_with_foreign_descendants() {
        block_on(async {
            let pool = pool().await;
            let without_name: Vec<_> = playlist_values()
                .into_iter()
                .filter(|(group, _)| *group != playlist_group::NAME)
                .collect();
            assert!(
                restore_recovered_playlist(
                    &mut pool.acquire().await.unwrap(),
                    &without_name,
                    &[],
                    42
                )
                .await
                .is_err()
            );
            let foreign = RecoveryDescendant {
                table: PLAYLISTS_TABLE,
                row_key: "other".to_string(),
                values: Vec::new(),
            };
            assert!(
                restore_recovered_playlist(
                    &mut pool.acquire().await.unwrap(),
                    &playlist_values(),
                    &[foreign],
                    42
                )
                .await
                .is_err()
            );
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM music_playlists WHERE name = 'Deep work'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(count, 0);
        });
    }
}
