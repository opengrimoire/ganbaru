# Data architecture

Ganbaru AI uses three storage classes with deliberately different authority: user documents, structured application data, and device-local runtime state. Treating one class as another causes ambiguous recovery, unsafe sync, and data that cannot be inspected outside the app.

## Sources of truth

### User documents

Diary entries, project working documents, generated reports, and attachments are files inside the active Ganbaru AI folder. When a document format is canonical, the file is the source of truth. SQLite may index its path, metadata, extracted text, tags, or links, but that index must be rebuildable.

Project working-folder Markdown remains ordinary user-owned Markdown. Managed project folders live below the vault. External working folders stay at user-selected paths and are represented in the vault by durable logical identities, never by portable absolute paths.

Keeping canonical documents as files means the user can inspect, edit, back up, and version them with ordinary tools, the folder stays useful without Ganbaru AI, and import or sync conflicts can be resolved at a visible document boundary.

### Structured data and document graphs

Calendar events, Pomodoro state, projects and tasks, Notes pages and blocks, Quick notes, themes, playlist definitions, and organizational Chat state are structured data. SQLite is authoritative because these domains require transactions, foreign keys, stable identities, ordering, and relational queries. Every authoritative connection, including recycled pool connections, uses WAL, `synchronous=FULL`, foreign keys, and a bounded busy timeout.

Notes is intentionally included here. A Notes page is a graph of blocks, properties, links, comments, history, collaboration operations, database rows, and assets. Markdown cannot preserve that graph without lossy conventions, so Notes Markdown is import, export, or bridge output, never the canonical page.

Organizational Chat is also structured data. A project channel can outlive any provider session. Replacing or deleting a provider continuation cannot replace, merge, or delete the surrounding channel, membership, approval, decision, checkpoint, or execution history.

### Device-local state

The platform application config directory stores state that is meaningful only on one installation: the active-vault pointer, device identity, external folder bindings, executable paths, provider homes, process state, probe caches, benchmark state, and transient runtime snapshots. Settings scoped to vault IDs that no reachable recent folder holds are forgotten, so deleted vaults leave no device-local residue. Nothing is forgotten while any recent folder is unreachable, such as a vault on a disconnected drive.

Portable rows may refer to a logical working-folder ID. Resolving it to an external absolute path requires a current device-local binding and filesystem identity check. A portable database never acquires authority merely because it contains a path copied from another device.

## The active Ganbaru AI folder

The active folder contains the vault marker, portable configuration, SQLite database, managed project folders, reserved document directories, and managed assets. The canonical tree is maintained in [AGENTS.md](../../AGENTS.md).

Production and development builds use separate default folders and separate platform config directories. Folder validation rejects an unrelated non-empty folder, an invalid marker, an unsupported schema version, permission failures, and a database that cannot be opened. The application never deletes or silently recreates a configured vault to recover from one of these errors.

Music bytes remain wherever the user stores them. The vault owns playlist definitions, library metadata, source identities, and playback state, not the music library itself. Desktop backups are written to a user-selected location outside the active folder.

Vault selection, whole-vault snapshots, ownership handoff, backup restore, and replacement share one native operation guard. A concurrent request receives a retry error instead of queuing. Before the active pointer changes, pools close, or files are replaced, the operation drains the Focus, Music, and anti-distraction native owners, fences managed file writes, and excludes new SQLite connections. A managed writer that does not drain within a bounded budget cancels the operation and leaves the active pointer unchanged. Android restore rechecks the active path, vault identity, writable role, and ownership generation after quiescence, and validates the archive before touching the vault. Reactivation failures are reported explicitly; a successful file replacement does not imply that native runtimes resumed.

## Portable configuration

Portable preferences that should follow the vault live in `config.json`. Writes use a Rust-owned read, validate, merge, and atomic-replace flow so independent windows do not overwrite unrelated settings. Unknown fields are handled by explicit validation, not silently preserved forever.

Device-only values never belong in `config.json`: external absolute paths, credential material, executable discovery, provider process state, and the active-folder pointer.

## Notes import and export

An exported Notes Markdown file is a derivative view. Editing it does not mutate the canonical page until the user performs an explicit import or transfer operation, which validates and converts external content into canonical rows. Exports may be regenerated at any time.

Project working-folder Markdown is different: it is already file-authoritative and appears beside linked Notes content without being copied into the Notes graph.

Assets use managed relative identities. Import copies validated bytes into a feature-owned asset directory and records the relationship transactionally. An export may copy or rewrite asset references but never becomes a second canonical asset store.

## Chat separation

The durable organization layer owns projects, channels, memberships, messages, ordered provider-session links, canonical events, projections, drafts, attachments, checkpoints, access revisions, and cleanup records. Provider-native thread IDs are continuation handles within that layer.

A run may be targetless while it plans or discusses. Before native filesystem, terminal, Git, preview, or process work begins, it resolves exactly one authorized execution target: a project working folder or a private scratch generation. Provider-native trust does not widen that target. See [Chat access control](access-control.md).

Native Chat sessions use Rust application services and an ephemeral, assignment-scoped internal MCP endpoint for host tools. A separately authorized external MCP service and a `ganbaru-ai` CLI are planned. They must reuse service-layer validation and are not current authorization paths.

## Transactions and filesystem work

Filesystem operations cannot participate in a SQLite transaction, so commands that affect both layers use an explicit staged workflow:

1. Validate authority and all input before mutation.
2. Prepare filesystem work using bounded paths and sibling temporary files where appropriate.
3. Commit the canonical database relationship at a defined point.
4. Finalize or compensate the filesystem step.
5. Persist retryable cleanup when immediate compensation is unsafe or incomplete.

Blocking filesystem, process, and operating-system work must not hold a database transaction or shared async lock. See [Native backend](../architecture/native-backend.md#asynchronous-and-blocking-work).

## Derived data and caches

Search indexes, projections, thumbnails, diagnostic summaries, and exported Markdown are derived. Every derived store needs a declared canonical input, invalidation rule, rebuild path, and size bound. A cache must never become the only copy of user-authored information. Rebuilding a Chat projection or Notes index must preserve authorization and audience filtering.

## Storage decision checklist

Before adding persisted data, answer:

1. Is this user-authored content with a useful canonical external format? Prefer a file.
2. Does it require relational integrity, transactional multi-row updates, or graph identity? Prefer SQLite.
3. Is it meaningful only on one device or tied to a native path or process? Keep it device-local.
4. Is it derivable? Define the canonical input and rebuild path instead of granting the derivative equal authority.
5. Does it contain a secret? Store only an opaque credential reference and keep secret material in the native credential store.
6. Will it synchronize? Give it stable identity, deterministic merge semantics, and explicit authorization first.

Domain records stay normalized. JSON columns are a narrow, named exception: command receipts (typed retry results bound to the complete request) and bounded runtime checkpoints such as accepted Focus control state and device-keyed Music queue state. The owning service validates their encoding on every read and write, and a schema invariant test rejects JSON storage anywhere else.

## Whole-vault handoff and future replication

The implemented single-writer handoff transfers one consistent copy of the complete portable vault between linked devices. Only the current ownership generation writes; other devices hold a read-only replica. It transfers current state and never merges independently edited vaults. Planned concurrent replication requires per-field classification, a mutation journal, and conflict semantics before any field participates. Both are specified in [Device linking and synchronization](sync.md).
