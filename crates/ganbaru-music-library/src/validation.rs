use super::models::*;
use ganbaru_music::error::{MusicLibraryError, MusicLibraryResult};
use std::collections::HashSet;
#[cfg(desktop)]
use std::path::{Component, Path};

pub(crate) const MAX_BULK_MEMBERSHIPS: usize = 500;
pub(crate) const MAX_REVIEW_SELECTION_ITEMS: usize = 20_000;
const MAX_ID_BYTES: usize = 200;
const MAX_NAME_CHARS: usize = 200;
const MAX_ICON_CHARS: usize = 500;
const MAX_REASON_CHARS: usize = 500;
#[cfg(desktop)]
const MAX_RELATIVE_PATH_BYTES: usize = 4_096;
pub(crate) const MAX_ITEM_WINDOW: i64 = 200;
pub(crate) const MAX_SUMMARY_WINDOW: i64 = 500;
const MAX_SEARCH_CHARS: usize = 300;

pub(crate) fn validate_id(value: &str, field: &str) -> MusicLibraryResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(MusicLibraryError::validation(field, "is required"));
    }
    if trimmed.len() > MAX_ID_BYTES {
        return Err(MusicLibraryError::validation(
            field,
            format!("exceeds the {MAX_ID_BYTES} byte limit"),
        ));
    }
    Ok(())
}

fn validate_timestamp(value: i64, field: &str) -> MusicLibraryResult<()> {
    if value <= 0 {
        return Err(MusicLibraryError::validation(
            field,
            "must be a positive Unix epoch millisecond value",
        ));
    }
    Ok(())
}

fn validate_optional_text(value: &str, field: &str, max_chars: usize) -> MusicLibraryResult<()> {
    if value.chars().count() > max_chars {
        return Err(MusicLibraryError::validation(
            field,
            format!("exceeds the {max_chars} character limit"),
        ));
    }
    Ok(())
}

fn validate_name(value: &str) -> MusicLibraryResult<()> {
    if value.trim().is_empty() {
        return Err(MusicLibraryError::validation("name", "is required"));
    }
    validate_optional_text(value, "name", MAX_NAME_CHARS)
}

