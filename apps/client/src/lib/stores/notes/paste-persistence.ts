import type { NotesEditOperation } from "$lib/api/notes/compound-edits";
import type { NotesAppendBlockChildrenRequest } from "$lib/notes/types";

/** Plan a paste in source order for one native transaction, including owned graph copies. */
export function notesPasteOperations(
  requests: readonly NotesAppendBlockChildrenRequest[],
  copiedPageIds: Readonly<Record<string, string>> = {},
  copiedDatabaseIds: Readonly<Record<string, string>> = {},
): NotesEditOperation[] {
  const operations: NotesEditOperation[] = [];
  for (const request of requests) {
    let after = request.after ?? null;
    let children: NotesAppendBlockChildrenRequest["children"] = [];
    const flush = (): void => {
      if (!children.length) return;
      operations.push({ type: "append", request: { parent: request.parent, after, children } });
      after = children.at(-1)!.id;
      children = [];
    };
    for (const child of request.children) {
      const databaseId = copiedDatabaseIds[child.id];
      if (databaseId) {
        flush();
        operations.push({ type: "copy_database", request: { id: child.id, source_block_id: databaseId, parent: request.parent, after_block_id: after } });
        after = child.id;
        continue;
      }
      const sourceId = copiedPageIds[child.id];
      if (!sourceId) { children.push(child); continue; }
      flush();
      operations.push({ type: "duplicate", request: {
        block_ids: [sourceId], duplicated_block_ids: [{ source_id: sourceId, duplicate_id: child.id }],
        parent: request.parent, after, before: null, include_trashed_sources: true,
      } });
      after = child.id;
    }
    flush();
  }
  return operations;
}
