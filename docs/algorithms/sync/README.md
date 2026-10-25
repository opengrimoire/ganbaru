# Sync merge rules

**Status: Reference.** These rules are implemented in the sync engine for every replicated domain; a test-only fixture domain exercises the shapes no production domain uses yet. Delivery, writers, and mixed mode are owned by [Device linking and synchronization](../../data/sync.md); the reasons behind the design are in [Sync engine decisions](../../architecture/decisions/sync-engine.md).

## Model

- **Operation.** A signed, immutable record from one writer, identified by space, writer, and a sequence that is contiguous from 1. Each operation carries a hybrid logical clock and a causal context: for every writer, the highest sequence its author had applied when sealing.
- **Field group.** The unit of merging inside a row: one column, or several columns that must change together. A write changes whole groups.
- **Version.** One writer's value of one group of one row. A replica keeps at most one version per writer per group.
- **Supersession.** A new version removes exactly the versions its causal context covers. The remaining versions of a group are mutually concurrent.
- **Winner.** The version materialized into the domain table, chosen by the group's merge kind. Versions with equal value hashes are never a conflict. Sealing skips a captured group whose value hashes to its uncontested winner, since writing it again changes nothing; a group forced by a conflict resolution is always written.
- **Causal readiness.** An operation applies only after its writer's previous operation and every operation its context depends on. Validation depends only on the operation's bytes and the current manifest, never on local state or arrival order. An operation from a newer manifest version is held, and a new version may only add tables and groups or loosen rules, so an operation valid under one version stays valid under later ones.
- **Apply group.** Consecutive operations of one writer that apply in one transaction or not at all, because their changes only hold together. Each operation of a group but the last says in its header that the group continues. The group ends at an operation that does not continue, at its 64th operation, or at the operation whose envelope bytes reach 1 MiB, whatever the header says, so every receiver forms the same groups from the same operations. Nothing of a group applies until every operation of it is stored and ready; an operation the selected state excludes, or one past its writer's cutoff, ends the group before it. A chain is a run of one writer's operations that each say the next continues, which the bounds cut into several groups. No group of a chain applies until the whole chain is stored and closed; its groups then apply in order. On a receiver that can seal, every repair waits between two of its groups, and after its last group the rows it touched are repaired, at most 4,096 of them per transaction together with the rows that reference them. A receiver that can seal starts one chain at a time, each once the rows the previous one deferred were repaired. Of the chains that could start, the one whose last operation the receiver stored first starts, then the writer id, never the clock, so a writer cannot keep another writer's chain waiting. A chain closes at an operation that does not continue, before its writer's cutoff, an excluded operation, or an operation held as invalid, and at its 256th operation; a missing operation or a hold for a newer format or manifest keeps it open.

The hybrid logical clock is 48 bits of Unix milliseconds and a 16-bit counter. A replica adopts remote clocks only up to one minute past its own time. The clock never decides what is superseded or kept, only which concurrent version is displayed. Clocks are a tiebreak, not authorization, and never cause rejection: a paired writer that advances its clock wins concurrent ties until real time catches up. That is accepted because the writer already holds write authority, its operations are signed and attributable, its device can be revoked, and merges stay deterministic. Refusing far-future clocks at the hub would strand honest writers whose clock was once wrong, since later operations of the writer cannot be admitted past a refused one.

## Merge kinds

