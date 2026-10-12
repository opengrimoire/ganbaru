# Notes

Notes is a local knowledge system of pages, blocks, databases, links, history, and collaboration metadata. Its canonical page graph lives in SQLite. Markdown is import, export, or bridge output unless the user is explicitly editing a separate working-folder Markdown file.

## Source-of-truth boundary

Two document models appear in the Notes workspace:

1. **Notes pages:** SQLite-canonical pages, blocks, databases, comments, assets, history, and links.
2. **Working-folder Markdown:** file-authoritative `.md` files from an authorized project working folder, edited as raw Markdown with revision-safe saves.

Working-folder Markdown never becomes a Notes page automatically and does not gain block operations, comments, backlinks, Notes history, database relations, or collaboration metadata. Exported Notes Markdown is derivative; external edits to it are new import input, not edits to the source page.

## Current scope

| Capability | Status |
| --- | --- |
| Pages, folders, navigation, project scoping, favorites, recents, Archive, and Trash | Implemented |
| Designed and uploaded page covers with focal cropping | Implemented |
| Rich-text block editor with document selection, keyboard structure, undo/redo, comments, suggestions, and managed assets | Implemented |
| Atomic compound edits with canonical preconditions, retry receipts, and scoped undo | Implemented |
| Broad local block catalog and preserved unsupported imports | Implemented with documented limits |
| Databases with table, board, gallery, list, calendar, and timeline views | Implemented |
| Database templates, relations, rollups, formulas, and typed buttons | Implemented with bounded action limits |
| Markdown, HTML, Notion API and export-folder, CSV, graph, and agent-bridge transfer | Implemented with loss diagnostics |
| Page and project history, page templates, and safety versions | Implemented |
| Backlinks, aliases, unresolved links, mentions, and local notifications | Implemented |
| Daily-note product surface | Planned |
| Multi-person encrypted sync | Planned |

Real desktop and Android interaction acceptance is tracked in [Notes editor testing](../../testing/notes-editor.md).

## Product principles

- Canonical data survives editor, index, cache, and export changes.
- Unsupported imported content remains visible with diagnostics rather than disappearing silently.
- Managed local files have explicit ownership and validated paths.
- Search, backlinks, graph data, rollups, and formula output are rebuildable projections.
- Destructive restoration creates a safety version and applies canonical changes atomically.
- Page access also governs search, exports, notifications, AI context, history, and linked views.
- The editor stays usable for small notes without requiring database or collaboration concepts.

## Documentation map

- [Pages and navigation](pages-and-navigation.md)
- [Page covers](page-covers.md)
- [Editor](editor.md)
- [Blocks](blocks.md)
- [Databases](databases.md)
- [Shared collection views](../collections.md)
- [Import and export](import-export.md)
- [History and recovery](history-and-recovery.md)
- [Links and collaboration](links-and-collaboration.md)
- [Clipboard interoperability](../../interop/notes-clipboard.md)
- [Editor testing](../../testing/notes-editor.md)
- [Data architecture](../../data/architecture.md)
- [Schema reference](../../data/schema/README.md)
