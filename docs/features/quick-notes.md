# Quick notes

Quick notes is an app-wide capture surface for temporary thoughts, reminders, and small pieces of information. It is deliberately separate from [Notes](notes/README.md).

A Quick note has no project membership, page hierarchy, blocks, icons, covers, comments, backlinks, file attachments other than images, version history, import/export, or collaboration state.

## Collection

The title-bar control opens a floating collection panel on desktop and mobile. The panel stays within the visible viewport and adapts to narrow windows without becoming a primary destination.

The collection supports All, ordered tags, search, Archive, and Trash. A note has at most one tag. Numeric shortcuts can select All and the first nine tag views when the user is not typing or interacting with a nested modal, and the workspace tab stepping shortcuts (Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageUp, Ctrl+PageDown) step through All and the tags, wrapping at the ends. While the panel is open, workspace navigation and view shortcuts behind it stay inactive.

A device can create tags only while fewer than nine exist. Merging changes from linked devices can leave more than nine, and they all stay reachable so the user can rename or delete them. Tag names are unique without regard to case. Each tag has an event-palette color. A new tag starts with a random color that no other tag uses yet, excluding the default note color so an unchanged tag stays distinguishable, and the user can change it while typing the name. Tags can be renamed, recolored, reordered, or, after confirmation, deleted. Deleting a tag keeps its notes and clears the tag from them, and a note saved with a tag that was deleted meanwhile is left untagged instead of failing.

The view row shows All first, then each tag chip in its color, with All in the default note color. Tags that do not fit collapse into a button that opens a list of every tag, so the row never scrolls sideways. Chips and list rows drag to reorder, and their menu also moves a tag one step, renames, recolors, or deletes it. All always stays first; its menu sets the default color for new untagged notes, which affects only notes created later and is stored in the vault configuration rather than synced.

The tag picker on a card or in the editor can also create a tag, which is assigned to the note right away. Assigning a tag sets the note's color to the tag's color, and a note created inside a tag view starts with that color. Clearing the tag keeps the note's color, and recoloring a tag does not recolor existing notes.

Active notes have durable manual order with separate pinned and unpinned groups. Moving a note changes only its own position between its new visible neighbors, so reordering inside a filtered tag view changes only the relative order of visible matching notes. Creating, pinning, unarchiving, and restoring place a note first in its group; content edits never move it. Concurrent reorders on linked devices converge to the same order everywhere. Search, Archive, and Trash use relevance or lifecycle order and cannot be manually reordered.

Cards use a responsive masonry layout. Mobile cards are compact, with two columns on phones, and leave note actions to the editor. Pointer users can drag from a valid card area. Touch users reorder with a long press on the card, so swipes keep scrolling. Cards have no separate drag handle. Keyboard movement and live announcements provide an equivalent reorder path. Cancellation and persistence failure restore canonical order.

Search uses a local rebuildable SQLite projection. Collection reads are bounded. Loading and view changes keep stable controls available and do not expose unpositioned cards as a false final layout.

## Editor

The editor has an optional title and one multiline body with bold, italic, and underline. Paste retains supported text and formatting while flattening or dropping links, lists, media, scripts, styles, and raw HTML.

Titles are limited to 200 characters and bodies to 65,536 characters. Empty new drafts are discarded.

The background uses a theme-aware event-palette slot. Notes store the slot identity, so theme changes recolor them without rewriting content. Text chooses the stronger black or white contrast against the resolved background.

Edits autosave after a short debounce, and pending writes flush at relevant lifecycle boundaries. Revision checks prevent another window or device from silently overwriting newer content. When the canonical note changed meanwhile, the editor merges its unsaved edits with it: fields only the editor changed keep the local value and fields only the other side changed take the canonical value. Only a field changed on both sides to different values stops autosave and offers to reload the canonical note or preserve local work as a separate copy. While the editor has nothing unsaved, it follows remote changes directly.

Revision-sensitive native writes return a stable `revision_conflict` code for stale revisions and `failed` for other failures. Recovery decisions use that code, never words in the diagnostic message.

