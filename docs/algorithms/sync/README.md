# Sync merge rules

**Status: Reference.** These rules are implemented in source for Quick notes, the first replicated domain. Delivery, writers, and mixed mode are owned by [Device linking and synchronization](../../data/sync.md); the reasons behind the design are in [Sync engine decisions](../../architecture/decisions/sync-engine.md).

## Model

- **Operation.** A signed, immutable record from one writer, identified by space, writer, and a sequence that is contiguous from 1. Each operation carries a hybrid logical clock and a causal context: for every writer, the highest sequence its author had applied when sealing.
- **Field group.** The unit of merging inside a row: one column, or several columns that must change together. A write changes whole groups.
- **Version.** One writer's value of one group of one row. A replica keeps at most one version per writer per group.
- **Supersession.** A new version removes exactly the versions its causal context covers. The remaining versions of a group are mutually concurrent.
- **Winner.** The version materialized into the domain table, chosen by the group's merge kind. Versions with equal value hashes are never a conflict.
- **Causal readiness.** An operation applies only after its writer's previous operation and every operation its context depends on. Validation depends only on the operation's bytes and manifest version, never on local state or arrival order.

The hybrid logical clock is 48 bits of Unix milliseconds and a 16-bit counter. A replica adopts remote clocks only up to one minute past its own time. The clock never decides what is superseded or kept, only which concurrent version is displayed.

## Merge kinds

| Kind | Winner among concurrent versions | Quick notes use |
| --- | --- | --- |
| Immutable | Set on create; concurrent creates keep the lowest `(clock, writer)` | `created_at` |
| Register | Greatest `(clock, writer)` | Title, body, color, tag name |
| Coupled register | Greatest `(priority, clock, writer)` over several columns that must stay consistent | Lifecycle: pinned, archived, and trash time, with priority trashed, then archived, then active |
| Max | Greatest value, then `(clock, writer)`; never a conflict | `updated_at` |
| Position | Greatest `(clock, writer)` over an order key; rows sort by `(order_key, id)` | Note and tag order |
| Reference | Greatest `(clock, writer)` over a row key that resolves through redirects | Note tag |
| Owned children | The parent group's value is a canonical list that replaces the child rows | Body text runs |

- A multi-column CHECK always lives inside one coupled register, so every materialized winner satisfies it.
- A register is surfaced when its concurrent distinct versions are shown to people as a conflict; otherwise it resolves silently. Quick note title and body are surfaced. Color, tag, lifecycle, order, and tag names are silent.
- A group is recoverable when edits to it that a deletion did not see should be offered for recovery. Quick note title, body, color, tag, and lifecycle are recoverable; order, timestamps, and every tag group are not.

## Order keys

Order keys are variable-length base62 strings compared by bytes. The first character encodes the length of an integer part, and an optional fraction follows that never ends in `0`. A key can always be generated before, after, or between any two distinct keys without changing other rows. Moving a row writes only that row's key.

When two rows end up with equal keys, which concurrent placement can cause, the row id breaks the tie. A later local placement between two equal neighbors re-keys only the run of rows equal to the lower neighbor. When keys would exceed their length bound, the local command re-keys the whole group, which is an ordinary write.

## Deletion and recovery

- A deletion is a tombstone carrying the causal context of its operation. A tombstoned row is deleted from the domain table, its key is never reused, and older operations never resurrect it.
- The tombstoned row keeps its versions as retained state. A retained version of a recoverable group that no tombstone's context covers is an edit the deletion did not see. Such a row is a recovery entry, regardless of whether the edit arrived before or after the deletion.
- Restoring creates a new row from the winning retained values. Restoring and discarding both seal a tombstone whose context covers every retained version, which closes the entry on every replica.
- A tombstone can redirect references to a row with a lower key. A reference to a tombstoned row follows the lowest redirect among its tombstones, or resolves to null when there is none. Redirects always point to lower keys, so chains terminate.

## Unique values

Live rows whose winning values collide on a declared unique column set form a class. Only the lowest key is materialized; the others are hidden, and references to them resolve to the kept row. Hiding is a pure function of replicated state, so every replica shows the same rows regardless of arrival order. A replica that can seal also writes repair tombstones for hidden rows that redirect to the kept row; concurrent repairs from several replicas are harmless. While materialization swaps unique values between rows, a value moves aside under a placeholder so the index never fails mid-batch.

## Conflicts

A conflict is a surfaced group of a live row with two or more versions whose values differ. Every replica derives the same conflicts from its merge state. Resolving writes the chosen value with a context that covers every alternative, even when the value is unchanged, so the resolution supersedes all of them and replicates like any edit. Keeping both keeps the displayed value and creates a new row from the alternative.

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

### Fork re-seal

1. Desktop A's disk is cloned to a second machine C, including the writer key and record. Both continue with writer W after its seq 30.
2. C pushes W's seq 31. The hub already stores a different seq 31 from A and refuses with `fork`. A clone whose chain has the same length as the hub's is found through the probe in the hello request instead.
3. C fetches W's operation hashes from the hub and compares them with its own chain from seq 1. The chains agree through seq 30.
4. C creates successor writer W2, removes W's operations past seq 30 from its own merge state, and re-seals them under W2 with their original clocks, manifest versions, and causal contexts. Its local view of W stops at seq 30.
5. The hub keeps A's operations under W, and receives C's changes under W2. Every change from both machines survives and converges like any concurrent edits.

The re-seal is refused when an operation from another writer already depends on one of the operations being replaced, because renaming them would change what that writer saw.
