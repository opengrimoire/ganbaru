# Notes editor

Status: implemented. Highlight painting and vertical caret movement depend on the platform webview and still need desktop and Android acceptance (see [Notes editor testing](../../testing/notes-editor.md)).

## Editing model

The editor presents a continuous document backed by a canonical page block tree. Blocks carry content and structure but must not impose text selection boundaries. Lightweight outlines and bounded rendering windows support incremental loading without treating the rendered window as the complete document; operations that need hidden content (move, duplicate, Trash, copy, indentation) read it canonically so it cannot be lost.

App-wide shortcuts (tab navigation, settings, music, theme, zoom, diagnostics, shortcut help) stay available while editing; the shell handles them before the editor. Plain Tab, Shift+Tab, text formatting, undo, and IME composition belong to the editor.

## Rich text and context menu

Text-capable blocks store structured rich text with plain-text caches for search and summaries. Formatting includes emphasis, underline, strike, code, colors, links, mentions, equations, and comments where the block type permits them.

The editor has no per-block add or drag handles. Right-clicking text opens a compact menu with formatting, block type, insert, link, comment, suggestion, and clipboard actions; block surfaces have their own block action menu. Pasting a single URL over selected prose links the existing words; see [Inline links](links-and-collaboration.md#inline-links).

## Block creation

- **Headings:** six levels, matching Markdown structure. They can be created from slash commands, the context menu, or one to six `#` characters followed by Space. Heading sizes derive from the body font-scale setting, and the page title stays larger than heading 1.
- **Slash commands:** typing `/` in an empty text block opens a searchable, grouped command menu filtered by localized names and English aliases. Empty rows stay visually blank. Dismissing the menu never consumes the literal slash text, code blocks keep `/` literal, and filtering waits for IME composition to commit. Opening the menu in the middle of existing prose is outside the current contract. The interaction follows Notion's [slash-command guide](https://www.notion.com/en-gb/help/guides/using-slash-commands); the command set comes from the local catalog.
- **Note rows:** `/Note` immediately reserves the row as a child note and opens it in a preview once its page exists. Creation joins the same ordered write queue as typing, so late editor events cannot turn the reserved row back into a paragraph. An unselected note row is an atomic document item: arrows move past it, Enter and Shift+Enter insert a paragraph below or above, and Space or click opens it. Its title is edited in the child page.

## Keyboard structure

The guiding rule is that structural keys change only the row the user is on, keep other rows where they visually are, and never create artificial empty parent blocks.

- **Enter** splits the block at the selection. Paragraphs, list items, and to-dos continue their type; headings and quotes continue as a paragraph. Enter on an empty list item, to-do, or quote converts it to a paragraph, and an empty indented row first reduces its indentation. Toggles and callouts treat Enter inside the label as entering their body; Enter at the start of a nonempty label inserts a sibling before it. Shift+Enter inserts a soft line break. Code blocks keep Enter as a newline.
- **Backspace** at the start of a list item, to-do, toggle, quote, callout, or heading first converts it to a paragraph, preserving text and children, as one undo step. At the start of indented text it removes one indentation level first. Otherwise it merges into the previous compatible block or removes an empty block, always leaving at least one editable block.
- **Tab and Shift+Tab** change the current row's indentation by one level without moving its neighbours or children. For example, indenting ABC in `ABC: 0, DEF: 1, GHI: 0` gives `ABC: 1, DEF: 1, GHI: 0`. Indentation is stored as a relative `ganbaru_indent` level in the block payload, so it does not require a previous sibling or a parent block. A multi-row selection indents each explicitly selected row once, as one undo step. Nesting into a collapsed toggle opens it so the edited block stays visible. Page roots, databases, table internals, columns, and tab labels are structural boundaries. Code blocks use Tab to edit whitespace.

Numbered list ordinals derive from the complete ordered outline, including unloaded rows, with an independent sequence per parent. A different sibling type ends the sequence.

## Document selection

Ctrl/Cmd+A in the page body selects the whole body, including offscreen blocks. Shift+Arrow, Shift-click, Ctrl/Cmd+Shift+Home/End, and pointer dragging extend text ranges across blocks. A range stores only its anchor, focus, direction, and UTF-16 offsets; selecting does not load the unmounted span, and editing or clipboard operations hydrate what they need in bounded reads. Partial text selections are never rounded to whole blocks, and the end of a range is exclusive (a range ending at the start of a row includes the line break but none of that row's text or formatting).

Cross-block ranges support copy, cut, deletion, replacement typing, paste, and basic formatting. Replacement preserves the unselected prefix and suffix, unselected descendants survive removal of a selected parent, and each operation is one undo step. Note rows are selected as whole items; deleting one removes its paired page through the note lifecycle, and undo restores the original page identity.

Selection shows no action bar or counter; right-click opens selection actions. Clicking row padding, indentation, or a list marker places the caret on the nearest text position instead of selecting the block. Whole-block selection is available from non-text block surfaces, with duplicate, move, clipboard, and delete actions in its menu and keyboard shortcuts. Embedded database fields, table cells, and form controls keep their own editing domains.

## Clipboard

Copy and cut write readable Markdown in `text/plain` and semantic HTML in `text/html`, generated from the canonical model rather than editor CSS. Paste accepts sanitized HTML and Markdown within the supported block catalog and never inserts executable markup or unsafe URL schemes; paste as plain text and paste inside code stay literal. Copying a fully selected collapsed toggle includes its hidden descendants.

Within the same vault, rich paste of a note row duplicates the note graph with new identities, and rich paste of an embedded database creates an independent copy with an optional Paste and sync choice (see [Databases](databases.md#copy-paste-and-removal)). In plain text, notes and databases appear as local `#notes?page=UUID` references, which identify objects in the current vault and are not public or operating-system links.

The [Notes clipboard interoperability contract](../../interop/notes-clipboard.md) owns the supported semantics, approximations, and size limits.

## Undo and redo

Undo and redo record page-local before and after snapshots at the same semantic boundaries as normal commands. Text bursts and consecutive Enter presses can group; formatting, links, paste, template use, and structural edits stay discrete. Snapshots restore focus and the complete selection, including document ranges. The stack is bounded by count and size and is derived state, not a second content source.

Undo replays only changed payloads and placements, using canonical evidence from native receipts for content the editor did not have loaded. A later external change to an affected row rejects a stale undo instead of silently overwriting it, and an undo failure never prevents the page from loading. Converting a block into a child page starts a new undo boundary, because replaying the old paragraph over the new page identity would break the paired page and block lifecycle.

## Persistence

Edits update local content immediately. Typing saves, structural edits, and undo/redo share one ordered persistence queue, and an acknowledgement replaces local state only when no newer local revision exists. A failed write pauses dependent writes, keeps the local draft, and shows a save error with Retry. Unsaved drafts are held in memory, so the window must stay open until saving succeeds.

The same page can be open in several panes or windows. Each typing save carries the content revision it was based on, and Rust refuses it when another pane or window has since replaced or trashed that block; moves and edits to other blocks never refuse it. Structural edits check the full rows they were based on. A refused write is dropped, the page reloads from the database, and a notice explains that the note changed elsewhere. After every committed write, other panes and windows showing the page reload it once typing pauses, keeping any block with unsaved local typing.

Compound actions (split, merge, range replacement and formatting, mixed paste, conversions, table and layout edits, selection indentation and deletion, template and button insertion, and undo/redo) commit through one Rust-owned SQLite transaction. Each request carries a stable operation identity and canonical block preconditions: a stale precondition rejects the whole action, and a retried request returns the committed receipt without applying it twice. Selection, composition, clipboard parsing, and rendering stay in the frontend. See [Notes persistence](../../data/schema/notes-and-projects.md#compound-editor-edits).

Between linked devices, a page open in the editor reloads after each sync apply, keeping the caret, selection, and unsaved typing. A held edit to a block that another device changed since the edit began is rebased when at most one side changed the block's text, each side keeping what it changed; otherwise the block shows both texts with Use mine, Use theirs, and Keep both, or Restore with my text and Discard my text when the block was deleted. Titles, block text, property values, discussion status, and comment bodies that two devices edited concurrently are listed above the page with each version's device and time, and choosing one writes it everywhere. Template and property configuration conflicts keep the version shown now.

**Planned for device synchronization:** text fields will use SQLite-persisted Yrs state with Yjs-compatible editor operations, because full-block payload writes are not a merge protocol. The adapter must preserve relative selections, comment anchors, UTF-16 offsets, local undo, and pending input across failures. See [Device linking and synchronization](../../data/sync.md).

## Comments and suggestions

Page discussions, block comments, inline anchored comments, and text suggestions are collaboration metadata, not block content, and are written in the same transaction as the related canonical change. The page title area offers Add icon, Add cover, and Add comment actions, revealed on hover or focus on desktop, always visible on touch, and also available from Note actions on mobile. Add comment opens a composer below the title and any row page properties, in full pages and previews alike. Accepting or rejecting a suggestion validates its stored base context, so a stale anchor produces a conflict instead of editing the wrong text. See [Links and collaboration](links-and-collaboration.md).

## Media and assets

Local media and files become managed Notes assets after path, size, type, and digest validation; blocks store managed identities, never raw absolute paths. External media is an explicit HTTPS reference and is not fetched automatically. Missing managed files show a recoverable unavailable state, and replacing a reference never bypasses deduplication, history pins, or ownership cleanup.

## Focus, scrolling, and empty pages

One page viewport owns scrolling. Clicking or moving the caret must not recenter the page; programmatic focus scrolls only as far as needed. Loading placeholders keep approximate document position without becoming editable rows.

A page always has a writable body. Opening an empty page creates one paragraph without stealing focus from the title, and deleting every block leaves one focused empty paragraph in the same undo step. When the deleted block is structural, such as a database, it is trashed and a separate paragraph is inserted so undo restores the original identity. Enter in the title moves to the start of the body.

## Accessibility

Insert and conversion menus, formatting, comments, movement, media controls, database actions, and page navigation are keyboard reachable, and selection and drag actions have non-pointer equivalents. Focus stays visible and returns to a stable nearby target after deletion, movement, dialogs, or layout changes.
