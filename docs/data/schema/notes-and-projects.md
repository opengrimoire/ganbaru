# Notes and projects schema

Notes and Projects share project identity, navigation, task scheduling, managed working folders, assets, and history, but keep different canonical models. Notes is a document graph. Projects is structured planning data plus a logical working-folder boundary.

## Notes canonical graph

A page has stable identity, metadata, placement, trash and archive state, and an ordered block tree. Blocks have stable identity independent of their current parent and order. Mutations validate the resulting graph for cycles, stale parents, invalid project placement, and unsupported content before committing.

Page bodies are not canonical Markdown or HTML. Rich content is validated block data and text runs; rendered output, Markdown export, previews, and search projections are derivative. Page cover descriptors (a designed pattern with a palette slot, or an image reference with an optional focal point) are canonical and follow duplication, templates, and history, while image bytes stay managed assets. See [Page covers](../../features/notes/page-covers.md).

Trash is recoverable. Restore validates the current graph and chooses a valid destination instead of blindly reinstating a stale parent. Permanent deletion applies retention, asset, link, comment, database, and history rules through one domain service.

## Compound editor edits

Compound editor actions are one bounded native command over typed block, layout, and database operations on one page. Rust claims the SQLite writer before checking block preconditions, so the graph change, mention and asset-reference maintenance, one page-history snapshot, and the `notes_edit_receipts` entry commit or roll back together. Operations stay confined to the requested page, except copy sources and an explicit move between named source and destination pages.

Each receipt binds the operation identity to a digest of the complete request. A retry after an uncertain response or restart returns the original result; reusing an identity with different intent fails. Receipts live as long as their page. Block fingerprints are opaque equality tokens over payload, placement, lifecycle, and metadata, not clocks.

Undo is selective: it restores only changed payloads and placements from bounded preimages and never rewrites a whole page, so unrelated later edits and unloaded content survive. Limits bound operation counts, references, traversal, request and result bytes, and one copy budget across every copied source. Exceeding a limit rejects the whole action. In-memory drafts and pending editor requests never become a second canonical store.

## Placement and projects

A page is workspace-rooted, nested below one valid page, or placed through one valid project location (invariant 8 in [Data invariants](../invariants.md)). Moves update project and ancestry implications transactionally.

Project working-folder Markdown stays file-authoritative and is not copied into the Notes graph. The project tree may show pages and files together, but their source-of-truth rules remain distinct.

## Notes databases

Databases define stable property identities, types, options, views, filters, sorts, groups, and row values. A page can be a database row without losing its page identity.

A database shell owns its data sources. A linked shell has independent views and an independent editing lock over another shell's sources; the lock is a local editing preference, not authorization. Copying a database assigns new identities to every owned object in one transaction and remaps internal references while leaving external ones intact. Trash follows owned sources and rows, never a linked shell's shared source, and records which objects each deletion changed so restore does not revive objects trashed independently.

Relations point to stable identities. Rollups and formulas derive from canonical values, and cached results need an explicit dependency revision and rebuild path. Property type changes need a conversion policy and keep incompatible values instead of reinterpreting them. Deleting a property resolves relation, formula, rollup, view, and row dependencies explicitly.

Schema changes reconcile every referencing view, including linked shells, in the same transaction: deleted property references are removed, incompatible grouping and calculations are cleared, and surviving presentation is preserved. Saved views own grouping, wrapping, freezing, display formats, calculations, and conditional colors by stable property identity.

Row sub-items live in `notes_data_source_row_hierarchy`: at most one parent per row, same source, acyclic, and at most 32 levels deep. Trash and archive keep the edges; moving a row to another source detaches incompatible ones. Copies, graph export, and project history include the relationship.

Row windows use stable ordering and keyset boundaries (see [Query plans](query-plans.md)). Group counts and calculation footers describe the complete filtered source, including unloaded rows, and are read projections rather than canonical values.

Text predicates are one contract shared by SQLite queries, Rust export evaluation, and frontend conditional colors. `contains`, `equals`, and `not_equals` fold only ASCII `A-Z`, matching SQLite's default; there is no Unicode normalization, and wildcard characters are literal. Text is empty when null, empty, or made only of the 25 Unicode White_Space characters (U+0009 to U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F, U+3000). U+200B and U+FEFF count as content.

## Links, aliases, mentions, and comments

Internal links store stable target identity when resolved, plus source block identity. Titles are presentation snapshots, not identity. Aliases and unresolved-link state let renamed or not-yet-existing targets reconcile without rewriting text. Backlink and unresolved-link indexes are rebuildable.

