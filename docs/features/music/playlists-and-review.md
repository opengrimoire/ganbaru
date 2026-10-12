# Music playlists and review

## Playlist identity

A playlist has a stable identity, a localized or user-authored name, an icon or emoji, a playback order mode, a repeat mode, and ordered memberships.

Fresh vaults provide starter playlists for focus, breaks, meditation, exercise, hygiene, chores, cooking, and commute. Built-in playlists have localized names and icons and cannot be renamed or deleted, but their membership and order are the user's. Custom playlists can be created, renamed, reordered, and deleted. Deleting a playlist never deletes library items or media files.

## Membership settings

A membership connects one canonical media item to one playlist and can define:

- Enabled state and stored order.
- Mix frequency (ignored by In order and Shuffle).
- Start and end positions.
- Skip ranges.
- Volume and playback rate overrides.
- Snooze, scoped to one playlist or everywhere.

These settings belong to the membership, not to Calendar assignments, so the same item can behave differently in different playlists without duplicating its identity. Stored order and the selected view sort are distinct: sorting a list never rewrites membership order.

## Snooze

Snooze temporarily excludes an item from one playlist or all playlists until a boundary (1 day, 1 week, or 1 month). It is reversible, shows its scope and end, and does not change review state or source availability. Snoozed rows show a clock that removes the snooze without playing the row; a playlist marks only snoozes effective in that playlist. Player-side controls are described in [Playback](playback.md#track-preferences).

## Review workflow

Newly discovered items enter review, which shows source hierarchy, metadata, artwork, availability, playlist choices, and repair actions. The user never has to finish the queue before using Music, and a playlist stays usable while review is incomplete.

Review state is unreviewed, reviewed, deferred, or ignored. Deferring or ignoring never deletes the item or its source. Ignored items are hidden by default, can be revealed, and can be returned to unreviewed individually or in bulk. The Review badge counts unique unreviewed canonical items, not per-source totals, because one item can belong to several collections.

The builder shows a complete projection rather than a progressively filling list, and keeps its workspace, active item, and drafts while the app shell is alive.

Opening the builder does not interrupt current playback unless Review autoplay is on. Review playback temporarily suspends the prior player source, queue, position, and play state, and returning to the player restores it. Explicit playback of another playlist, or Calendar or Focus automation, supersedes the suspended state instead of restoring it. YouTube durations learned during playback are stored and used immediately.

## Playlist management

Built-in and custom playlists share one stored order. Reordering starts from an explicit handle and works with pointer, touch, and keyboard; a failed save restores the prior order and reports the error. Unavailable built-in actions stay visible with an explanation.

## Sources explorer

Sources is a library explorer, not a configuration dashboard. It has two top-level roots, Local music and YouTube, with user-editable names. Selecting a root, collection, or folder shows its children and tracks; a side panel keeps the full hierarchy.

- Local music mirrors each connected folder's structure without copying or changing files. A fresh vault tries to connect the operating system Music folder. More folders are added inside Local music.
- YouTube has one link entry point for videos and playlists. Playlists keep their own collection; single videos go to Saved videos. Link previews are playable, and names resolve through YouTube's keyless metadata surface when possible. One failed video never blocks a playlist, and refreshes keep metadata already resolved.

Activating a track starts a normal player queue for that source branch. Refresh, rename, repair, and removal are contextual to the selected root or collection. Removal names its scope and consequence explicitly, and disconnecting one device path is distinct from removing a source or unused tracks. Source health appears as an attention marker, with detailed repair in its own workflow.

## Accessibility

Review, playlist navigation, membership selection, reordering, filters, item menus, and playback are keyboard reachable. Drag operations have keyboard equivalents with live announcements. Responsive layouts keep destination labels instead of unlabeled icons. Blocking builder workflows (setup, edit, delete, repair, relink, refresh, import, export) use the shared app-level dialog with focus trapping, focus restoration, and Escape dismissal when safe.
