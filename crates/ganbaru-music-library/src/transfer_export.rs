//! Consistent, bounded database snapshots for portable Music transfers.

use super::transfer::TransferBinding;
use super::transfer_codec::{MAX_CHILD_ROWS, MAX_MEMBERSHIPS, MAX_PLAYLISTS, validation};
use super::transfer_read::{ITEM_TEXT_COLUMNS, TransferReadBudget};
use super::*;
use sqlx::{Sqlite, Transaction};
use std::collections::{BTreeMap, HashMap};

#[derive(sqlx::FromRow)]
struct Location {
    item_id: String,
    root_id: String,
    relative_path: String,
    availability: String,
}

/// Reads every exported family from one caller-owned SQLite snapshot using batch queries.
pub(super) async fn snapshot(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_ids: &[String],
    exported_at: i64,
    bindings: &[TransferBinding],
) -> MusicLibraryResult<MusicInterchangeDocument> {
    snapshot_with_budget(
        transaction,
        playlist_ids,
        exported_at,
        bindings,
        &mut TransferReadBudget::default(),
    )
    .await
}

/// Read export families within an import review's existing aggregate allowance.
pub(super) async fn snapshot_with_budget(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_ids: &[String],
    exported_at: i64,
    bindings: &[TransferBinding],
    budget: &mut TransferReadBudget,
) -> MusicLibraryResult<MusicInterchangeDocument> {
    if playlist_ids.is_empty() || playlist_ids.len() > MAX_PLAYLISTS {
        return Err(validation("Choose between 1 and 500 playlists"));
    }
    validate_bounded_unique_ids(playlist_ids, "playlistIds")?;
    let selected =
        serde_json::to_string(playlist_ids).map_err(|error| validation(error.to_string()))?;
    let playlists = budget.read::<MusicPlaylistRow>(transaction,
        "SELECT * FROM music_playlists WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id", &["id", "name", "icon", "repeat_mode"], &[&selected], MAX_PLAYLISTS).await?;
    if playlists.len() != playlist_ids.len() {
        return Err(validation(
            "A selected playlist no longer exists or was selected twice",
        ));
    }
    let memberships = budget.read::<MusicMembershipRow>(transaction,
        "SELECT * FROM music_playlist_memberships WHERE playlist_id IN (SELECT value FROM json_each(?)) ORDER BY playlist_id, position, id", &["id", "playlist_id", "item_id", "weight"], &[&selected], MAX_MEMBERSHIPS).await?;
    let item_ids: Vec<_> = memberships.iter().map(|entry| &entry.item_id).collect();
    let item_ids =
        serde_json::to_string(&item_ids).map_err(|error| validation(error.to_string()))?;
    let items = budget.read::<MusicLibraryItemRow>(transaction,
        "SELECT * FROM music_library_items WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id", ITEM_TEXT_COLUMNS, &[&item_ids], MAX_MEMBERSHIPS).await?;
    let locations = budget.read::<Location>(transaction,
        "SELECT item_id, root_id, relative_path, availability FROM music_local_locations WHERE item_id IN (SELECT value FROM json_each(?)) ORDER BY item_id, root_id, relative_path", &["item_id", "root_id", "relative_path", "availability"], &[&item_ids], MAX_CHILD_ROWS).await?;
    let signals = budget.read::<(String, String)>(transaction,
        "SELECT item_id, signal FROM music_item_signals WHERE item_id IN (SELECT value FROM json_each(?)) ORDER BY item_id, signal", &["item_id", "signal"], &[&item_ids], MAX_CHILD_ROWS).await?;
    let ranges = budget.read::<(String, i64, i64)>(transaction,
        "SELECT membership_id, start_ms, end_ms FROM music_membership_skip_ranges WHERE membership_id IN (SELECT id FROM music_playlist_memberships WHERE playlist_id IN (SELECT value FROM json_each(?))) ORDER BY membership_id, sort_order, id", &["membership_id"], &[&selected], MAX_CHILD_ROWS).await?;
    let snoozes = budget.read::<MusicSnoozeRow>(transaction,
        "SELECT * FROM music_snoozes WHERE item_id IN (SELECT value FROM json_each(?)) AND (scope = 'all-playlists' OR playlist_id IN (SELECT value FROM json_each(?))) ORDER BY item_id, starts_at_ms, id", &["id", "item_id", "scope", "playlist_id", "reason"], &[&item_ids, &selected], MAX_CHILD_ROWS).await?;
    let uses = budget.read::<(String, String)>(transaction,
        "SELECT playlist_id, intended_use FROM music_playlist_intended_uses WHERE playlist_id IN (SELECT value FROM json_each(?)) ORDER BY playlist_id, intended_use", &["playlist_id", "intended_use"], &[&selected], MAX_CHILD_ROWS).await?;
    let roots = budget.read::<(String, String)>(transaction,
        "SELECT id, name FROM music_local_roots WHERE id IN (SELECT root_id FROM music_local_locations WHERE item_id IN (SELECT value FROM json_each(?))) ORDER BY id", &["id", "name"], &[&item_ids], MAX_PLAYLISTS).await?;
    let assignments = budget.read::<ganbaru_music::assignments::AssignmentRow>(transaction,
        "SELECT owner_kind, owner_id, phase, behavior, playlist_id, soundscape_id, soundscape_behavior, provenance_kind, provenance_id, updated_at_ms, version FROM music_context_assignments WHERE playlist_id IN (SELECT value FROM json_each(?)) ORDER BY owner_kind, owner_id, phase", &["owner_kind", "owner_id", "phase", "behavior", "playlist_id", "soundscape_id", "soundscape_behavior", "provenance_kind", "provenance_id"], &[&selected], MAX_MEMBERSHIPS).await?;

    let mut items_by_id = HashMap::new();
    for item in items {
        let item = MusicLibraryItem::try_from(item)?;
        items_by_id.insert(
            item.id.clone(),
            MusicInterchangeItem {
                identity_key: item.identity_key,
                source_kind: item.source_kind,
                youtube_video_id: item.youtube_video_id,
                title: item.title_override.unwrap_or(item.original_title),
                artist: item.artist_override.unwrap_or(item.original_artist),
                album: item.album_override.unwrap_or(item.original_album),
                duration_ms: item.duration_ms,
                signals: Vec::new(),
                locations: Vec::new(),
            },
        );
    }
    for location in locations {
        if let Some(item) = items_by_id.get_mut(&location.item_id) {
            item.locations.push(MusicInterchangeLocation {
                root_id: location.root_id,
                relative_path: location.relative_path,
                availability: MusicLocationAvailability::try_from(location.availability.as_str())
                    .map_err(validation)?,
            });
        }
    }
    for (item_id, signal) in signals {
        if let Some(item) = items_by_id.get_mut(&item_id) {
            item.signals
                .push(MusicItemSignal::try_from(signal.as_str()).map_err(validation)?);
        }
    }
    let mut ranges_by_membership: HashMap<String, Vec<MusicInterchangeRange>> = HashMap::new();
    for (membership, start_ms, end_ms) in ranges {
        ranges_by_membership
            .entry(membership)
            .or_default()
            .push(MusicInterchangeRange { start_ms, end_ms });
    }
    let mut snoozes_by_item: HashMap<String, Vec<MusicSnooze>> = HashMap::new();
    for row in snoozes {
        let snooze = MusicSnooze::try_from(row)?;
        snoozes_by_item
            .entry(snooze.item_id.clone())
            .or_default()
            .push(snooze);
    }
    let mut by_playlist: BTreeMap<String, Vec<MusicInterchangeMembership>> = BTreeMap::new();
    let mut membership_bytes = 0usize;
    for row in memberships {
        let membership = MusicPlaylistMembership::try_from(row)?;
        let item = items_by_id
            .get(&membership.item_id)
            .ok_or_else(|| validation("An exported membership has no item"))?;
        let snoozes = snoozes_by_item
            .get(&membership.item_id)
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.scope == MusicSnoozeScope::AllPlaylists
                    || entry.playlist_id.as_deref() == Some(&membership.playlist_id)
            })
            .map(|entry| MusicInterchangeSnooze {
                scope: entry.scope,
                starts_at_ms: entry.starts_at_ms,
                ends_at_ms: entry.ends_at_ms,
                reason: entry.reason.clone(),
            })
            .collect();
        let exported = MusicInterchangeMembership {
            item: item.clone(),
            position: membership.position,
            weight: membership.weight,
            enabled: membership.enabled,
            start_ms: membership.start_ms,
            end_ms: membership.end_ms,
            volume: membership.volume,
            rate: membership.rate,
            skip_ranges: ranges_by_membership
                .remove(&membership.id)
                .unwrap_or_default(),
            snoozes,
        };
        membership_bytes = membership_bytes
            .saturating_add(super::transfer_codec::document_bytes(&exported)?.len());
        if membership_bytes > super::transfer_codec::MAX_BYTES {
            return Err(validation("Music export exceeds the 8 MB safety limit"));
        }
        by_playlist
            .entry(membership.playlist_id)
            .or_default()
            .push(exported);
    }
    let mut uses_by_playlist: HashMap<String, Vec<MusicIntendedUse>> = HashMap::new();
    for (playlist, intended_use) in uses {
        uses_by_playlist
            .entry(playlist)
            .or_default()
            .push(MusicIntendedUse::try_from(intended_use.as_str()).map_err(validation)?);
    }
    let mut exported_playlists = Vec::new();
    for row in playlists {
        let intended_uses = uses_by_playlist.remove(&row.id).unwrap_or_default();
        let playlist = row.into_model(intended_uses)?;
        exported_playlists.push(MusicInterchangePlaylist {
            memberships: by_playlist.remove(&playlist.id).unwrap_or_default(),
            id: playlist.id,
            name: playlist.name,
            icon: playlist.icon,
            shuffle_enabled: playlist.shuffle_enabled,
            mix_enabled: playlist.mix_enabled,
            repeat_mode: playlist.repeat_mode,
            intended_uses: playlist.intended_uses,
        });
    }
    let warnings = roots
        .iter()
        .filter(|(id, _)| {
            !bindings
                .iter()
                .any(|binding| binding.root_id == *id && binding.available)
        })
        .map(|(_, name)| format!("Local music root '{name}' is unavailable on this device."))
        .collect();
    let document = MusicInterchangeDocument {
        format: super::transfer_codec::FORMAT.to_string(),
        version: 1,
        exported_at,
        roots: roots
            .into_iter()
            .map(|(id, name)| MusicInterchangeRoot { id, name })
            .collect(),
        playlists: exported_playlists,
        context_assignments: assignments
            .into_iter()
            .map(ganbaru_music::assignments::decode)
            .collect::<MusicLibraryResult<_>>()?,
        warnings,
    };
    super::transfer_codec::validate_document(&document)?;
    Ok(document)
}

