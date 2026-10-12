import { applyNotesCompoundEdit, type NotesCompoundEdit, type NotesCompoundEditResult, type NotesEditKind, type NotesEditOperation } from "$lib/api/notes/compound-edits";
import { cloneNotesJson } from "$lib/notes/json-clone";
import type { NotesBlock, NotesParent } from "$lib/notes/types";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";
import { applyBlockUpdate } from "$lib/notes/blocks/factory";

export interface NotesCompoundEditContext {
  readSelectedPageId: () => string | null;
  blockById: (id: string) => NotesBlock | undefined;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  reconcileCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
  reconcileCompoundUndo?: (result: NotesCompoundEditResult) => void;
  readCanonicalRevision?: (id: string) => string | undefined;
  readBlocksById?: () => Readonly<Record<string, NotesBlock>>;
}

/** Identify persisted preconditions while excluding identities created by this action. */
export function notesEditReferences(operations: readonly NotesEditOperation[]): string[] {
  const references = new Set<string>();
  const created = new Set<string>();
  const reference = (id: string | null | undefined): void => { if (id && !created.has(id)) references.add(id); };
  const destination = (parent: NotesParent | null | undefined, anchor?: string | null): void => {
    if (parent?.type === "block_id") reference(parent.block_id);
    reference(anchor);
  };
  for (const operation of operations) {
    switch (operation.type) {
      case "table_column": reference(operation.table_id); break;
      case "move_children": reference(operation.source_block_id); destination(operation.parent, operation.after); break;
      case "update": case "trash": reference(operation.block_id); break;
      case "move": case "move_between_pages": reference(operation.block_id); destination(operation.request.parent, operation.request.after ?? operation.request.before); break;
      case "append":
        destination(operation.request.parent, operation.request.after);
        for (const child of operation.request.children) created.add(child.id);
        break;
      case "duplicate": case "duplicate_children":
        destination(operation.request.parent, operation.request.after ?? operation.request.before);
        for (const pair of operation.request.duplicated_block_ids) created.add(pair.duplicate_id);
        break;
      case "create_database": case "copy_database": case "link_database":
        destination(operation.request.parent, operation.request.after_block_id);
        reference(operation.request.replace_block_id);
        created.add(operation.request.id);
        break;
    }
  }
  return [...references];
}

/** Retain an immutable native request across uncertain responses and queued retries. */
export function createNotesCompoundPersistence(
  context: NotesCompoundEditContext,
  kind: NotesEditKind,
  operations: readonly NotesEditOperation[],
  knownBlocks: readonly NotesBlock[] = [],
  refreshOperations?: () => NotesEditOperation[],
): (() => Promise<NotesCompoundEditResult>) & { operationId: string } {
  const pageId = context.readSelectedPageId();
  if (!pageId) throw new Error("Notes edit requires an active page");
  let frozenOperations = cloneNotesJson([...operations]);
  let references = notesEditReferences(frozenOperations);
  const original = new Map([...Object.values(context.readBlocksById?.() ?? {}), ...knownBlocks].map((block) => [block.id, block]));
  for (const id of references) {
    const block = context.blockById(id);
    if (block) original.set(id, block);
  }
  const operationId = crypto.randomUUID();
  let request: NotesCompoundEdit | null = null;
  const presented = new Map<string, NotesBlock>();
  for (const id of new Set([...original.keys(), ...references, ...frozenOperations.flatMap((operation) => operation.type === "append" ? operation.request.children.map((child) => child.id) : [])])) {
    const block = context.blockById(id);
    if (block) presented.set(id, block);
  }
  const persist = async () => {
    if (!request) {
      if (refreshOperations) {
        frozenOperations = cloneNotesJson(refreshOperations());
        references = notesEditReferences(frozenOperations);
      }
      const expected: Record<string, string> = {};
      for (const id of references) {
        const revision = context.readCanonicalRevision?.(id) ?? context.blockById(id)?.edit_revision ?? original.get(id)?.edit_revision;
        if (!revision) throw new Error(`Notes edit requires loaded canonical block ${id}`);
        expected[id] = revision;
      }
      request = { operation_id: operationId, page_id: pageId, kind, expected_blocks: expected, operations: frozenOperations };
    }
    const result = await applyNotesCompoundEdit(request);
    const accepted = result.blocks.filter((saved) => {
      const expected = request!.expected_blocks[saved.id] ?? presented.get(saved.id)?.edit_revision ?? original.get(saved.id)?.edit_revision;
      const current = context.readCanonicalRevision?.(saved.id) ?? context.blockById(saved.id)?.edit_revision;
      return current === undefined || current === expected || current === saved.edit_revision;
    });
    context.reconcileCanonicalBlocks?.(accepted);
    context.reconcileCompoundUndo?.(result);
    const reconciled = new Map<string, NotesBlock>();
    if (context.readSelectedPageId() === pageId) {
      // Preserve typing queued after this action and reject old receipt revisions after a refresh.
      const blocks = accepted.flatMap((saved) => {
        const current = context.blockById(saved.id);
        const expected = request!.expected_blocks[saved.id] ?? presented.get(saved.id)?.edit_revision;
        if (!current || (current.edit_revision && current.edit_revision !== expected && current.edit_revision !== saved.edit_revision)) return [];
        const block = current === presented.get(saved.id) ? saved : { ...current, edit_revision: saved.edit_revision };
        reconciled.set(block.id, block);
        return [block];
      });
      if (blocks.length) context.applyPostMutation({ blocks, canonical: true });
    }
    const acceptedIds = new Set(accepted.map((block) => block.id));
    return {
      ...result,
      blocks: accepted.map((block) => reconciled.get(block.id) ?? block),
      placements: result.placements.filter((placement) => acceptedIds.has(placement.blockId)),
      databases: result.databases.filter((database) => acceptedIds.has(database.block.id)),
    };
  };
  return Object.assign(persist, { operationId });
}

/** Apply a bounded structural plan immediately, then queue its single canonical commit. */
export function enqueueNotesCompoundEdit(
  context: NotesCompoundEditContext & { enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void> },
  kind: NotesEditKind,
  operations: readonly NotesEditOperation[],
): Promise<void> {
  const persist = createNotesCompoundPersistence(context, kind, operations);
  for (const operation of operations) {
    if (operation.type === "update") {
      const block = context.blockById(operation.block_id);
      if (block) context.applyPostMutation({ blocks: [applyBlockUpdate(block, operation.update)] });
    } else if (operation.type === "append") {
      let after = operation.request.after ?? null;
      for (const write of operation.request.children) {
        const now = new Date().toISOString();
        const block = { object: "block", parent: operation.request.parent, created_time: now, last_edited_time: now, has_children: false, in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null, ...write } as NotesBlock;
        context.applyPostMutation({ blocks: [block], placements: [{ blockId: write.id, parent: operation.request.parent, after }] });
        after = write.id;
      }
    } else if (operation.type === "move") {
      const block = context.blockById(operation.block_id);
      if (block) context.applyPostMutation({ blocks: [{ ...block, parent: operation.request.parent }], placements: [{ blockId: block.id, ...operation.request }] });
    } else if (operation.type === "trash" && operation.in_trash) {
      context.applyPostMutation({ removedBlockIds: [operation.block_id] });
    }
  }
  return context.enqueueEditorMutation(async () => { await persist(); });
}
