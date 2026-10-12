# Notes pages and navigation

## Page model

A page has a stable identity, title, parent, optional project, icon, cover, properties, archive state, Trash state, timestamps, and optional import provenance. Its ordered block tree is stored separately and loaded in bounded ranges. Parents can be the workspace, a page, a data source, or other supported local contexts; database row pages are ordinary pages parented by a data source.

An empty title is valid and receives a localized fallback in navigation. Renaming a page updates navigation and link presentation without changing stable link identity, and title drafts and committed renames or moves update the header and sidebar immediately.

Opening a page uses one native page-open read for breadcrumbs, the initial block range, and its outline; optional panels and descendant hydration do not delay editing. Pending edits flush before leaving a page, and previous documents are released rather than cached. A brief skeleton appears only for slow loads, and a failed read shows its error with Retry while navigation stays available. New pages appear in navigation and the editor while their creation is still committing.

## Folders

Folders organize workspace-parented pages inside a project. They are navigation containers, not page blocks or Markdown directories, and moving a page into one changes placement only. Moving a folder or page rejects self-parenting, descendant cycles, inactive destinations, and destinations outside the effective project scope.

## Child pages and previews

A child page has both a normal page row and a paired `child_page` block in its parent document, sharing stable identity. Creation, rename, move, Trash, restore, and permanent deletion keep the pair synchronized. Moving a page under another page creates or restores the paired block at the destination; moving it back to the workspace hides the old block rather than leaving a broken reference. If a restored page's parent is missing, archived, or trashed, the page moves to a safe project root and the UI explains the placement.

Child notes open in a side or center preview with its own editor session (document, hydration, drafts, save queue, and undo history) while the main editor stays mounted. Both panes accept input in side view; a center preview blocks the main pane. Closing a preview saves its edits and returns to the main editor without reloading it or resetting its scroll position, and a save failure keeps the preview open. Promoting a preview to a full page keeps its editor. Small viewports show the preview full size.

## Breadcrumbs and hierarchy

With the sidebar collapsed, the workspace header shows the active note's ancestry: group, project, folders, and the path to the note. Sibling notes never appear as path segments, and in side view the path follows the focused pane. Opening a database extends the path through its containing note, and that segment returns to the retained editor and scroll position.

Breadcrumb segments open hierarchy pickers that list child notes and databases, including databases inside nested blocks. Database rows in these pickers expose saved views, which are presentation choices rather than navigation levels; selecting one opens the database with that view. Only the panel opened directly from a segment has a search field. Touch navigation uses equivalent drilldown controls.

## Project scope

Pages created from a project home, project-scoped picker, folder, or top-bar action belong to the selected project. Navigation, search, destination pickers, and flyouts show only content the current project and access policy permit. Opening a note owned by another project switches the active project and group to match, without reopening the note.

The sidebar shows folders and workspace-parented notes; child notes do not become nested sidebar rows, and the containing root note stays highlighted when a child is selected. Database row pages stay out of project roots because their database view owns their navigation, but they still participate in search, links, history, and direct opens.

## Favorites and recents

Favorites and recents are device-local navigation metadata outside the canonical page graph. Favoriting pins a page in navigation; opening or creating a page updates recents. Neither changes parent, project, order, content, or history.

When a page is open, its last-edited detail (with an activity panel), favorite control, and page actions sit in the workspace header; the editor has no separate page header. Previews keep their close and view-mode controls as compact overlays.

## Archive and Trash

Archive hides a page from active navigation, favorites, recents, search, and parent selection without deleting anything. The Archive view supports search and restore, and restore validates the stored parent.

Trash applies to the reachable page subtree so active children never remain under an inactive parent. Trashed pages leave navigation and Archive immediately and stay in Trash for the configured recovery period. If the move fails while the page is still active, it returns with an error and keeps pending edits. Restore preserves valid structure and reactivates paired child-page blocks. Permanent deletion requires confirmation or retention expiry and removes the page subtree, paired blocks, and owned content transactionally; asset bytes are collected through the ownership graph and history pins, not because one reference disappeared. See [History and recovery](history-and-recovery.md).

## Search and destination reads

Navigation, Archive, Trash, destination, and search surfaces use page summaries and bounded pagination rather than full page content. Search can match titles, aliases, blocks, database properties, comments, links, and managed file metadata according to access. Opening a block or comment result focuses the target when it still exists; a stale or inaccessible target fails safely instead of opening a page with a similar title.

## Working-folder Markdown

Authorized project working folders appear as source-aware roots when they contain Markdown files. Scanning is recursive and bounded, rejects symbolic links and unsafe paths, skips common generated and dependency directories, and reports truncated results. Unavailable folders and scan errors stay visible.

Opening a file provides raw editing, sanitized preview, dirty state, explicit Save, Refresh, Open externally, and Copy relative path. Reads include a revision digest, and Save requires the expected digest and replaces the file atomically. If another tool changed the file while local edits are dirty, Notes refuses to overwrite it and offers Reload, Copy local text, or Open externally. Notes does not create, rename, move, or delete working-folder files.

Android omits working-folder Markdown because local project folders and desktop path authority are unavailable there.

## Shared Notes and Chat sidebar design

Notes and Chat sidebars share one visual system: theme-derived tint, expanded width, toolbar and row heights, typography, icon size and stroke, hover and selection treatment, row action buttons, action menus, and search component. Sidebar metrics have one shared CSS owner so the two features cannot drift. Each feature keeps its own actions, such as Notes sorting and hierarchy controls and Chat channel sections and archive.

- On desktop, the selected note or channel appears in the workspace header only while its sidebar is collapsed; mobile headers always show it because navigation is a separate surface.
- Row action buttons appear on pointer hover or keyboard focus and are always visible on touch layouts. Truncated names show a full-name tooltip.
- List content width stays stable as the list grows, and the expanded or collapsed state survives switching application tabs.
- Notes search opens below the toolbar and takes focus; Chat search stays in the toolbar without taking focus.
