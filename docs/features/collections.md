# Shared collection views

Status: Implemented for the shared surfaces below.

Notes databases and Projects use one collection component family (`apps/client/src/lib/components/collections/`). Changes to shared rows, cells, resize handles, view buttons, floating panels, cards, inline creation, or Kanban interaction apply to both features. Shared components accept snippets and typed callbacks so each feature exposes its own properties and actions.

Rationale: the two features present the same kinds of collections, and one implementation keeps their interaction and visual rules from drifting apart.

## Domain boundaries

The shared UI does not import either feature's stores, invoke native commands, or own persisted data. Notes pages and properties remain Notes data; Projects tasks, statuses, priorities, scheduling, selection, and bulk actions remain Projects data. Each feature supplies only the operations it supports, and shared components never infer unsupported property operations or expose controls that cannot be saved safely.

Each feature owns its complete-result calculations and conditional rules; neither derives totals from a partially loaded row window.

Projects Calendar renders canonical Calendar events and task scheduling, while Notes Calendar renders date properties; their scheduling semantics remain distinct. Timeline and Gantt keep their own domain renderers.

## Tables

Rows share spacing, cell frames, focus treatment, and resize handles. Resizing works with pointer and keyboard; a failed width save restores the canonical width and shows the error. Notes persists widths in its saved database view; Projects keeps its own column sizing, automatic fit, task selection, subtasks, and grouped ordering.

Each row starts with a selection checkbox and an expand chevron in a gutter outside the table: row fills and dividers begin after it, and the gutter hangs into the left margin so the table content lines up with the surrounding title and toolbar, as far as the margin allows without clipping. Every row (headers, data rows, nested rows, add rows, and calculation rows) shares one row height; only wrapped columns grow taller. The checkbox appears on hover or focus and stays visible while it is checked; selecting rows shows one shared action bar with the count, the feature's actions, and Clear. The chevron stays visible on rows that have nested rows and appears on hover elsewhere. Rows with nested rows start expanded; opening a row without them shows an add row for its first nested row, and every expanded row ends with that add row. The name cell's Open button opens the row in detail.

Columns other than the leading title or name column are reordered by holding a header with a mouse or pen and sliding it sideways. Only the header row previews the move, with the passed headers making room; the body columns move once the header is released, and the release does not open the column menu. Dragging near a scroll edge scrolls the table, Escape cancels, and a failed save restores the saved order with an error. While the table is scrolled, a frozen header reorders only among the frozen columns. Touch and keyboard users move columns with the column menu's move actions.

Column headers show the property's own icon, or the icon of its value type when it has none; both features share one icon per property kind. Property icons are Lucide icons only, chosen from the icon square beside the name. Each feature has a General setting that hides these header icons; it is on by default. A header opens one shared column menu, by click or right click: an inline icon and rename field at the top, then the property, query, display, placement, and duplication actions the feature supports. A rejected rename or icon change keeps the menu open and shows the error. Built-in Projects columns cannot be renamed or given an icon. Properties are created directly on the table, from the trailing "+" control or the column menu's insert actions, through one shared draft header drawn where the new column will appear: an icon and an optional name in the header itself, with a searchable type grid grouped into sections below it. A blank name falls back to a unique name derived from the type. Choosing a type creates the property; a failed creation keeps the draft and shows the error, and Escape or an outside click discards it. The column menu's Edit property row appears only for types with settings, such as options, number or date formats, and relation targets, and opens them beside the menu. Every change there saves immediately; there is no separate save step. Delete property asks for confirmation because it removes the property's values from every row. In Projects, Edit property covers custom field options and column formats, and Project settings remain the place for full custom field configuration; Projects has no type change.

Inline creation differs by domain. Projects requires a task name before creating a task, and a failed write keeps the draft. Notes creates a real blank page immediately, since empty Notes titles are valid. Pending writes prevent duplicate submissions.

## Cards and Kanban

Boards share column widths, cards, inline creation per column, and secondary actions in a menu. Each feature provides menu actions for moving between groups so touch and keyboard users never need to drag.

The shared board owns drag state, drop feedback, pending state, and error feedback, and ignores unrelated external drags. Projects supplies status writes and manual ordering; sorted views permit group changes but not reordering within a column. Notes supplies its validated grouping-property writes and does not invent manual order. A failed write leaves the card in its canonical position. Gallery and Notes Calendar reuse the same card and action components.

## View controls and panels

Both features share view buttons and compact floating settings panels anchored to the invoking toolbar control. Opening settings leaves the collection visible and interactive. Panels fit the viewport, scroll only when content exceeds the available height, and stay inside an owning page preview when applicable.

Settings panels are navigable pages: rows show an icon, label, and current value, and selecting a row opens its detail page in the same popover. Back keeps drafts. Escape closes the innermost layer first, and explicit dismissal returns focus to the invoking control. Pointer menus stay compact while touch layouts use larger targets.

Rows inside a floating menu that lead to more options open a submenu beside that row instead of replacing the menu. A submenu is narrower than the menu it opens from and opens just right of its row, flips to the left when the right side lacks room, and opens below its row when neither side fits. Resting the mouse on a row opens its submenu without moving focus; moving to another row closes it after a short grace period unless focus is inside it, and only one sibling submenu is open at a time. Clicking the row or pressing ArrowRight moves focus into the submenu, and ArrowLeft or Escape returns to the row. Touch opens submenus by tap only.

Panels, column headers, and cell values, including select and status cells, share one collection text size derived from the Notes paragraph size and slightly smaller than it, so dense tables stay compact while headers match the rows beneath them; secondary captions such as field labels and counts stay smaller.