Mentions, comments, suggestions, and mention notifications keep author, target, thread, resolution, and delivery state as separate concepts. Notification delivery is a projection. Deleting or restoring content never fabricates a new comment or collaboration identity.

## Assets

Managed Notes assets use feature-owned relative identities below the vault. Page icons, covers, block files, property files, comment attachments, and imported files stay distinguishable so cleanup applies the correct references and retention. Rust validates type, size, content signature, and destination before importing bytes, and uses the staged workflow in [Data architecture](../architecture.md#transactions-and-filesystem-work). Exported paths never become canonical managed paths.

## Import, export, and working Markdown

Import is an explicit conversion into validated canonical rows; an arbitrary Markdown edit is never an implicit mutation. Transfer workflows stage, validate, and report conflicts before replacing content.

Working Markdown is a controlled bridge for selected Notes and related project context, with provenance and revision checks. Applying changes back requires an explicit command and conflict handling. It is not a second live source of truth.

## History and collaboration

Page history stores bounded compressed snapshots with digests and retention metadata. Global retention is 0 (indefinite), 7, 30, 90, 180, or 365 days, and a project may inherit it or override it.

History reads treat stored data as untrusted: encoding, declared size, decompression bound, digest, and JSON shape are validated before use. Restore records the current state first, applies canonical rows transactionally, keeps collaboration sequencing append-only, suppresses historical notification delivery, and rebuilds derived indexes.

Collaboration operations have stable sequence and author identity. They are durable local history and a future sync input, not evidence that remote collaboration exists.

## Search and derived indexes

Full-text search, backlinks, unresolved links, sidebar summaries, computed database values, and previews are rebuildable, each with an explicit source revision and invalidation rule. Authorization and project scope are applied before results leave Rust. A rebuild never changes canonical identities.

## Project planning

A project owns metadata, lifecycle state, optional grouping, views, statuses, tasks, dependencies, custom fields, templates, history, and scheduling links. Presentation order is explicit and deterministic.

Tasks have stable identity independent of status, view, schedule, or parent. Dependencies validate both endpoints, reject self-dependencies and forbidden cycles, and survive reordering. Calendar links distinguish scheduled work from looser references.

Custom-field definitions and values are separate, and changing a definition validates existing values. Custom-field and Notes data source property icons are optional presentation metadata limited to Lucide icons (a `lucide:` value in Projects, the native icon object in Notes); no icon means the type icon, and emoji or uploaded icons are rejected. Templates create ordinary rows with new identities; template IDs never become privileges or shared mutable state. Project history records meaningful user changes, not derived counts, and restore goes through domain commands so task links, Notes placement, and working folders stay consistent.

### Native project mutations

Dependency date cascades, bulk status, priority, archive, and restore, and reordering are native commands with the same shape:

- Rust claims the SQLite writer, then reads the canonical state it needs, including unloaded tasks (archive and restore compute the full descendant closure).
- Only the intended field comes from the client, and its displayed value is checked against storage; unrelated newer fields are preserved.
- Rows, per-field history, and an immutable receipt commit together. Retrying the same request returns the committed result after restart; reusing an operation ID with different intent is rejected. Receipts are deleted with their project.
- Limits on tasks, edges, siblings, and receipt bytes reject oversized work without truncating its meaning.

A dependency cascade previews deterministic finish-to-start shifts over one consistent project graph and reports cycles explicitly. Apply recomputes the reviewed digest and rejects a stale preview. Restoring a child below an archived parent is rejected unless the parent is restored in the same operation. Reordering separates section order from status order and normalizes equal ranks atomically.

Task, custom-field, and option rows carry a revision that increases on every update. The frontend ignores older revisions and keeps deletion evidence for pending requests, so a recovered receipt cannot overwrite newer state or resurrect removed schema. Absence from a paginated view is not deletion evidence.

## Working folders

Every project has a durable managed working-folder row and a managed directory below the vault named by project identity. Creation uses staged compensation so a project never claims a folder that does not exist.

External folders use the same logical identity but resolve through a device-local binding; absolute paths and filesystem fingerprints never enter the database. Chat authority over a project does not imply access to its folders; grants are governed by [Chat access control](../access-control.md).

Deleting a project must account for managed files, linked Notes, tasks, calendar events, Chat history, and external bindings. Filesystem removal is never an automatic foreign-key cascade.