## Images

A note holds up to 24 PNG, JPEG, WebP, or GIF images of at most 25 MiB each. Images are added with the editor's Add images button (the native file picker on desktop, the system picker on Android), by pasting copied images, or by dropping files on the editor. A paste that carries copied text along with a picture of it, as office suites produce, inserts the text instead. Adding an image the note already has is skipped, and files that are too large, unsupported, or unreadable are reported without blocking the rest.

Images sit above the title, flush with the top of the card and editor, as justified rows that keep each image's shape within bounds: panoramas and tall strips are cropped to a bounded tile, and a lone tall image fills the width at a bounded height. Cards show up to six images and count the rest on the last tile. A note with only images shows just the mosaic.

Selecting an image in the editor opens a viewer over the dimmed app: the image grows out of its tile, starts fitted to the window with its zoom shown as 100%, and returns to its tile on close. The viewer has zoom (wheel, pinch, double tap, click, and keys), panning, previous and next navigation with wrapping, swipe to change image, and swipe down or Escape to close. The viewer can copy the image to the clipboard, save a copy (a save dialog on desktop, shared Downloads on Android), and remove it from the note. Removal offers Undo in the editor. Read-only notes in Trash can view, copy, and save images but not change them.

Mosaics read scaled copies of large images and the viewer reads the original, both lazily and with bounded concurrency. Image files that no note, conflict version, or recovery entry references are deleted after a one-day grace period.

## Lifecycle

Active notes can be pinned, archived, or moved to Trash. Archive and Trash clear pin state. Trash remembers whether the note came from active or Archive so restore returns it appropriately.

Trashed notes are read-only and are permanently deleted after seven days. Expired notes are hidden at once and purged by a background job on a device that can write them. Manual permanent deletion and Empty Trash require confirmation.

## Linked devices

**Implemented in source; physical multi-device acceptance is pending.** Quick notes and their tags are editable on every linked device, online or offline, including devices that hold a read-only copy of the rest of the vault. Changes reach other devices on the same LAN through the coordinator desktop and converge to identical state everywhere. Protocol, writers, and delivery are owned by [Device linking and synchronization](../data/sync.md); merge rules are in the [sync algorithm](../algorithms/sync/README.md).

- **Silent merges:** concurrent changes to color, tag, lifecycle, and order resolve deterministically without asking the user. Two devices that create a tag with the same name end with one tag that keeps every assignment.
- **Conflicts:** concurrent edits of the same note's title, body, or image list to different values become a conflict. The card shows a conflict badge, and the editor shows a banner listing each version with its device and time: keep this keeps the displayed version, use this picks another version, and keep both keeps the displayed version and creates a new note from the other. Resolving on one device clears the conflict everywhere, and unsaved edits to other fields survive the resolution.
- **Recovery:** a note deleted on one device, including by the Trash purge, while another device edited it is not lost silently. The edit becomes a recovery entry in the Settings sync section, where Restore creates an active note from the retained values and Discard drops them. Either choice applies on every device.
- **Images:** image files follow the notes that reference them through the coordinator. A note can appear on another device before its images do; its tiles show a placeholder and load once the file arrives. Images added on a device that is offline upload on its next exchange.

## Data ownership

Quick notes, tags, formatted text runs, image references, lifecycle, order, color, revision, and derived search text are SQLite-canonical. Image bytes are content-addressed files under the vault's `assets/quick-notes/images/`, verified against their names on every read. Reordering is transactional and does not pretend content changed.

Quick notes creates no Markdown files and does not participate in Notes search, history, backlinks, exports, or collaboration.

## Accessibility

The panel and editor manage focus, close with Escape or Android Back as appropriate, and restore prior focus. DOM order remains canonical even when masonry changes visual placement. Reorder, tags, lifecycle, formatting, color, and close actions are keyboard reachable and have touch equivalents.

Reduced motion disables displacement animation. At recovery-size windows, close, save state, restore, and delete actions remain reachable.
