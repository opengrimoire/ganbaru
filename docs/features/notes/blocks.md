# Notes blocks

Blocks are ordered children of a page or another compatible block. Their canonical type, payload, rich text, parent, order, and Trash state live in SQLite. Internal structural children such as table rows, columns, and tab labels are stored as blocks but rendered through their parent surface.

## Catalog

Status terms: **Implemented** means creation or preservation, editing, persistence, and local rendering support the documented scope. **Implemented with limits** means a named advanced operation is unavailable. **Preserved** means imported content stays visible and exportable but is not fully editable.

| Family | Block types | Status | Important limits |
| --- | --- | --- | --- |
| Text | `paragraph`, `heading_1` through `heading_6`, `quote`, `callout` | Implemented | Headings 4 through 6 are local extensions beyond the current Notion API shape. |
| Lists | `bulleted_list_item`, `numbered_list_item`, `to_do`, `toggle` | Implemented | Nesting follows valid parent rules and hidden toggle children remain canonical. |
| Code and structure | `code`, `divider`, `equation` | Implemented | Equations use a safe local formula surface until a vetted renderer is selected. |
| Navigation | `child_page`, `breadcrumb`, `table_of_contents` | Implemented | Breadcrumb and heading text is derived rather than duplicated. |
| Databases | `child_database` | Implemented with limits | See [Databases](databases.md). Broad destructive automation is unavailable. |
| Layout | `column_list`, `column`, `tab` | Implemented | Internal column and tab-label rows are not standalone document rows. |
| Simple tables | `table`, `table_row` | Implemented | A document table, distinct from a database table view. |
| Media and files | `image`, `video`, `audio`, `file`, `pdf` | Implemented | Remote references are explicit; previews depend on validated managed assets and platform policy. |
| Web references | `bookmark`, `link_preview`, `embed` | Implemented | No automatic remote metadata fetch; open actions allow only safe URL schemes. |
| Reuse | `template`, local `button` | Implemented with limits | Child-page descendants and broad external or destructive actions are unavailable. |
| Synced content | `synced_block` | Implemented with limits | Originals and imported duplicates are preserved; live fanout editing and local duplicate creation are not implemented. |
| Unknown imports | `unsupported` | Preserved | Raw source metadata and warnings stay visible until deliberate conversion. |

## Text and list blocks

Text blocks share the rich-text editor and convert between compatible types. Colors, annotations, links, mentions, comments, and child subtrees survive compatible conversions and duplication. Explicit indentation is a relative `ganbaru_indent` payload level (see [Editor](editor.md#keyboard-structure)); list numbering and bullets derive from sibling structure and indentation. To-do checked state is canonical payload. Toggle open state can be local presentation metadata, while toggle children remain canonical content.

A callout is one colored container whose own text can act as its first paragraph and whose children are ordinary blocks, including nested callouts. Its icon uses the shared note icon picker, and uploaded icons are managed assets. Background color applies to the whole callout, while text colors remain inline formatting.

A toggle's label is editable text and its children stay canonical while hidden. Deleting a toggle row through its row action keeps the hidden children by moving them to the row's parent; deleting a whole-block selection, or a text range that fully covers a closed toggle, removes the subtree.

## Child pages and navigation blocks

Child-page blocks stay paired with normal page rows (see [Pages and navigation](pages-and-navigation.md#child-pages-and-previews)). Converting a block to a child page creates the nested page, moves valid existing children into its body, and opens it without losing the parent link.

Breadcrumbs derive their path from the page graph and label missing, archived, or trashed ancestors. Table of contents derives from current headings and focuses the selected heading.

## Layout blocks

A column list owns column blocks, each owning ordinary content. Columns can be added, removed, reordered, and resized, and blocks can move between them. Removing a column moves its children to a neighbouring column first. Narrow layouts stack columns without rewriting stored order or widths.

A tab block owns internal label rows, each owning its panel content, and renders as one tab surface. Removing a tab relocates its content before deleting the label.

## Simple tables

A table owns table-row children whose cells are rich-text arrays; the parent stores width and header flags. Simple tables have cell keyboard navigation and row and column controls, but no database properties, filtering, sorting, row pages, relations, or views. Those belong to [databases](databases.md).

## Media, files, and web references

Media and file blocks use explicit source objects. New remote references require HTTPS and a type compatible with the block. Supported YouTube links can be stored for video blocks but are opened explicitly rather than embedded. Local attachments become managed assets with content-derived identity and reference ownership. Imported provider-hosted or upload references remain placeholders and are never fetched silently; replacing one is an explicit user action.

Bookmark, link-preview, and embed blocks store the original safe URL and a local caption. Cards derive from URL parts only. No remote page metadata is fetched automatically, which preserves privacy and avoids hidden network access.

## Synced blocks

An original synced block can own ordinary children. A duplicate stores a source-block reference and appears as a read-only reference placeholder. Edits are not fanned out across duplicates.

## Template and button blocks

A template block owns reusable child blocks. Running it duplicates the complete stored child subtree at the configured position through canonical commands. Templates containing child pages are disabled because page identity pairing needs a separate safe duplication contract. This local block stays supported even though Notion's public API no longer creates its equivalent.

A button block has a rich label, optional icon, and bounded typed actions; the implemented action inserts a stored child subtree. Webhooks, email, third-party actions, broad database edits, and destructive automation stay disabled until each has a typed schema, authority model, confirmation behavior, and tests.

## Unsupported blocks

Unsupported imports stay visible with source type, warnings, and raw payload. They participate in search and the normal block lifecycle. Explicit conversion can create a readable summary or editable JSON; conversion never happens silently.

## Validation

Payloads, colors, URLs, IDs, parent compatibility, managed assets, and structural operations are validated at frontend boundaries and again in canonical Rust commands. External data never becomes a typed block without validation.
