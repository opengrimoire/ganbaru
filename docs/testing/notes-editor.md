# Notes editor testing

**Status: Reference.** Manual acceptance for Notes in the real desktop and Android app. Behavior contracts live in [Notes](../features/notes/README.md) and the [editor specification](../features/notes/editor.md).

## Automated coverage

Native Rust tests own SQLite atomicity: compound edits roll back after any failed prefix, reject stale or foreign preconditions, reuse retry receipts, and reconstruct correctly after reopening storage. Template, button, and database copies are bounded and keep stable identities on retry.

Frontend tests connect the real action, persistence, tree projection, and undo controllers with delayed storage (for example `apps/client/src/lib/stores/notes/editor-transactions.test.ts`). They cover typing during pending saves, stale receipt replay, queued undo and redo, selections spanning unloaded content, clipboard parsing and serialization, and slash menu behavior.

These tests do not measure real WebView input, clipboard output, layout, or software keyboards. The cases below cover that gap.

## How to run

Run each group on desktop (Linux and Windows) and Android, in light and dark themes, with at least one long note that is partly unloaded or virtualized. Where a case mentions delay or failure, repeat it while storage is slow and after an injected save failure. The common acceptance rule for every edit is:

- Text, caret, and selection stay where the user put them while saves are pending.
- Reopening the note shows exactly the completed operation, with no duplicates or lost blocks.
- A failed save is visible, the draft stays editable, and Retry persists it without overwriting newer edits.
- Late responses from an earlier note, view, or vault never replace current state.
- Undo and redo restore text, structure, and selection in one step per user action.

## Loading and navigation

- Slow page, cover, and database reads show an aligned skeleton only after the reveal delay; fast reads show content immediately. Skeletons never shift layout or block input, and reduced motion is respected.
- A database placeholder spans renderer, view metadata, and row reads without flashing the title or tabs early. Returning to a cached database shows rows immediately.
- A failed page read offers Retry while sidebar navigation stays usable. Switching notes saves pending edits first.
- Side and center child previews keep the correct breadcrumb, undo target, and parent. Closing a preview restores the main editor and scroll without reloading.
- The group, project, and note selector moves header, sidebar, creation location, and settings to the destination, including from side previews. Hierarchy hover panels list child databases and saved views.
- Rename and move pages from the editor and the sidebar; both surfaces update without switching pages. Invalid destinations (self, descendant, cross-project, inactive) are rejected.
- Favorite, archive, Trash, restore, and permanently delete pages and subtrees. A restore with an invalid parent lands at the root with an explanation.

## Rich text and blocks

- Typing, inline formatting, colors, links, mentions, and equations persist after reload.
- Enter, Shift+Enter, and Backspace behave per block type: split preserves annotations, soft breaks stay soft, empty list and quote items return to paragraphs, Backspace at offset zero converts a styled block to a paragraph before merging, and the final block is protected.
- Toggles and callouts: Enter at the start of a label inserts before it, elsewhere creates a child; Enter on an empty child leaves the container; collapsed toggles hide children completely and reopen intact.
- Tab and Shift+Tab indent and outdent once per selected row, keep descendants' relative depths, open collapsed parents, and work in the Linux WebKitGTK event format. Code blocks receive whitespace instead.
- Numbered lists count correctly beyond 9 and 99, across unmounted regions, and restart after nesting or an intervening paragraph.
- H1 through H6 can be created by slash, conversion, and Markdown shortcut, appear in the table of contents, and round-trip through Markdown and HTML.
- Insert, convert, duplicate, move, nest, Trash, restore, and delete each block family. Internal table rows, columns, and tab labels never appear as document rows; removing a column or tab relocates its content.
- Template and button blocks insert ordinary children, explain disabled child-page cases, and return the same identities on retry.
- Unsupported imported blocks stay visible, searchable, and deliberately convertible.
- `/Note` creates exactly one child page even when repeated during a delayed save, and Undo does not replay the old paragraph over it.
- Deleting every block leaves one focused paragraph that participates in undo. Unloaded content is never treated as an empty page.

## Selection

- Ctrl/Cmd+A in the body selects the complete document on the first press, including unloaded blocks, without rendering the whole page. Inside the title, fields, cells, and dialogs it stays scoped to that control.
- Shift+Arrow, Shift+Home/End, Shift-click, and dragging extend precisely across wrapped lines and block edges, keep the original anchor when reversing, and stay responsive across long spans.
- Clicking row padding, markers, or blank space after text places a caret on that visual line without scrolling the viewport.
- Typing, Enter, and plain or rich paste over a cross-block selection keep prefix and suffix text and unselected descendants, and undo as one step.
- After undoing a range replacement, the original range is reselected with its direction.
- IME composition, emoji, and soft breaks work at selection boundaries on each supported WebView.

## Links, menus, and slash commands