pub(super) fn m3u(
    document: &MusicInterchangeDocument,
    bindings: &[TransferBinding],
) -> MusicLibraryResult<String> {
    let mut contents = String::from("#EXTM3U\n");
    for playlist in &document.playlists {
        for membership in &playlist.memberships {
            let item = &membership.item;
            let path = if let Some(video_id) = &item.youtube_video_id {
                Some(format!("https://www.youtube.com/watch?v={video_id}"))
            } else {
                item.locations.iter().find_map(|location| {
                    if location.availability != MusicLocationAvailability::Available {
                        return None;
                    }
                    let binding = bindings
                        .iter()
                        .find(|binding| binding.root_id == location.root_id && binding.available)?;
                    // Android content identities cannot be represented as portable filesystem paths.
                    if binding.folder_path.starts_with("content://") {
                        return None;
                    }
                    let separator = if binding.windows { "\\" } else { "/" };
                    Some(format!(
                        "{}{}{}",
                        binding.folder_path.trim_end_matches(['/', '\\']),
                        separator,
                        location.relative_path.replace(['/', '\\'], separator)
                    ))
                })
            };
            if let Some(path) = path {
                let entry = format!(
                    "#EXTINF:-1,{}\n{path}\n",
                    item.title.replace(['\r', '\n'], " ")
                );
                if contents.len().saturating_add(entry.len()) > super::transfer_codec::MAX_BYTES {
                    return Err(validation("Music export exceeds the 8 MB safety limit"));
                }
                contents.push_str(&entry);
            }
        }
    }
    super::transfer_codec::check_contents(&contents)?;
    Ok(contents)
}
