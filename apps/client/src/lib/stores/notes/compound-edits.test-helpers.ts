import type { NotesCompoundEdit, NotesCompoundEditResult, NotesEditOperation } from "$lib/api/notes/compound-edits";
import type { NotesAppendBlockChildrenRequest, NotesBlock, NotesBlockUpdate, NotesCreatedDatabase, NotesDatabaseCreateRequest, NotesLinkedDatabaseCreateRequest, NotesMoveBlockRequest } from "$lib/notes/types";

interface NotesEditTestAdapters {
  updateNotesBlock?: (id: string, update: NotesBlockUpdate) => Promise<unknown>;
  appendNotesBlockChildren?: (request: NotesAppendBlockChildrenRequest) => Promise<unknown>;
  trashNotesBlock?: (id: string, inTrash: boolean) => Promise<unknown>;
  moveNotesBlock?: (id: string, request: NotesMoveBlockRequest) => Promise<unknown>;
  createNotesDatabase?: (request: NotesDatabaseCreateRequest) => Promise<unknown>;
  createNotesLinkedDatabaseView?: (request: NotesLinkedDatabaseCreateRequest) => Promise<unknown>;
}

/** Adapt per-operation delayed storage fixtures to the single-command compound edit boundary. */
export function createNotesCompoundTestAdapter(adapters: NotesEditTestAdapters, readBlocks: () => readonly NotesBlock[] = () => [], restoreBlocks?: (blocks: readonly NotesBlock[]) => void) {
  const receipts = new Map<string, NotesCompoundEditResult>();
  return async (request: NotesCompoundEdit): Promise<NotesCompoundEditResult> => {
    const receipt = receipts.get(request.operation_id);
    if (receipt) return receipt;
    const changed = new Map<string, NotesBlock>();
    const databases: NotesCreatedDatabase[] = [];
    const placements: NotesCompoundEditResult["placements"] = [];
    const original = [...readBlocks()];
    const accept = (value: unknown): void => {
      if (typeof value !== "object" || value === null || Array.isArray(value)) return;
      const record = value as Record<string, unknown>;
      if (record.object === "block") changed.set(String(record.id), value as NotesBlock);
      if (Array.isArray(record.results)) for (const block of record.results) accept(block);
      if (record.block && typeof record.block === "object") {
        accept(record.block);
        databases.push(value as NotesCreatedDatabase);
      }
    };
    try {
      for (const operation of request.operations) {
      let value: unknown;
      switch (operation.type) {
        case "update": value = await adapters.updateNotesBlock?.(operation.block_id, operation.update); break;
        case "trash": value = await adapters.trashNotesBlock?.(operation.block_id, operation.in_trash); break;
        case "move": case "move_between_pages": value = await adapters.moveNotesBlock?.(operation.block_id, operation.request); placements.push({ blockId: operation.block_id, ...operation.request, after: operation.request.after ?? null }); break;
        case "append": {
          value = await adapters.appendNotesBlockChildren?.(operation.request);
          let after = operation.request.after ?? null;
          for (const child of operation.request.children) {
            placements.push({ blockId: child.id, parent: operation.request.parent, after });
            after = child.id;
          }
          break;
        }
        case "create_database": value = await adapters.createNotesDatabase?.(operation.request); break;
        case "link_database": {
          value = await adapters.createNotesLinkedDatabaseView?.(operation.request);
          if (operation.request.parent) placements.push({ blockId: operation.request.id, parent: operation.request.parent, after: operation.request.after_block_id ?? null });
          break;
        }
        case "move_children": {
          let after = operation.after;
          for (const block of readBlocks().filter((block) => block.parent.type === "block_id" && block.parent.block_id === operation.source_block_id && !block.in_trash)) {
            accept(await adapters.moveNotesBlock?.(block.id, { parent: operation.parent, after, before: null }));
            placements.push({ blockId: block.id, parent: operation.parent, after });
            after = block.id;
          }
          break;
        }
        default: throw new Error(`No frontend fixture adapter for ${operation.type satisfies NotesEditOperation["type"]}`);
      }
      accept(value);
      }
    } catch (error) {
      restoreBlocks?.(original);
      throw error;
    }
    const removed = new Set(request.operations.flatMap((operation) => operation.type === "trash" && operation.in_trash ? [operation.block_id] : []));
    for (const placement of placements) {
      if (!placement.after || !removed.has(placement.after)) continue;
      const index = original.findIndex((block) => block.id === placement.after);
      placement.after = original.slice(0, index).reverse().find((block) => !removed.has(block.id))?.id ?? null;
      if (!placement.after) placement.before = original.slice(index + 1).find((block) => !removed.has(block.id))?.id ?? null;
    }
    const result = { operation_id: request.operation_id, page_id: request.page_id, blocks: [...changed.values()], databases, placements };
    receipts.set(request.operation_id, result);
    return result;
  };
}
