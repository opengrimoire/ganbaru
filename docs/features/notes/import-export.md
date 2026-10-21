# Notes import and export

Status: implemented with documented loss diagnostics.

All transfers preserve the source-of-truth boundary. Imports create canonical SQLite pages, blocks, rows, comments, and assets. Exports are derivative views or diagnostic snapshots and never become authoritative merely because a file exists.

## Shared safety rules

- Every external response, file, archive entry, path, payload, and identifier is untrusted.
- Imports preview or prepare content before canonical writes when the format supports it.
- One canonical transaction owns each accepted unit of imported data.
- Local file references are copied only through the bounded managed-asset policy.
- Unsupported content is preserved visibly or reported. It does not disappear silently.
- Diagnostics never include secrets or inaccessible content.
- Exports state their loss boundaries and whether they include asset bytes.

## Markdown import

Markdown import creates a new page and normal blocks. It preserves heading levels 1 through 6 and supports paragraphs, safe inline links, lists, to-dos, quotes, dividers, fenced code, supported images, and simple tables. Safe frontmatter can provide a title.

Unknown frontmatter, unsafe or local file references, unsupported HTML, reference definitions, and syntax that cannot be represented return diagnostics. Unsupported source can become a visible preservation block when useful. A page import accepts up to 1,000 blocks, counted while parsing so oversized tables stop early, and a table wider than the 100 column Notes limit becomes an unsupported block with a note instead of failing the import. HTML import follows the same limits.

Imported Markdown records provenance, but the source text is not retained as editable authority.

## HTML import

HTML import sanitizes before conversion. It supports common text formatting, heading levels 1 through 6, links, lists, checkbox items, quotes, preformatted code, dividers, callouts, toggles, simple tables, and explicitly retained supported HTTPS media.

Scripts, styles, event attributes, unsafe URLs, SVG, iframes, and unapproved embedded content are removed and reported. Local and relative media references require the managed import-file flow. Documents that nest elements more than 256 levels deep are rejected with an error before sanitizing or conversion, which keeps deep markup from exhausting the stack or stalling the HTML parser.

## Notion API import

Notion API import uses a user-provided integration token for the current request only. The token is never stored in SQLite, configuration, page payloads, comments, or diagnostics.

The current adapter targets public Notion API version `2026-03-11`. It can import selected pages, recursively paginated block children, data-source schemas and rows, unresolved comments, and optional users. Pages and data sources are chosen by ID or by Notion link; every object ID, typed or returned by Notion, must be a 32-digit hex UUID and is sent in canonical form, so no input can change the API path or query.

Responses stream into strict byte limits before JSON parsing, require JSON media types, bound pagination and object counts, detect repeated cursors, cap retry waits, and cancel when the operation is dropped. Remote response bodies never appear in errors.

Each page or row body accepts up to 5,000 blocks. One import makes at most 50,000 API requests and fetches at most 100,000 blocks; past 20,000 comments, later comments are skipped with a note. The user can stop a running import, while closing the dialog lets it finish in the background. A stop, or reaching the request or block budget, keeps every source finished before it, skips the source in progress and later ones with a note naming where it stopped, and rebuilds search and link indexes.

Supported objects convert to local Notion-shaped contracts. A bookmark, embed, or link preview whose URL uses a blocked scheme keeps its caption and loses only the URL with a note; an embed or link preview with another safe link, such as `mailto:`, becomes a bookmark. Unsupported blocks become visible placeholders. Unsupported properties degrade only through an explicit documented representation and diagnostic. Provider-hosted file URLs and upload identities are not downloaded automatically.

## Notion export-folder import

The user chooses an already downloaded Notion export directory with the native folder picker; the webview cannot name the folder, and canceling the picker is an ordinary outcome. The filesystem root, the home folder or any folder containing it, and folders inside the active vault are rejected so a broad selection cannot import unrelated documents. The chosen folder is scanned within bounded depth, a total budget of visited files and folders, and a cap on importable files; reaching a budget stops the import with an error. Hidden files, unsafe paths, symbolic links (including Windows junctions), special files, and workspace sitemap artifacts are excluded, each skipped link is reported, and files are read only when they resolve inside the chosen folder. Import notes are capped and the rest summarized.

Markdown and HTML reuse the canonical importers. Local links between exported files are rewritten to local Notes targets where identity can be established. Local assets are copied only when the user enables copying (off by default) and the shared boundary validates the source.

