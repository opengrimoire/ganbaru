# Notes databases

Status: implemented with bounded action limits. Real desktop and Android visual and touch acceptance remains pending.

Local databases are structured views over Notes row pages. They follow useful public Notion concepts while remaining an independent local SQLite implementation. Hosted permissions, arbitrary automation, and AI autofill would need separate product contracts and are not exposed as inactive menu actions.

## Design direction

Database controls use progressive disclosure: rows and their values are the primary content, and controls stay compact. A view bar holds view tabs, compact utilities, and a split New button; an applied query strip shows active filters and sorts; property and row menus act on the item that opened them; settings show current values in concise rows and open detail pages only when needed. Colors communicate status and active query state rather than decorate every action. Touch layouts get larger targets without imposing that spacing on pointer layouts.

This follows Notion's documented [views, filters, and sorts](https://www.notion.com/help/views-filters-and-sorts) and [table](https://www.notion.com/help/tables) controls as product references, not as a requirement to copy hosted features. Notes and Projects share [collection components and visual rules](../collections.md); shared components own presentation and interaction, while each domain adapter owns schema, persistence, query semantics, and record mutation.

## Data model

A child database creates a database shell, one canonical data source, an initial table view, and a visible `child_database` block in one transaction. A linked database view creates another shell and view over the same data source without duplicating rows or schema. Each view owns its presentation (filters, sorts, visible properties, grouping, date range, row-open mode); views never become separate sources of row data.

A shell can also create additional owned sources or attach an existing local source through a new table view. The Source page in view settings lists other local sources with where each database lives (project group, project, folders, and pages), so similarly named sources stay distinguishable; long paths collapse their middle and truncate each segment. Attached sources share properties, row pages, and templates with their owner while keeping independent view presentation. The active saved view determines which source the layout, New, templates, and property actions use. A single owned source follows the shell title; once a shell owns several sources, they keep independent names. Renaming a linked database never renames shared sources.

Schema updates contain property definitions only and reconcile every referencing view in the same transaction: surviving order, visibility, widths, and query priorities are kept, new properties enter table views, and deleted or incompatible properties leave presentation, queries, grouping, and date or cover selections. Editing a schema therefore cannot restore an older view configuration captured when the editor opened.

Imported title-only child databases stay visible as preservation placeholders until connected to local data.

## Views and presentation

Database blocks render their current view inline; a new database starts with one table view, an empty title, and a row creation line, without requiring the user to expand the block or edit its schema first. Database controls are separate from document block selection. Database links and mentions open a dedicated full-width database surface whose breadcrumb extends through the containing note.

Saved views can use six layouts: table, board, gallery, list, calendar, and timeline. Selecting a view tab switches to it; pressing the active tab, right clicking any tab, or a long press on touch opens that view's menu: rename, display as (text and icon, text only, or icon only, a personal choice kept in folder-local config rather than in the shared view), edit view settings, source (which opens view settings on the Source page), copy a link to the view, open the database as a full page, duplicate, and delete with confirmation. The last view cannot be removed, and a data source keeps a table view for property editing. The toolbar shows a few tabs and a text-only overflow button that opens a views panel with search, per-view menus, New view, and New data source. Views are reordered by dragging toolbar tabs or panel rows, or with the arrow keys on a row's handle; the order is saved on the shell and blocked by the editing lock. Chart, dashboard, map, form, and feed views are not implemented.

