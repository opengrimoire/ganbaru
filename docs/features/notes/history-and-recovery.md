# Notes history and recovery

Status: implemented. Sync-aware restoration is planned.

Notes provides page-local history, project-wide versions, reusable page templates, Archive, Trash, and safety versions. These surfaces recover canonical data without turning history snapshots into a second live graph.

## Retention

History retention can be off or use a supported day window, with 30 days as the default. Projects may override the shared value. A long retention choice warns about local storage use.

Shortening retention or turning it off previews the number of versions and reclaimable bytes before confirmation. Turning history off stops new page snapshots and project checkpoints for that scope and prunes according to the confirmed policy.

## Page history

Mutations to page details, placement, lifecycle, child links, or blocks record a deduplicated pre-change snapshot when history is enabled. Rapid editor writes coalesce into a meaningful editing checkpoint rather than one version per debounced save.

Version history renders the selected snapshot without live editing controls and lists localized time and author. Users can copy blocks from a version or restore it.

Copy creates fresh block IDs and valid paired child pages where needed. Restore saves the current page first when history is enabled, then replaces page details and body through one transaction. It does not move the page to an old parent, which prevents a content restore from unexpectedly changing current navigation and access.

Copying a database block from page history creates an independent copy of its currently available database graph. A page snapshot does not contain historical database source and row graphs. Page-history restore therefore refuses pages or snapshots with local database content and directs users to project versions, preventing block replacement from deleting canonical database data.

## Project versions

A project version captures the complete project Notes closure: folders and placement, active, archived, and trashed pages, nested pages, blocks, databases, data sources, row properties, views, database templates, comments, suggestions, notifications, aliases, and managed asset references.

It excludes project identity, global preferences, undo stacks, collaboration operation history, and rebuildable projections.

Content-addressed immutable bundles share unchanged data between versions. Managed assets referenced by retained versions remain pinned. Dirty scheduling survives restarts, and no version is created merely by opening Notes.

Continuous editing creates bounded periodic and idle checkpoints. Destructive changes and imports create a complete safety version and checkpoint the resulting state. An unchanged manifest does not create a duplicate version.

## Project restore

Restore preview identifies pages that will be removed, recreated, changed, or copied. Confirmation creates a safety version and applies canonical changes in one transaction. Derived indexes rebuild only after that commit succeeds.

Pages created after the selected version leave the restored project but remain recoverable through the safety version. If a historical page now belongs to another project, the current page remains untouched and the historical graph is restored with fresh identities.

Collaboration operation sequence history is never rewound or deleted. Restore appends new operations describing restored comment and suggestion state. Historical pending notifications do not become newly deliverable.

**Planned for device synchronization:** restoration must create a safety version and new operations against the current replica, including text and structural changes. It cannot rewind causal history or peer acknowledgements. Tombstones and incompatible concurrent changes remain recoverable, and compaction must retain state required by enrolled offline devices. The current comment and suggestion operation log does not provide that general replication boundary. See [Synchronization](../../data/sync.md).

## Page templates

Page templates are global reusable Notes templates, distinct from database item templates. A template snapshots page properties, icon, cover, and root block tree.

Applying creates ordinary pages and blocks with fresh identities. Rename, update, duplicate, and delete affect only template records and never mutate pages that previously used the template.

Child-page template blocks create valid paired local child pages with safe default bodies. Deeper reusable child-page content requires explicit identity-aware support rather than copying broken links.

## Archive

Archive is reversible organization. Archived pages retain canonical content and history but leave active navigation, favorites, recents, and active search. Restore validates placement and safely promotes a page when its old parent is unavailable.

## Trash and permanent deletion

Trash is the recovery surface for deleted pages. Page subtrees move together, paired child blocks hide or restore consistently, and retained history and asset pins remain valid.

Trashing a database block, containing block subtree, or containing note also trashes its owned database shells, sources, row pages, and nested row content in the same transaction. Trashing a linked shell leaves the shared source graph intact. The deletion records which active objects it changed. Restore recovers only objects still owned by that deletion, preserving rows and blocks individually trashed before it, as well as objects restored and deleted again afterward. Restoring an individual row or nested note uses the same ownership boundary and validates its placement.

Permanent deletion requires confirmation or retention expiry. It removes canonical page closure transactionally. Asset bytes are collected only when no live record, history version, import record, comment, property, or other owner references them.

## Undo and history distinction

Editor undo is short-lived local operation recovery. Page history recovers one page across sessions. Project versions recover a project graph. Archive and Trash recover lifecycle state. Each surface explains its scope and never implies it can restore data owned by another layer.
