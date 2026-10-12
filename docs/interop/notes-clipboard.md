# Notes clipboard interoperability

Status: implemented with automated coverage. Acceptance in external applications remains pending.

## Scope

Notes writes readable Markdown in `text/plain` and semantic HTML in `text/html`, generated from the canonical model rather than editor CSS, for keyboard and menu copying, cross-block selections, whole blocks, and table-cell text. The receiving app chooses which representation to use. Native copy events provide both formats; menu copying falls back to Markdown alone when the webview lacks the rich asynchronous clipboard API. Failed writes leave cut content intact.

Text editors and document ranges accept sanitized HTML or Markdown. Table cells accept inline formatting and flatten block structure into text. Whole-block paste from a non-editable row is an internal graph operation; if another app replaces the system clipboard, that stale internal reference is discarded. Pasting a single web, email, or local Notes URL over selected prose links the existing words, while code and explicit plain-text paste stay literal.

This contract covers clipboard exchange only. SQLite remains authoritative, and file import and export, managed assets, and working Markdown have separate workflows.

## References

- [Obsidian basic syntax](https://obsidian.md/help/syntax) and [Obsidian Flavored Markdown](https://obsidian.md/help/obsidian-flavored-markdown): six heading levels, CommonMark and GFM, and no Markdown rendering inside HTML elements, which is why combined underline marks use nested semantic HTML.
- [Obsidian callouts](https://obsidian.md/help/callouts) and [Obsidian HTML guidance](https://obsidian.md/help/html): foldable `+` and `-` callouts, and why `<details>` is not a reliable Markdown substitute.
- [GFM](https://github.github.com/gfm/): table, task, and strikethrough syntax used for portable exchange.
- [Notion import](https://www.notion.com/help/import-data-into-notion), [keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts), and [content styling](https://www.notion.com/help/customize-and-style-your-content): supported structures, but no published clipboard format.

Because Notion publishes no clipboard contract, toggle and heading handling were shaped by observed behavior: Notion copies toggles as nested bullets in plain text and drops the body of a closed `<details>` element on paste. Automated fixtures are synthetic semantic examples, not captured Notion or Obsidian payloads.

## Supported semantics

| Content | Copy from Notes | Paste into Notes |
| --- | --- | --- |
| Headings | H1 through H6 in HTML and Markdown | H1 through H6; matching Markdown levels correct conflicting HTML levels without discarding inline styling |
| Paragraphs and line breaks | Semantic paragraphs and Markdown paragraph/hard-break syntax | Paragraphs and supported line breaks |
| Bold, italic, strike, inline code | Semantic HTML and Markdown | Both representations, including adjacent and overlapping annotations |
| Underline | HTML, including portable inline HTML in Markdown | Supported semantic inline HTML |
| Web/email links | Explicit HTTP, HTTPS, and mailto links | Supported links; relative destinations remain readable text without an invented host |
| Local text links | Linked label and validated `#notes?page=UUID` reference, optionally with `block=UUID` | A navigable reference to the original page or block, preserved through canonical saving |
| Local note rows | Title and `#notes?page=UUID` reference in Markdown; HTML includes the local source page identity | Same-vault rich paste creates independent canonical note copies, including nested notes; plain Markdown creates a page mention |
| Embedded databases | Title and `#notes?page=UUID&block=UUID` reference in Markdown and HTML, in document order; canonical local HTML also includes the source identity | Same-vault rich paste creates an independent database copy; Paste and sync replaces it with a shared view. Plain Markdown retains the link, and pasted database URLs offer mention, linked view, or URL |
| Local page and database mentions | Readable title, available local hyperlink, and validated inline identity metadata in HTML | Same-vault rich paste retains the inline reference without duplicating its destination |
| Bullets, numbered lists, tasks | Nested lists and checked state | Mixed nested hierarchy and checked/unchecked tasks |
| Toggles | Open `<details><summary>` in HTML so child blocks stay available to rich paste readers; a Notes attribute records closed state. Plain text uses a nested bullet with indented child paragraphs | Sanitized `<details><summary>` and incoming foldable callouts become toggles with children and initial open state; a plain-text bullet remains a list |
| Callouts | Semantic `<aside>` in HTML with icon and color metadata; plain text uses a readable `<aside>` wrapper with an emoji line and nested Markdown blocks | Sanitized HTML and Notion-style plain-text `<aside>` wrappers become callouts with normal child blocks, including headings and nested callouts |
| Quotes and dividers | HTML and Markdown | Quote text, paragraph boundaries, and dividers |
| Fenced code | Safe variable-length fences and language metadata; literal HTML code text | Code text and language; existing code editors keep pasted source literal |
| Simple tables | HTML cells and header flags; GFM table syntax | Structured tables, rich cell text, headers, and retained surrounding text |
| Cell text selections | Markdown and semantic inline HTML | Inline styles retained; multiple blocks flattened into the cell |
| Partial/document selections | Selected UTF-16 ranges and hydrated offscreen content | Retained prefix/suffix, hierarchy, and undo/redo |

Heading reconciliation applies only when the HTML and Markdown representations have equivalent text with the same heading count and corresponding heading text; it never shifts arbitrary website headings. When a foldable Markdown callout and styled HTML without a semantic `<details>` contain the same words, the Markdown structure wins.

Pasting a toggle into the middle of text keeps the prefix and suffix in surrounding blocks, and a pasted closed toggle leaves focus on a visible block.

## Approximations and limits

- Fonts, sizes, alignment, arbitrary CSS colors, and layout are not a cross-app fidelity guarantee. Notes emits and reads its own color metadata in HTML; external color palettes are not imported, and Markdown has no color syntax.
- HTML preserves explicit blank blocks, repeated spaces, and tabs; Markdown consumers may collapse them.
- Ordered lists use the Notes numbering model; HTML start values, reversed numbering, and custom task states are not kept.
- GFM requires a header row, so a headerless table gains an empty header on Markdown export. Markdown cannot represent row-header flags or merged cells; merged HTML cells expand into a rectangle with content in the first cell. Nested and over-wide tables degrade to readable text.
- There is no shared Markdown toggle syntax across Notion and Obsidian. Notes writes a nested bullet in plain text (Obsidian will read it as a list) and an open `<details>` in HTML so rich readers see the children, recording closed state in a Notes attribute that other apps may ignore. Plain-text callouts preserve only a leading emoji icon. Columns flatten into document order.
- Relative paths have no shared vault base. Images keep their descriptions and source references as text; clipboard handling never downloads images, imports media bytes, or authorizes foreign paths.
- Wikilinks, equations, highlights, footnotes, embeds, comments, relations, synced objects, foreign page identities, and plugin syntax have no general conversion; available text stays readable. Local note and database references resolve only in the same vault and fail visibly when the source is unavailable.
- Rich HTML is sanitized and bounded to 128 KiB, plain text to 64 KiB, and structured paste to 101 blocks, with readable flattening where applicable.
- Clipboard permissions, webview capabilities, the operating system, and receiving-app settings affect which formats are available.

## Verification

Automated tests cover serialization and reparsing, heading reconciliation, links and unsafe markup, nested lists, toggles and foldable callouts, code fences, combined inline styles, tables, Unicode, whitespace, empty blocks, insertion boundaries, write failures, stale internal clipboard ownership, and undo/redo for toggle, table, and list paste.

These tests establish behavior in Notes, not in every version of Notion or Obsidian. Use the [manual clipboard acceptance matrix](../testing/notes-editor.md#clipboard-export-interoperability), recording application versions, OS and webview, source and destination modes, clipboard formats, and observed differences, before claiming external-app acceptance.