- Pasting a URL over selected text creates a link without changing the words; code and plain-text paste stay literal.
- Link hover actions open without moving the caret, stay within the viewport, and do not open on touch or text drag. Click opens the destination.
- Right-click on text opens the text menu with the range still visible; right-click on a non-text block opens block actions.
- `/` opens the slash menu immediately on shifted and unshifted layouts, software keyboards, and after IME commit. Filtering accepts localized labels, the active item stays in view, and Escape or outside click keeps the literal text. Code blocks keep `/` literal.
- Floating panels and menus near viewport edges, in narrow previews, and with the software keyboard stay fully visible and return focus on Escape.
- App shortcuts (Alt+number, Ctrl+Tab, settings, theme, zoom) never type, indent, or format text as a side effect.

## Page chrome, covers, and comments

- Add icon, Add cover, and Add comment open aligned panels that stay visible on scroll and resize, toggle on a second click, and return focus on Escape.
- Every cover design renders in light, dark, and custom themes, on wide banners and narrow phones, keeping the subject recognizable. Selections apply immediately; repositioning persists only after Save position, and Cancel, Escape, or navigation discards it.
- Closing the picker or switching pages during a selection never updates another page.
- Show resolved toggles comment threads once per click, and switching pages does not reuse another page's threads.

## Media and assets

- Attach supported images, video, audio, PDF, and generic files. Oversized, mismatched, unsafe, missing, and unmanaged paths are rejected.
- Removing and replacing references never deletes an asset still owned elsewhere. A missing managed file shows a recoverable unavailable state.
- External references never auto-fetch where content policy forbids it.

## Databases

Run for table, board, gallery, list, calendar, and timeline layouts.

- `/database` shows an empty-titled table immediately, before creation completes. Clicking inside the table never selects the whole document block.
- Add, switch, rename, duplicate, and delete views. Duplicates have independent settings over shared rows; the last view and last table view are protected.
- View settings, filters (up to ten), sorts (up to five), grouping, collapse, and calculations persist, match the full filtered source, and reconcile after schema changes without dropping compatible settings.
- New page shows an editable row immediately, keeps typed titles through delayed creation, and reuses the same page ID on retry.
- Resize, reorder, hide, wrap, and freeze columns; widths stay visible during delayed saves and revert on cancel. Board cards move by drag and menu.
- Linked views share rows and schema but keep their own view settings and title.
- Copying a database creates an independent copy by default; Paste and sync replaces it with a linked view. Pasting a database URL offers mention, linked view, or URL.
- Deleting an owned database, a linked view, or a selection containing several asks with owned-source counts, and Undo or Trash restore recovers the owned graph.
- Relations validate targets and maintain inverse links. Rollups and formulas recompute after source, schema, Trash, and restore changes.
- Templates, typed buttons (with stale-schema rejection), multiple sources, locked shells, and nested sub-items behave correctly after reopening.
- Edits from previews, another window, imports, or history restore refresh affected databases without blanking rows. Switching vaults during a read restores nothing from the old vault.
- A write slower than the threshold shows one indicator beside that database's title.

## Working-folder Markdown

- Open, edit, atomically save, refresh, and open externally an authorized Markdown file.
- An external change during a dirty edit makes Save refuse the stale revision.
- Unsafe paths, symbolic links, oversized files, generated directories, and unsupported encodings are excluded or diagnosed.

## History and transfer

- Page history restores text-only versions and rejects snapshots containing local database graphs; project history restores the complete graph, including pages since moved to another project.
- A safety version exists before every destructive restore or import.
- Markdown, HTML, Notion, export-folder, CSV, graph, and agent-bridge transfers handle supported and unsupported data, with diagnostics that name approximations and never expose tokens or inaccessible content.

## Clipboard export interoperability

The portable contract is in [Notes clipboard interoperability](../interop/notes-clipboard.md). Record application and OS versions, WebView, and both clipboard representations; synthetic fixtures do not count as external-app acceptance.

- Copy and cut single ranges, partial cross-block ranges, whole blocks, and a whole long page. Paste into Notion, Obsidian, and a plain-text editor. Headings, inline marks, links, nested mixed lists, quotes, tasks, tables, and code whitespace survive where the receiver supports them, with no editor chrome or unselected text.
- Plain text uses Markdown heading markers and list indentation. Toggles become nested bullets; callouts use an `<aside>` wrapper.
- Paste from Obsidian, Notion, and plain Markdown into Notes: list hierarchy, heading levels, tables, toggles, and callouts are restored, and pasting into a table cell stays inside that cell.
- Page and database mentions keep local identities through in-app paste. Databases copy through the dedicated source-copy flow, not plain text.
- Stale internal clipboard content is never pasted after the user copies something else in another app. A denied clipboard write keeps cut text in place.

## Accessibility and responsive behavior

- Primary editing, navigation, comments, movement, block insertion, database interaction, history, and recovery work with the keyboard only.
- Focus returns predictably after menus, dialogs, deletion, movement, and layout changes.
- Narrow layouts keep Archive, Trash, restore, and permanent-delete actions reachable, and scaled text does not clip controls.