#[cfg(desktop)]
fn validate_relative_path(value: &str) -> MusicLibraryResult<()> {
    if value.trim().is_empty() {
        return Err(MusicLibraryError::validation("relativePath", "is required"));
    }
    if value.len() > MAX_RELATIVE_PATH_BYTES || value.contains('\0') {
        return Err(MusicLibraryError::validation(
            "relativePath",
            "is too long or contains a null byte",
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(MusicLibraryError::validation(
            "relativePath",
            "must stay within its logical music root",
        ));
    }
    Ok(())
}

pub(crate) fn validate_collection_write(
    collection: &MusicCollectionWrite,
) -> MusicLibraryResult<()> {
    validate_id(&collection.id, "id")?;
    validate_id(&collection.identity_key, "identityKey")?;
    validate_name(&collection.name)?;
    validate_timestamp(collection.updated_at_ms, "updatedAt")?;
    match collection.kind {
        MusicCollectionKind::LocalRoot
            if collection.local_root_id.is_none() || collection.youtube_playlist_id.is_some() =>
        {
            Err(MusicLibraryError::validation(
                "localRootId",
                "a local collection requires only a logical root id",
            ))
        }
        MusicCollectionKind::YouTubePlaylist
            if collection.youtube_playlist_id.is_none() || collection.local_root_id.is_some() =>
        {
            Err(MusicLibraryError::validation(
                "youtubePlaylistId",
                "a YouTube collection requires only a playlist id",
            ))
        }
        _ => Ok(()),
    }
}

pub(crate) fn validate_local_root_create(root: &MusicLocalRootCreate) -> MusicLibraryResult<()> {
    validate_id(&root.root_id, "rootId")?;
    validate_id(&root.collection_id, "collectionId")?;
    validate_id(&root.identity_key, "identityKey")?;
    validate_name(&root.name)?;
    validate_timestamp(root.created_at_ms, "createdAt")
}

#[cfg(desktop)]
pub(crate) fn validate_item_repair_apply(request: &MusicItemRepairApply) -> MusicLibraryResult<()> {
    validate_id(&request.item_id, "itemId")?;
    validate_id(&request.root_id, "rootId")?;
    validate_id(&request.location_id, "locationId")?;
    validate_name(&request.root_name)?;
    validate_id(
        &request.expected_strong_fingerprint,
        "expectedStrongFingerprint",
    )?;
    validate_relative_path(&request.relative_path)?;
    validate_timestamp(request.applied_at, "appliedAt")?;
    if request.folder_path.trim().is_empty() || !Path::new(&request.folder_path).is_absolute() {
        return Err(MusicLibraryError::validation(
            "folderPath",
            "must be an absolute folder path",
        ));
    }
    Ok(())
}

pub(crate) fn validate_playlist_create(playlist: &MusicPlaylistCreate) -> MusicLibraryResult<()> {
    validate_id(&playlist.id, "id")?;
    validate_name(&playlist.name)?;
    validate_icon(&playlist.icon)?;
    validate_playlist_playback_mode(playlist.shuffle_enabled, playlist.mix_enabled)?;
    validate_timestamp(playlist.created_at_ms, "createdAt")?;
    validate_unique_intended_uses(&playlist.intended_uses)
}

pub(crate) fn validate_playlist_update(playlist: &MusicPlaylistUpdate) -> MusicLibraryResult<()> {
    validate_id(&playlist.id, "id")?;
    validate_name(&playlist.name)?;
    validate_icon(&playlist.icon)?;
    validate_playlist_playback_mode(playlist.shuffle_enabled, playlist.mix_enabled)?;
    if playlist.expected_version <= 0 {
        return Err(MusicLibraryError::validation(
            "expectedVersion",
            "must be greater than zero",
        ));
    }
    validate_timestamp(playlist.updated_at_ms, "updatedAt")?;
    validate_unique_intended_uses(&playlist.intended_uses)
}

pub(crate) fn validate_playlist_playback_mode(
    shuffle_enabled: bool,
    mix_enabled: bool,
) -> MusicLibraryResult<()> {
    if mix_enabled && !shuffle_enabled {
        return Err(MusicLibraryError::validation(
            "mixEnabled",
            "Mix requires randomized playback",
        ));
    }
    Ok(())
}

pub(crate) fn validate_icon(icon: &str) -> MusicLibraryResult<()> {
    validate_optional_text(icon, "icon", MAX_ICON_CHARS)?;
    ganbaru_projects::validation::validate_project_icon(icon)
        .map_err(|message| MusicLibraryError::validation("icon", message))
}

fn validate_unique_intended_uses(uses: &[MusicIntendedUse]) -> MusicLibraryResult<()> {
    let unique = uses.iter().copied().collect::<HashSet<_>>();
    if unique.len() != uses.len() {
        return Err(MusicLibraryError::validation(
            "intendedUses",
            "must not contain duplicates",
        ));
    }
    Ok(())
}

pub(crate) fn validate_membership_write(
    membership: &MusicMembershipWrite,
) -> MusicLibraryResult<()> {
    validate_id(&membership.id, "id")?;
    validate_id(&membership.playlist_id, "playlistId")?;
    validate_id(&membership.item_id, "itemId")?;
    validate_timestamp(membership.updated_at_ms, "updatedAt")?;
    if membership.position < 0 {
        return Err(MusicLibraryError::validation(
            "position",
            "must be zero or greater",
        ));
    }
    if membership.start_ms.is_some_and(|value| value < 0)
        || membership.end_ms.is_some_and(|value| value < 0)
        || matches!((membership.start_ms, membership.end_ms), (Some(start), Some(end)) if end < start)
    {
        return Err(MusicLibraryError::validation(
            "endMs",
            "must not be earlier than the non-negative start",
        ));
    }
    if membership
        .volume
        .is_some_and(|volume| !(0.0..=1.0).contains(&volume))
    {
        return Err(MusicLibraryError::validation(
            "volume",
            "must be between 0 and 1",
        ));
    }
    if membership
        .rate
        .is_some_and(|rate| !(0.25..=2.0).contains(&rate))
    {
        return Err(MusicLibraryError::validation(
            "rate",
            "must be between 0.25 and 2",
        ));
    }
    if membership
        .expected_version
        .is_some_and(|version| version <= 0)
    {
        return Err(MusicLibraryError::validation(
            "expectedVersion",
            "must be greater than zero",
        ));
    }
    Ok(())
}

pub(crate) fn validate_bulk_membership_write(
    request: &MusicBulkMembershipWrite,
) -> MusicLibraryResult<()> {
    if request.memberships.is_empty() {
        return Err(MusicLibraryError::validation(
            "memberships",
            "must contain at least one membership",
        ));
    }
    if request.memberships.len() > MAX_BULK_MEMBERSHIPS {
        return Err(MusicLibraryError::validation(
            "memberships",
            format!("exceeds the {MAX_BULK_MEMBERSHIPS} item limit"),
        ));
    }
    let mut identities = HashSet::with_capacity(request.memberships.len());
    for membership in &request.memberships {
        validate_membership_write(membership)?;
        if !identities.insert((&membership.playlist_id, &membership.item_id)) {
            return Err(MusicLibraryError::validation(
                "memberships",
                format!(
                    "contains duplicate item '{}' for playlist '{}'",
                    membership.item_id, membership.playlist_id
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_bulk_membership_edit(
    request: &MusicBulkMembershipEdit,
) -> MusicLibraryResult<()> {
    validate_id(&request.action_id, "actionId")?;
    validate_bounded_unique_ids(&request.item_ids, "itemIds")?;
    if request.add_playlist_ids.is_empty()
        && request.remove_playlist_ids.is_empty()
        && request.weight_playlist_ids.is_empty()
    {
        return Err(MusicLibraryError::validation(
            "playlistIds",
            "must contain at least one requested edit",
        ));
    }
    if !request.add_playlist_ids.is_empty() {
        validate_bounded_unique_ids(&request.add_playlist_ids, "addPlaylistIds")?;
    }
    if !request.remove_playlist_ids.is_empty() {
        validate_bounded_unique_ids(&request.remove_playlist_ids, "removePlaylistIds")?;
    }
    if !request.weight_playlist_ids.is_empty() {
        validate_bounded_unique_ids(&request.weight_playlist_ids, "weightPlaylistIds")?;
    }
    if request
        .add_playlist_ids
        .iter()
        .any(|playlist_id| request.remove_playlist_ids.contains(playlist_id))
    {
        return Err(MusicLibraryError::validation(
            "playlistIds",
            "cannot add and remove the same playlist",
        ));
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")?;
    match (
        request.weight_playlist_ids.is_empty(),
        request.weight.is_some(),
    ) {
        (false, false) => {
            return Err(MusicLibraryError::validation(
                "weight",
                "is required when setting weight",
            ));
        }
        (true, true) => {
            return Err(MusicLibraryError::validation(
                "weight",
                "must be empty when no playlist weight is changing",
            ));
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn validate_playlist_reorder(request: &MusicPlaylistReorder) -> MusicLibraryResult<()> {
    validate_id(&request.playlist_id, "playlistId")?;
    validate_id(&request.item_id, "itemId")?;
    if request.target_index < 0 {
        return Err(MusicLibraryError::validation(
            "targetIndex",
            "must be zero or greater",
        ));
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")
}

pub(crate) fn validate_playlists_reorder(
    request: &MusicPlaylistsReorder,
) -> MusicLibraryResult<()> {
    if request.playlists.is_empty() || request.playlists.len() > MAX_SUMMARY_WINDOW as usize {
        return Err(MusicLibraryError::validation(
            "playlists",
            format!("must contain between 1 and {MAX_SUMMARY_WINDOW} playlists"),
        ));
    }
    let mut ids = HashSet::with_capacity(request.playlists.len());
    for playlist in &request.playlists {
        validate_id(&playlist.playlist_id, "playlistId")?;
        if playlist.expected_version <= 0 {
            return Err(MusicLibraryError::validation(
                "expectedVersion",
                "must be greater than zero",
            ));
        }
        if !ids.insert(playlist.playlist_id.as_str()) {
            return Err(MusicLibraryError::validation(
                "playlists",
                "must not contain duplicate playlist ids",
            ));
        }
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")
}

pub(crate) fn validate_bulk_review_write(request: &MusicBulkReviewWrite) -> MusicLibraryResult<()> {
    if request.items.is_empty() || request.items.len() > MAX_BULK_MEMBERSHIPS {
        return Err(MusicLibraryError::validation(
            "items",
            format!("must contain between 1 and {MAX_BULK_MEMBERSHIPS} items"),
        ));
    }
    let mut ids = HashSet::with_capacity(request.items.len());
    for item in &request.items {
        validate_id(&item.item_id, "itemId")?;
        if item.expected_version <= 0 {
            return Err(MusicLibraryError::validation(
                "expectedVersion",
                "must be positive",
            ));
        }
        if !ids.insert(&item.item_id) {
            return Err(MusicLibraryError::validation(
                "items",
                "contains duplicate item ids",
            ));
        }
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")?;
    if request.review_state != MusicReviewState::Deferred && request.deferred_until.is_some() {
        return Err(MusicLibraryError::validation(
            "deferredUntil",
            "must be empty unless the review state is deferred",
        ));
    }
    Ok(())
}

pub(crate) fn validate_review_selection_write(
    request: &MusicReviewSelectionWrite,
) -> MusicLibraryResult<()> {
    validate_id(&request.action_id, "actionId")?;
    validate_review_selection_items(&request.items, "items")?;
    if !matches!(
        request.review_state,
        MusicReviewState::Reviewed | MusicReviewState::Ignored
    ) {
        return Err(MusicLibraryError::validation(
            "reviewState",
            "must be reviewed or ignored",
        ));
    }
    if request.review_state == MusicReviewState::Ignored
        && (!request.add_playlist_ids.is_empty() || !request.remove_playlist_ids.is_empty())
    {
        return Err(MusicLibraryError::validation(
            "playlistIds",
            "must be empty when ignoring a review selection",
        ));
    }
    if !request.add_playlist_ids.is_empty() {
        validate_bounded_unique_ids(&request.add_playlist_ids, "addPlaylistIds")?;
    }
    if !request.remove_playlist_ids.is_empty() {
        validate_bounded_unique_ids(&request.remove_playlist_ids, "removePlaylistIds")?;
    }
    if request
        .add_playlist_ids
        .iter()
        .any(|playlist_id| request.remove_playlist_ids.contains(playlist_id))
    {
        return Err(MusicLibraryError::validation(
            "playlistIds",
            "cannot add and remove the same playlist",
        ));
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")
}

pub(crate) fn validate_bulk_snooze_write(request: &MusicBulkSnoozeWrite) -> MusicLibraryResult<()> {
    validate_id(&request.action_id, "actionId")?;
    validate_bounded_unique_ids(&request.item_ids, "itemIds")?;
    validate_timestamp(request.starts_at_ms, "startsAt")?;
    validate_timestamp(request.created_at_ms, "createdAt")?;
    validate_optional_text(&request.reason, "reason", MAX_REASON_CHARS)?;
    if request
        .ends_at_ms
        .is_some_and(|end| end <= request.starts_at_ms)
    {
        return Err(MusicLibraryError::validation(
            "endsAt",
            "must be later than startsAt",
        ));
    }
    match request.scope {
        MusicSnoozeScope::Playlist if request.playlist_id.is_none() => Err(
            MusicLibraryError::validation("playlistId", "is required for playlist scope"),
        ),
        MusicSnoozeScope::AllPlaylists if request.playlist_id.is_some() => Err(
            MusicLibraryError::validation("playlistId", "must be empty for all-playlists scope"),
        ),
        _ => Ok(()),
    }
}

pub(crate) fn validate_playlist_duplicate(
    request: &MusicPlaylistDuplicate,
) -> MusicLibraryResult<()> {
    validate_id(&request.source_playlist_id, "sourcePlaylistId")?;
    validate_id(&request.new_playlist_id, "newPlaylistId")?;
    if request.source_playlist_id == request.new_playlist_id {
        return Err(MusicLibraryError::validation(
            "newPlaylistId",
            "must differ from sourcePlaylistId",
        ));
    }
    validate_name(&request.name)?;
    validate_timestamp(request.created_at_ms, "createdAt")
}

pub(crate) fn validate_playlist_delete(request: &MusicPlaylistDelete) -> MusicLibraryResult<()> {
    validate_id(&request.playlist_id, "playlistId")?;
    if let Some(replacement_id) = &request.replacement_playlist_id {
        validate_id(replacement_id, "replacementPlaylistId")?;
        if replacement_id == &request.playlist_id {
            return Err(MusicLibraryError::validation(
                "replacementPlaylistId",
                "must differ from the deleted playlist",
            ));
        }
    }
    if request.expected_version <= 0 {
        return Err(MusicLibraryError::validation(
            "expectedVersion",
            "must be greater than zero",
        ));
    }
    for (field, value) in [
        ("membershipCount", request.expected_impact.membership_count),
        (
            "projectFocusAssignmentCount",
            request.expected_impact.project_focus_assignment_count,
        ),
        (
            "projectBreakAssignmentCount",
            request.expected_impact.project_break_assignment_count,
        ),
        (
            "calendarAssignmentCount",
            request.expected_impact.calendar_assignment_count,
        ),
        (
            "contextAssignmentCount",
            request.expected_impact.context_assignment_count,
        ),
    ] {
        if value < 0 {
            return Err(MusicLibraryError::validation(
                field,
                "must be zero or greater",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_review_write(request: &MusicReviewWrite) -> MusicLibraryResult<()> {
    validate_id(&request.item_id, "itemId")?;
    if request.expected_version <= 0 {
        return Err(MusicLibraryError::validation(
            "expectedVersion",
            "must be greater than zero",
        ));
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")?;
    if request.review_state == MusicReviewState::Deferred {
        if let Some(deferred_until) = request.deferred_until {
            validate_timestamp(deferred_until, "deferredUntil")?;
            if deferred_until <= request.updated_at_ms {
                return Err(MusicLibraryError::validation(
                    "deferredUntil",
                    "must be later than updatedAt",
                ));
            }
        }
    } else if request.deferred_until.is_some() {
        return Err(MusicLibraryError::validation(
            "deferredUntil",
            "must be null unless reviewState is deferred",
        ));
    }
    Ok(())
}

pub(crate) fn validate_metadata_override_write(
    request: &MusicMetadataOverrideWrite,
) -> MusicLibraryResult<()> {
    validate_id(&request.item_id, "itemId")?;
    if request.expected_version <= 0 {
        return Err(MusicLibraryError::validation(
            "expectedVersion",
            "must be greater than zero",
        ));
    }
    for (field, value, maximum) in [
        ("titleOverride", &request.title_override, 500_usize),
        ("artistOverride", &request.artist_override, 500_usize),
        ("albumOverride", &request.album_override, 500_usize),
        ("artworkOverride", &request.artwork_override, 2_048_usize),
    ] {
        if let Some(value) = value {
            if value.trim().is_empty() || value.len() > maximum {
                return Err(MusicLibraryError::validation(
                    field,
                    format!("must be non-empty and at most {maximum} bytes"),
                ));
            }
        }
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")
}

pub(crate) fn validate_item_signals_write(
    request: &MusicItemSignalsWrite,
) -> MusicLibraryResult<()> {
    validate_bounded_unique_ids(&request.item_ids, "itemIds")?;
    if request.signals.len() > 6 {
        return Err(MusicLibraryError::validation(
            "signals",
            "must contain at most six values",
        ));
    }
    let mut unique = HashSet::with_capacity(request.signals.len());
    if request.signals.iter().any(|signal| !unique.insert(signal)) {
        return Err(MusicLibraryError::validation(
            "signals",
            "must not contain duplicate values",
        ));
    }
    validate_timestamp(request.updated_at_ms, "updatedAt")
}

pub(crate) fn validate_membership_remove(
    request: &MusicMembershipRemove,
) -> MusicLibraryResult<()> {
    validate_bounded_unique_ids(&request.membership_ids, "membershipIds")
}

pub(crate) fn validate_advanced_membership_write(
    request: &MusicAdvancedMembershipWrite,
) -> MusicLibraryResult<()> {
    validate_membership_write(&request.membership)?;
    let mut previous_end = None;
    let mut ids = HashSet::new();
    for (index, range) in request.skip_ranges.iter().enumerate() {
        validate_id(&range.id, "skipRanges.id")?;
        if range.membership_id != request.membership.id {
            return Err(MusicLibraryError::validation(
                "skipRanges.membershipId",
                "must match membership.id",
            ));
        }
        if !ids.insert(&range.id) || range.start_ms < 0 || range.end_ms <= range.start_ms {
            return Err(MusicLibraryError::validation(
                "skipRanges",
                "must contain unique ranges with endMs greater than startMs",
            ));
        }
        if range.sort_order != index as i64 || previous_end.is_some_and(|end| range.start_ms < end)
        {
            return Err(MusicLibraryError::validation(
                "skipRanges",
                "must be ordered, contiguous in sort order, and non-overlapping",
            ));
        }
        previous_end = Some(range.end_ms);
    }
    Ok(())
}

pub(crate) fn validate_snooze_remove(request: &MusicSnoozeRemove) -> MusicLibraryResult<()> {
    validate_id(&request.snooze_id, "snoozeId")
}

pub(crate) fn validate_bounded_unique_ids(
    values: &[String],
    field: &str,
) -> MusicLibraryResult<()> {
    if values.is_empty() {
        return Err(MusicLibraryError::validation(
            field,
            "must contain at least one id",
        ));
    }
    if values.len() > MAX_BULK_MEMBERSHIPS {
        return Err(MusicLibraryError::validation(
            field,
            format!("exceeds the {MAX_BULK_MEMBERSHIPS} item limit"),
        ));
    }
    let mut unique = HashSet::with_capacity(values.len());
    for value in values {
        validate_id(value, field)?;
        if !unique.insert(value) {
            return Err(MusicLibraryError::validation(
                field,
                format!("contains duplicate id '{value}'"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_review_selection_ids(
    values: &[String],
    field: &str,
) -> MusicLibraryResult<()> {
    if values.is_empty() {
        return Err(MusicLibraryError::validation(
            field,
            "must contain at least one id",
        ));
    }
    if values.len() > MAX_REVIEW_SELECTION_ITEMS {
        return Err(MusicLibraryError::validation(
            field,
            format!("exceeds the {MAX_REVIEW_SELECTION_ITEMS} item limit"),
        ));
    }
    let mut unique = HashSet::with_capacity(values.len());
    for value in values {
        validate_id(value, field)?;
        if !unique.insert(value) {
            return Err(MusicLibraryError::validation(
                field,
                format!("contains duplicate id '{value}'"),
            ));
        }
    }
    Ok(())
}

fn validate_review_selection_items(
    items: &[MusicVersionedItem],
    field: &str,
) -> MusicLibraryResult<()> {
    let ids = items
        .iter()
        .map(|item| item.item_id.clone())
        .collect::<Vec<_>>();
    validate_review_selection_ids(&ids, field)?;
    for item in items {
        if item.expected_version <= 0 {
            return Err(MusicLibraryError::validation(
                "expectedVersion",
                "must be positive",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_item_window(request: &MusicItemWindowRequest) -> MusicLibraryResult<()> {
    if request.offset < 0 {
        return Err(MusicLibraryError::validation(
            "offset",
            "must be zero or greater",
        ));
    }
    if !(1..=MAX_ITEM_WINDOW).contains(&request.limit) {
        return Err(MusicLibraryError::validation(
            "limit",
            format!("must be between 1 and {MAX_ITEM_WINDOW}"),
        ));
    }
    if request.search.chars().count() > MAX_SEARCH_CHARS {
        return Err(MusicLibraryError::validation(
            "search",
            format!("exceeds the {MAX_SEARCH_CHARS} character limit"),
        ));
    }
    if request.now_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "nowMs",
            "must be a positive Unix epoch millisecond value",
        ));
    }
    match request.destination {
        MusicListDestination::Playlist if request.playlist_id.is_none() => Err(
            MusicLibraryError::validation("playlistId", "is required for a playlist window"),
        ),
        MusicListDestination::Review | MusicListDestination::Library
            if request.playlist_id.is_some() =>
        {
            Err(MusicLibraryError::validation(
                "playlistId",
                "must be empty outside a playlist window",
            ))
        }
        _ => {
            if let Some(playlist_id) = &request.playlist_id {
                validate_id(playlist_id, "playlistId")?;
            }
            if let Some(collection_id) = &request.source_collection_id {
                validate_id(collection_id, "sourceCollectionId")?;
            }
            if let Some(playlist_id) = &request.membership_playlist_id {
                validate_id(playlist_id, "membershipPlaylistId")?;
            }
            if matches!(
                request.sort,
                MusicItemSort::ManualPosition | MusicItemSort::AddedToPlaylist
            ) && request.destination != MusicListDestination::Playlist
            {
                return Err(MusicLibraryError::validation(
                    "sort",
                    "this sort is available only inside a playlist",
                ));
            }
            Ok(())
        }
    }
}

pub(crate) fn validate_summary_window(offset: i64, limit: i64) -> MusicLibraryResult<()> {
    if offset < 0 {
        return Err(MusicLibraryError::validation(
            "offset",
            "must be zero or greater",
        ));
    }
    if !(1..=MAX_SUMMARY_WINDOW).contains(&limit) {
        return Err(MusicLibraryError::validation(
            "limit",
            format!("must be between 1 and {MAX_SUMMARY_WINDOW}"),
        ));
    }
    Ok(())
}