CSV files can reconstruct data sources. The first column becomes title, remaining columns become compatible properties, and non-empty rows become row pages. A matching Markdown or HTML file can supply a row page body rather than creating a duplicate page. Each exported page or row body accepts up to 5,000 blocks, matching Notion API import, and one folder import accepts up to 500,000 blocks in total. A file that cannot be read or parsed, such as one over the size or nesting limits, or one that would pass the folder total, is skipped with a note while the rest of the folder imports. Each referenced local file is copied once per import, and copying stops with a note after about 1 GiB of files.

## CSV database import

CSV import is a dry-run-first path into an existing data source. Column mapping can use property name or stable identity. Preview reports mapped and skipped columns, valid and invalid rows, and row-level diagnostics without writing.

Accepted rows become normal database row pages. Writable scalar, select-like, date, contact, place, and valid relation properties can import. Generated, computed, system, file, people, rollup, formula, unique-ID, and button values are skipped unless a dedicated safe conversion exists.

Invalid rows do not partially write broken property state.

## Markdown export

Markdown export renders ordered active blocks, safe rich text, supported mentions, equations, headings, lists, to-dos, toggles, callouts, quotes, code, dividers, simple tables, and media references. Comments are optional and omit local read state.

Text is escaped so it never becomes raw HTML, and link destinations are percent-encoded. Web references and rich text links become Markdown links only for the schemes their blocks accept with an explicit scheme; anything else is written as plain text with a note. HTML export applies the same rule to web references.

Markdown cannot represent every page, database, layout, color, asset, upload, or unsupported block losslessly. Those approximations and omissions appear in diagnostics. The output can always be regenerated from SQLite.

## HTML export

HTML export produces a readable static archive with page files, shared styling, a manifest, optional subpages, optional managed assets, comments, and database view manifests.

Local Notes links target included exported pages. Assets are copied only when requested and available, and only from a regular file in a managed Notes asset folder reached without symlinks whose bytes still match the recorded size and digest; any other asset is left out with a warning. Database views export readable row data and settings rather than pretending to be a complete interactive database runtime.

## CSV database export

Current-view export applies the active table properties, filters, and sorts. Full-data export includes all active rows and current properties without view hiding.

Headers are deterministic. Cells use readable plain text, including available relation titles and computed output. In the saved file, text cells and headers that start with `=`, `+`, `-`, `@`, tab, or carriage return get a leading `'` so spreadsheets show them as text instead of running a formula; number properties and plain numbers such as `-12.5` stay unchanged, and the agent bridge reads the exact text. Structured values that cannot round-trip through CSV produce warnings, so CSV is never presented as a complete database backup.

## JSON graph export

JSON graph export is a local diagnostic and backup-oriented snapshot of the Notes SQLite graph. It includes a schema version, applied migration context, object counts, warnings, and selected canonical families.

Callers can include or omit rebuildable indexes, history, templates, and local UI state. JSON columns export as JSON values rather than encoded strings. FTS shadow tables are omitted. Managed asset metadata and references can be included, but binary files remain outside the JSON and produce a clear warning.

## Agent bridge export

Agent bridge export creates deterministic readable Markdown for an explicitly selected Notes page and approved related context. It can include subpages, backlinks, database views, project tasks, and comments according to effective access. Backlinks follow the same visibility as the backlinks panel, so trashed or archived sources are left out.

The output begins with a source-of-truth warning, does not create canonical export rows, and does not embed managed asset bytes. It never includes restricted page names, counts, backlinks, rows, comments, tasks, or derived facts merely because they share a project.

This bridge is the current agent-readable Notes export. It is not the planned external `ganbaru` CLI and does not authorize internal coding-agent access by itself.

## Round-trip diagnostics

Transfer surfaces report object counts and categorized outcomes such as preserved, approximated, skipped, unsupported, warning, and error. Diagnostics can point to safe source lines, object IDs, paths inside the selected root, CSV rows, or local Notes targets.

Navigation links in diagnostics are convenience only. They do not make the source file, folder, archive, CSV, JSON, or derivative output authoritative.

## Structure fidelity

Markdown and HTML transfers preserve all six heading levels. HTML body headings use matching `h1` through `h6` elements rather than shifting levels beneath the page title, so deep headings are neither flattened nor emitted as an invalid `h7`.

Explicit text indentation is kept in graph export, shown as relative margins in HTML, and emitted as leading spaces in Markdown, where readers may interpret deep indentation as code. Indentation beyond the bounded whitespace budget produces a diagnostic while keeping the text.

HTML and Markdown exports omit page cover appearance and report `page_cover_omitted`; graph export keeps the editable cover descriptor. See [Page covers](page-covers.md#persistence-and-transfer).