- **Settings:** a floating popover beside the toolbar shows the view name and setting summaries, with detail pages for layout, property visibility, filters, sorts, grouping, templates, and transfer. The property editor is a separate popover showing one property at a time.
- **Property headers:** edit, rename, change icon, change type (converting existing values in the same transaction), insert left or right, duplicate (empty values, fresh property and option identities), delete, filter, sort, hide, move, resize, wrap, freeze, format, and calculate. The title property stays first and visible and cannot be converted, duplicated, or deleted; hovering a title cell shows an Open button that opens the row. A duplicated reciprocal relation becomes a one-way relation so it cannot take over the original's reciprocal.
- **Query strip:** all layouts show active filters and ordered sorts below the toolbar, editing the same saved query as view settings.
- **Grouping:** select, status, multi-select, checkbox, people, relation, and date properties. Group order, collapse, and empty-group hiding belong to the saved view. Counts cover the complete filtered source; a multi-valued row appears in each distinct matching group and counts once per group.
- **Formatting:** date and number formats change display only, never canonical values. Date editing offers explicit start, end, and time zone, and changing only the start keeps the existing end and zone.
- **Wrap and freeze:** saved per view. Narrow viewports reduce the effective frozen prefix while preserving the saved boundary.
- **Calculations:** footers summarize the complete filtered source and each group, independent of the loaded window. Empty means null, blank text, or empty arrays; zero and false are populated. Sums of empty input are zero, while average, minimum, maximum, and percent checked have no result. Unique counts use canonical identities. Results are derivative and never stored.
- **Conditional colors:** ordered row or property color rules use the same typed predicates as filters. The first matching rule per target wins, and a cell rule overrides a row tint. Colors are presentation only.

The **editing lock** is a reversible per-shell setting (a linked shell has its own). It blocks view, source, schema, and query changes while row creation and editing stay available. It protects against accidental layout edits and is not an access-control boundary; native commands enforce it.

## Loading and session cache

An inline database shows one placeholder until its view metadata and first row window are ready, so its name, tabs, and rows appear together. Fast loads appear directly, and previously loaded rows stay visible during background refreshes.

Switching application tabs unmounts Notes but retains recent database row windows, view lists, templates, selected views, and scroll positions in a process-local session, so returning renders without another SQLite read when the data is current. The session holds at most 32 resources within a 4 MiB serialized-data budget, evicting least recently used entries; it keeps no inactive DOM. Successful Notes mutations, imports, and history restores invalidate it conservatively across the vault, other desktop windows publish invalidations, stale responses cannot become current, and switching vaults clears it. Tables defer automatic refresh while a text cell has focus so refreshes cannot replace active typing.

## Creation and saving

Selecting `/database` reserves the inline surface immediately; failed creation stays in the editor's retryable write queue with the same identities. The slash query never becomes the title. Converting existing text into a database keeps that text as its title.

New row creation reserves a blank row and focuses its title without waiting for a name, and the next New line stays usable while earlier rows save. A failed creation shows Retry on the row, reuses its reserved page ID, and checks for an already committed page before reporting failure, so retries never duplicate rows. Templates accept a reserved page ID for the same reason. Save feedback stays beside the database title and appears only for slow saves.

## Copy, paste, and removal

Copying an embedded database gives other applications its title and local link. Rich paste in the same vault creates an independent database with new sources, schemas, views, rows, row bodies, nested content, and templates, then offers Dismiss or Paste and sync; Paste and sync replaces the copy with a linked view of the original. References inside the copied graph, including self-relations, point to the new identities, while references to unrelated data stay external. Copy planning captures the source graph first, so pasting into one of its own rows cannot expand recursively. Pasting a local database URL offers an inline mention, a linked view, or a plain URL. None of these choices load source rows until a copy or display requires them.

Copying a cut block includes only objects that were live before the cut. Identity-preserving paste moves the cut graph to the destination, keeping its identities, and rejects destinations inside its own row graph.

Deleting a database, or a selection containing one, asks for confirmation that names the owned sources whose pages move to Trash; removing a linked view confirms separately and keeps shared data. Undo and restore recover the owned graph while preserving items that were already trashed individually. Database blocks do not convert into unrelated block types.

## Row pages and sub-items

Every row is a normal Notes page parented by its data source, with property values matching the schema and an ordinary page body. Rows open as full pages or previews and use the same editor, history, comments, links, assets, and Trash behavior as other pages. They stay out of the ordinary project root because the database view owns their navigation, but they still participate in search, links, and history.