| Kind | Winner among concurrent versions | Quick notes use |
| --- | --- | --- |
| Immutable | Set on create; concurrent creates keep the lowest `(clock, writer)` | `created_at` |
| Register | Greatest `(clock, writer)` | Title, body, color, tag name, tag color |
| Coupled register | Greatest `(priority, clock, writer)` over several columns that must stay consistent | Lifecycle: pinned, archived, and trash time, with priority trashed, then archived, then active |
| Max | Greatest value, then `(clock, writer)`; never a conflict | `updated_at` |
| Position | Greatest `(clock, writer)` over an order key; rows sort by `(order_key, id)` | Note and tag order |
| Reference | Greatest `(clock, writer)` over a row key that resolves through redirects | Note tag |
| Owned children | The parent group's value is a canonical list that replaces the child rows | Body text runs, note images |
| Existence register | Greatest `(clock, writer)` over 1 (present) or 0 (removed) on a derived-key table | Project dependencies, tag links, custom field values |
| Tree | A reference into its own table and a position, each won as above, then deterministic [cycle and depth repair](#trees-and-acyclic-edge-sets) | Project tasks (parent, depth 1) |
| Acyclic edge set | Existence registers of a derived-key edge table; an edge that closes a cycle is severed | Project task dependencies |

- A multi-column CHECK always lives inside one coupled register, so every materialized winner satisfies it.
- There is no counter kind. Usage and statistics are rows keyed by device and bucket, each written by one device and merged with `Max`, so concurrent increments never meet in one version.
- A register is surfaced when its concurrent distinct versions are shown to people as a conflict; otherwise it resolves silently. Quick note title, body, and images are surfaced. Color, tag, lifecycle, order, tag names, and tag colors are silent.
- A group is recoverable when edits to it that a deletion did not see should be offered for recovery. Quick note title, body, images, color, tag, and lifecycle are recoverable; order, timestamps, and every tag group are not.

## Trees and acyclic edge sets

A tree table nests rows through a reference group into the table itself and orders siblings with a position group, so a move writes only the moved row's parent and order key. Because each row has one parent, every cycle among winning parents is a simple loop, broken at the row whose winning parent version has the greatest `(clock, writer)`, with the greater row key breaking a tie within one operation. Then every row deeper than the table's maximum depth is detached, which gives the same result as repeatedly moving the deepest row to the root. A detached row materializes as a root and keeps its order key, and a replica that can seal writes its parent as null. Detaching depends only on merge state, so replicas that cannot seal show the same tree. A replicated move log is not used, because every replica would have to undo and replay moves in one total order.

An acyclic edge set is a derived-key table whose rows are directed edges between two rows of one table, such as dependencies. Edges count while they exist and both ends are shown, taken in ascending `(clock, writer)` of their winning existence version, then row key. An edge whose end already reaches its start through the edges taken before it is severed: it is hidden and surfaced as a conflict of its existence group until an edge of the cycle is removed. Severing seals nothing, so it undoes itself once the cycle is gone.

## Derived keys

A derived-key table keys each row by its natural identity, such as a membership's two ends, so every device derives the same key for the same row and adding it again must work after a removal. The manifest names the key prefix and the input columns in derivation order, and a change that writes every group of such a row is invalid unless its key equals the key derived from those values; a table keyed by a fixed natural key, such as preferences, declares no prefix and no inputs. Such a table has a hidden existence register instead of tombstones: a local insert or update writes 1, a delete writes 0, and the domain row is deleted while the winner is 0 and inserted again when it returns to 1. A local insert of a key the space already knows writes every group, immutable ones included, so the re-added row carries its new values. Immutable groups hold only key columns, the re-create guard does not apply, and a derived-key table never redirects or repairs duplicates. A derived-key row whose existence winner is 0 counts as deleted for references to it.

A table with a composite primary key replicates under a `sync_key` text column derived from the primary key columns, which form its required immutable identity group. A reference in that identity group is never redirected: while its target is hidden as a duplicate or not live, the row is hidden instead.

## Order keys

Order keys are variable-length base62 strings compared by bytes. The first character encodes the length of an integer part, and an optional fraction follows that never ends in `0`. A key can always be generated before, after, or between any two distinct keys without changing other rows. Moving a row writes only that row's key.

When two rows end up with equal keys, which concurrent placement can cause, the row id breaks the tie. A later local placement between two equal neighbors re-keys only the run of rows equal to the lower neighbor. When keys would exceed their length bound, the local command re-keys the whole group, which is an ordinary write.

## Deletion and recovery

- A deletion is a tombstone carrying the causal context of its operation. A tombstoned row is deleted from the domain table, its key is never reused, and older operations never resurrect it.
- The tombstoned row keeps its versions as retained state. A retained version of a recoverable group that no tombstone's context covers is an edit the deletion did not see. Such a row is a recovery entry, regardless of whether the edit arrived before or after the deletion.
- Restoring creates a new row from the winning retained values. Restoring and discarding both seal a tombstone whose context covers every retained version, which closes the entry on every replica.
- A live row is hidden by cascade when a cascading reference, followed through cascading references of other live rows, reaches a deleted root: a tombstoned row without a redirect, a removed derived-key row, or a severed edge. A reference cycle hides nothing on its own. Hiding is a pure function of merge state; a replica that can seal also tombstones each hidden row with a cause naming its root, while a hidden derived-key row stays live in merge state.
- A tombstone naming a cause covers what the cause's deletion covers, because a repair sealed later may have seen edits the user's deletion did not. A row hidden by cascade is checked against its root's coverage, so a child edited concurrently with its parent's deletion is offered for recovery.
- Recovery groups rows under their root: a row deleted by the user is its own root, and a row deleted or hidden by cascade belongs to the root its cause or its cascade reaches. One entry offers the root with every descendant holding an unseen edit, and restoring or discarding it closes the whole group.
- A tombstone can redirect references to a row with a lower key. A reference to a tombstoned row follows the lowest redirect among its tombstones, or resolves to null when there is none. Redirects always point to lower keys, so chains terminate.

## Publication

- A table may declare a publication predicate over its own columns, such as a message that replicates once it is sent. Rows that fail it are drafts: they stay on the device that wrote them and are never captured. The write that first satisfies the predicate captures the row as a create with every group.
- Publication is monotonic: a guard refuses a local write that makes a published row fail the predicate again. A merged winner that fails it is an invalid row, hidden like a row hidden by cascade.
- A child table may publish with a predicate table through an immutable cascading reference. Its rows follow the parent: they are drafts while the parent is, they are captured with every group when the parent publishes, and they are hidden while the parent is. A child may declare that a null reference publishes at once, as a row with no parent; its column is then nullable, and `NOT NULL` otherwise. The reference still cascades, never `ON DELETE SET NULL`: clearing it would publish a deleted parent's pending children through a foreign key action, and a receiver's merged reference could disagree with the local one. Replicated references to a predicate table from any other table are refused, since they could name a draft.
- Capture seeding treats existing rows the same way: only rows that satisfy the predicate, or whose parent does, are seeded.

## Deletion holds

- A table may hold deletes for domain Undo. Domain code holds a local delete in the transaction that deletes the row, for at most 10 seconds; sealing leaves the delete local until the lease ends.
- Undo inside the lease checks that the hold still exists, then inserts the row again with its key. The capture becomes an ordinary write, so no tombstone is ever sealed and no recovery entry appears. Undo after the lease refuses, because the delete may already be sealed and published keys are never reused.
- Published rows the delete removes through `ON DELETE CASCADE`, at any depth and in derived-key tables too, share the lease. Undo checks the hold of the rows it deleted and inserts them again with their children, so no tombstone or existence removal is sealed for any of them; expiry seals the whole set, with the children's tombstones naming their cause. Restoring the parent with changed values but without its children seals the children's deletes at once, because materializing the parent would otherwise bring them back. A parent restored unchanged seals nothing, so its children's deletes stay held until the lease ends.
- The same lease may also hold rows the deleting transaction inserts in tables that hold deletes, such as the Calendar archive of a deleted event, together with their cascading children. Undo removes them again, so nothing about them is ever published; expiry seals each as one create. A held insert seals early in the same cases as a held delete, and also when a constraint over its table must read it.
- A held delete seals early whenever a remote operation or a merge would write its row or a row cascading with it, so merge state and the domain table never disagree. Quiescing before a handoff or backup and revoking the local writer also end every hold.

## References

- A reference column holds a row key of any replicated table, including its own table and tables listed after it. A reference group holds one such column; register and immutable groups may also declare several, each resolved like a reference group while the group's value merges as a whole.
- Each reference declares the delete action of its foreign key: clear to null or cascade. An immutable reference cascades, because clearing would change an immutable column. A row whose cascading reference no longer resolves is hidden by cascade, as described in [Deletion and recovery](#deletion-and-recovery).
- Tables are listed in dependency order: a table needing another table's rows (a reference, an owned child's foreign key, or a trigger visible to the applier that reads it) comes after it, unless both lie in one strongly connected component, which is listed contiguously. Foreign keys are deferred while the engine applies, so a reference cycle materializes within its component in one batch. Owned and applier-visible dependencies read rows statement by statement, so they never lie on a cycle.
- Deleting a materialized row rematerializes the rows in every table whose references named it, because the foreign key action clears or cascades them at once.

## Owned tables

- An owned table holds child rows whose ordered list is the value of one immutable, register, or coupled group of the owner. A domain may keep its own codec for the list; otherwise the generic codec encodes each row's declared typed columns in order. Real numbers never travel in replicated columns.
- Owned rows are keyed by owner and ordinal, and materialize writes ordinals from zero without gaps, rewriting a stored list whose ordinals have gaps. A nested owned table keys its rows by owner, parent row ordinal, and its own ordinal, and its rows travel inside their parent row's value, so they follow the parent row when the list reorders.
- An id column of an owned row is a unique data column, reinserted unchanged. Uniqueness is validated within one owner's value only, so domain code must mint fresh ids: two owners holding the same id cannot both materialize.
- An owned row may name rows of other tables. While a target is missing, deleted, or hidden, materialize skips the row with its nested rows and numbers the rest from zero; the row stays in the group value and returns with its target. Domain code that deletes such a target locally numbers the remaining owned rows from zero again, as materialize writes them.
- Foreign keys into an owned table come only from its owner, its own rows, or its nested tables.

## Materialize hooks

A domain may register a hook for a replicated table. It runs in the engine transaction after materialization with the keys of every changed row of that table, deleted rows included, and a change sink whose notices apply and seal reports return so the app can refresh. Hooks maintain hook columns and derived tables. A hook column is never captured, has no foreign key, is nullable or has a default, and is excluded from codecs and summaries; domain code keeps it current for local edits.

## Unique values

A unique column set equal to the non-empty inputs of a derived key needs no resolution, because the row key already makes those values unique. Live rows whose winning values collide on any other declared unique column set form a class. Rows hidden for another reason (by cascade, severing, removal, or publication) never keep a class, so a deleted or hidden row cannot take a surviving row down with it. The remaining rows are taken in key order: a row that collides under any rule with a row already shown is hidden behind the lowest such row, and every other row is shown, so a table with several unique rules never hides a row behind a hidden one. References to a hidden duplicate resolve to its kept row. Because the rows hidden for other reasons can depend on the duplicates, the choice repeats until they settle, and falls back to ignoring them after a fixed number of rounds. Hiding is a pure function of replicated state, so every replica shows the same rows regardless of arrival order. A replica that can seal also writes repair tombstones for hidden rows that redirect to the kept row; concurrent repairs from several replicas are harmless. While materialization swaps unique values between rows, a value moves aside under a placeholder so the index never fails mid-batch.

## Conflicts

A conflict is a surfaced group of a live row with two or more versions whose values differ, or the existence group of a [severed edge](#trees-and-acyclic-edge-sets). Every replica derives the same conflicts from its merge state. Resolving writes the chosen value with a context that covers every alternative, even when the value is unchanged, so the resolution supersedes all of them and replicates like any edit. Keeping both keeps the displayed value and creates a new row from the alternative.

## Worked examples

Phone P and desktop A share a vault. A is the hub. Clocks are written as small integers.

### Concurrent title edit

1. Note N has title "Plan", version `(A, seq 4, clock 10)`.
2. Offline, P sets the title to "Plan draft" at clock 12. Its context covers A's seq 4, so P's version replaces it locally.
3. A sets the title to "Plan final" at clock 15, also covering only its own seq 4.
4. After exchange, both replicas hold two title versions; neither context covers the other. The winner is greatest `(clock, writer)`: "Plan final" at clock 15. Title is surfaced, so both devices show N with a conflict listing both versions.
5. P keeps "Plan draft". The resolution writes "Plan draft" with a context covering both versions. It replaces both everywhere, and the conflict disappears on every device.

If P had changed the color instead, the two writes touch different groups. Both apply and no conflict exists.

### Delete against edit

1. A permanently deletes note N. The tombstone's context covers every version A had seen.
2. Offline, P edits N's body. P had not seen the deletion, so the tombstone does not cover P's body version.
3. On A, the body version arrives after the deletion and is retained on the tombstoned row. On P, the tombstone arrives after the edit, deletes N, and keeps the edit as retained state. Both replicas compute the same result: N is gone and is a recovery entry.
4. Either device restores it. A new note is created from the winning retained values, and a covering tombstone closes the entry on both devices.

Had P's edit been sealed and applied on A before the deletion, the tombstone would cover it and no recovery entry would exist.

### Duplicate tag name

1. Offline, A creates tag "Work" with id `t2`, and P creates tag "work" with id `t7`. Names compare case-insensitively.
2. After exchange, both tags are live in engine state. Only `t2`, the lower id, is materialized. Notes P tagged with `t7` resolve to `t2`, so every note keeps a "Work" tag.
3. A replica that can seal writes a tombstone for `t7` redirecting to `t2`. If both replicas write one concurrently, the redirects agree. The final state is one tag, `t2`, on both devices.

If P later renames `t7` concurrently with the repair, the rename is discarded with the merged tag; tag names are not recoverable.

### Concurrent reorder

1. Notes X, Y, and Z have keys `a0`, `a1`, `a2`.
2. Offline, A moves Z first: Z gets a key before `a0` at clock 20. P moves Z between X and Y: Z gets a key between `a0` and `a1` at clock 21.
3. Z's order group now has two concurrent versions. The winner is greatest `(clock, writer)`, P's key, so every replica shows X, Z, Y. Order is silent, so no conflict appears.
4. If A had moved X and P had moved Y into the same slot, both would get keys from the same neighbors and could be equal. They sort by id, identically on every replica, and nothing else moves.

### Project subtask depth and dependency cycles

1. Tasks T, U, and V are top-level. Offline, A makes T a subtask of U, and P makes U a subtask of V. Each write is valid alone because tasks nest one level deep.
2. After exchange, T sits under U and U under V, which is two levels. The tree repair detaches the deepest task, T, to the root, and a replica that can seal writes its parent as null. Every replica shows U under V and T at the root.
3. Separately, A adds the dependency "T blocks U" and P adds "U blocks T". Each is a derived-key row with its own key, so neither overwrites the other. Taken in ascending `(clock, writer)`, the later edge closes the cycle and is hidden. Projects shows it as removed to break a cycle. Nothing is sealed, so removing the other edge brings it back.

### Deleted status against move

1. A deletes status S while P, offline, moves task K into S.
2. Referenced settings rows are never tombstoned except with their project: A's delete sets `deleted_at`. The two writes touch different groups and both apply.
3. K stays in S. S is hidden from pickers and settings but still shown where K uses it, so no task points at a missing row.

### Fork re-seal

1. Desktop A's disk is cloned to a second machine C, including the writer key and record. Both continue with writer W after its seq 30.
2. C pushes W's seq 31. The hub already stores a different seq 31 from A and refuses with `fork`. A clone whose chain has the same length as the hub's is found through the probe in the hello request instead.
3. C fetches W's operation hashes from the hub and compares them with its own chain from seq 1. The chains agree through seq 30.
4. C creates successor writer W2, removes W's operations past seq 30 from its own merge state, and re-seals them under W2 with their original clocks, manifest versions, and causal contexts. Its local view of W stops at seq 30.
5. The hub keeps A's operations under W, and receives C's changes under W2. Every change from both machines survives and converges like any concurrent edits.

The re-seal is refused when an operation from another writer already depends on one of the operations being replaced, because renaming them would change what that writer saw.