Table rows support sub-items within one source. Parents must be active rows in the same source; native commands reject self-parenting, cycles, and depths over 32. Filtered or unloaded parents do not hide matching children, and child counts cover the complete source. Collapse is saved per view (up to 500 collapsed rows). Trashing or archiving a parent shows its active children at the top level without discarding the relationship. Copies, duplication, graph export, and project history remap and validate these relationships before commit.

## Properties

Supported types are title, rich text, text, number, select, multi-select, status, date, checkbox, URL, email, phone, files, people, place, created and edited metadata, unique ID, relation, rollup, formula, and button. The title cannot be hidden, deleted, or converted. Select-like properties own stable option identities, labels, colors, and status groups. System metadata, computed values, and local people edits are read-only.

## Layouts

- **Table:** inline cell editing, keyboard navigation, column order and width (one saved update per resize gesture), visible and hidden columns, sub-items, and full-page or preview opening.
- **Board:** groups by a compatible property and shares Kanban columns, cards, and inline creation with Projects. Card menus provide keyboard and touch move actions. Dragging writes only when the grouping property has a safe local representation; read-only or ambiguous types disable drag writes. Column counts come from the backend.
- **Gallery:** responsive cards with a page cover, a files-property preview, or no image. Missing media is an explicit card state.
- **List:** compact rows with optional grouping. Grouping never changes page hierarchy.
- **Calendar:** month view over a date property. Ranges appear on every covered day, and creating from a day prefills the date. It does not create Ganbaru Calendar events.
- **Timeline:** a bounded range over a date property with resize controls that write the same canonical date as table cells.

## Relations, rollups, and formulas

A relation targets one data source. Values are also normalized into link rows for validation, backlinks, and efficient reads; those are rebuildable projections. Targets outside the configured source are rejected, and an inverse relation synchronizes only when its schema is valid. Relation text uses the target title, falling back to the row ID.

Rollups compute from current related rows and may use a rebuildable cache; they are never stored as row values. Unsupported combinations and computed-property cycles are rejected.

Formulas are bounded expressions evaluated by a checked Rust engine (literals, property reads, arithmetic, comparison, boolean logic, conditionals, text and numeric helpers). They never use JavaScript evaluation. Unknown properties, cycles, type errors, and runtime failures produce typed errors, and results are read-only.

## Templates and buttons

Database item templates belong to one data source. Creating one snapshots a row's editable properties and body; applying one creates a new row with fresh block IDs and maps properties against the current schema. One template can be the default, blank creation stays available, and editing or deleting templates never changes existing rows. Creating a template starts from an existing row.

Button properties are typed local row actions, not automation blobs. The implemented action updates a configured property on the current row, optionally with confirmation, after reloading current schema and row state. Broad edits, deletion, webhooks, mail, and third-party actions stay unavailable until they have explicit schemas and authority boundaries.

## Queries

All views apply filters and sorts before bounded row hydration and page with stable cursors. A query allows ten predicates, eight nested AND/OR groups, three group levels, and five sorts with one sort per property. Conditions follow the property type: text matching, checkbox state, numeric comparison, and calendar-day or timestamp comparison (ISO dates compare days, RFC 3339 values compare instants). Empty stays distinct from zero and unchecked.

Text matching ignores ASCII letter case only, with no Unicode normalization; `%` and `_` are literal. Text made entirely of Unicode White_Space is empty (see the [schema contract](../../data/schema/notes-and-projects.md)). Queries, exports, and conditional colors share these rules.

Formula, rollup, and button values are hydrated after the row query, so they cannot participate in filtering, counts, or pagination and are rejected as predicates. Relation titles, computed values, covers, and previews hydrate only for returned rows. Exact layout, query plans, and cache schema belong in the [data documentation](../../data/README.md).
