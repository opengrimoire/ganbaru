import {
  getNotesBlockFrontier,
  getNotesBlockOutlineFrontier,
  hydrateNotesBlocks,
  openNotesPage,
} from "$lib/api/notes";
import { buildNotesChildIdsByParent } from "$lib/notes/blocks/tree";
import type { NotesFocusRequest } from "$lib/notes/editor/focus";
import type {
  NotesBlock,
  NotesBlockOutline,
  NotesPage,
} from "$lib/notes/types";
import type { NotesBlockOutlineItem } from "$lib/notes/blocks/outline";

const BLOCK_VIRTUALIZATION_THRESHOLD = 120;
export const BLOCK_HYDRATION_LIMIT = 200;

interface NotesHydrationControllerContext {
  readPageGeneration: () => number;
  readSelectedPageId: () => string | null;
  readBlockOutlines: () => NotesBlockOutline[];
  readFlatBlockOutlines: () => NotesBlockOutlineItem[];
  readBlocksById: () => Record<string, NotesBlock>;
  hasLocalChanges: (blockId: string) => boolean;
  readFocusRequest: () => NotesFocusRequest;
  mergeBlockOutlines: (outlines: readonly NotesBlockOutline[], pageId: string) => void;
  replaceBlockOutlines: (outlines: readonly NotesBlockOutline[], pageId: string) => void;
  replaceHydratedBlocks: (
    blocksById: Record<string, NotesBlock>,
    childIdsByParentId: Record<string, string[]>,
  ) => void;
  setLoadError: (message: string) => void;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Coordinate Notes outline expansion and bounded block hydration. */
export function createNotesHydrationController(context: NotesHydrationControllerContext) {
  let hydrationRequestId = 0;
  let pendingHydration: { key: string; promise: Promise<void> } | null = null;

  function invalidate(): void {
    hydrationRequestId += 1;
    pendingHydration = null;
  }

  function hydrateBlockRange(blockIds: readonly string[], generation = context.readPageGeneration()): Promise<void> {
    const key = JSON.stringify([context.readSelectedPageId(), generation, context.readFocusRequest().blockId, blockIds]);
    if (pendingHydration?.key === key) return pendingHydration.promise;
    const promise = performHydration(blockIds, generation);
    pendingHydration = { key, promise };
    const clear = () => { if (pendingHydration?.promise === promise) pendingHydration = null; };
    void promise.then(clear, clear);
    return promise;
  }

  async function performHydration(
    blockIds: readonly string[],
    generation = context.readPageGeneration(),
  ): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId || generation !== context.readPageGeneration()) return;
    const outlines = context.readBlockOutlines();
    const outlinesById = new Map(outlines.map((outline) => [outline.id, outline]));
    const retained = new Set(blockIds);
    const focusBlockId = context.readFocusRequest().blockId;
    if (focusBlockId) retained.add(focusBlockId);
    for (const blockId of [...retained]) {
      let parent = outlinesById.get(blockId)?.parent;
      while (parent?.type === "block_id") {
        if (retained.has(parent.block_id)) break;
        retained.add(parent.block_id);
        parent = outlinesById.get(parent.block_id)?.parent;
      }
    }
    const boundedIds = [...retained];
    const currentBlocks = context.readBlocksById();
    const missingIds = boundedIds.filter((id) => !currentBlocks[id]);
    const retainedIds = new Set(boundedIds);
    const shouldPrune = context.readFlatBlockOutlines().length >= BLOCK_VIRTUALIZATION_THRESHOLD
      && Object.keys(currentBlocks).some((id) => !retainedIds.has(id) && !context.hasLocalChanges(id));
    if (missingIds.length === 0 && !shouldPrune) return;
    const requestId = ++hydrationRequestId;
    const hydrated: NotesBlock[] = [];
    for (let offset = 0; offset < missingIds.length; offset += BLOCK_HYDRATION_LIMIT) {
      hydrated.push(...await hydrateNotesBlocks({
        page_id: pageId, block_ids: missingIds.slice(offset, offset + BLOCK_HYDRATION_LIMIT),
      }));
      if (requestId !== hydrationRequestId || generation !== context.readPageGeneration()) return;
    }
    if (
      requestId !== hydrationRequestId
      || generation !== context.readPageGeneration()
      || pageId !== context.readSelectedPageId()
    ) return;
    if (hydrated.length === 0 && !shouldPrune) return;
    const nextBlocks = context.readFlatBlockOutlines().length >= BLOCK_VIRTUALIZATION_THRESHOLD
      ? Object.fromEntries(Object.entries(context.readBlocksById()).filter(([id]) => retainedIds.has(id) || context.hasLocalChanges(id)))
      : { ...context.readBlocksById() };
    for (const block of hydrated) {
      if (!context.hasLocalChanges(block.id)) nextBlocks[block.id] = block;
    }
    const ordered = context.readFlatBlockOutlines()
      .map((item) => nextBlocks[item.outline.id])
      .filter((block): block is NotesBlock => block !== undefined);
    const orderedIds = new Set(ordered.map((block) => block.id));
    ordered.push(...Object.values(nextBlocks).filter((block) => !orderedIds.has(block.id)));
    context.replaceHydratedBlocks(nextBlocks, buildNotesChildIdsByParent(ordered));
  }

  async function loadOutlineDescendantFrontiers(
    pageId: string,
    generation: number,
  ): Promise<void> {
    let frontier = context.readBlockOutlines()
      .filter((outline) => outline.has_children && outline.type !== "child_page")
      .map((outline) => outline.id);
    const visited = new Set<string>();
    while (frontier.length > 0) {
      const parentIds = frontier.filter((id) => !visited.has(id));
      if (parentIds.length === 0) break;
      parentIds.forEach((id) => visited.add(id));
      const children = await getNotesBlockOutlineFrontier(pageId, parentIds);
      if (generation !== context.readPageGeneration() || pageId !== context.readSelectedPageId()) return;
      context.mergeBlockOutlines(children, pageId);
      frontier = children
        .filter((outline) => outline.has_children && outline.type !== "child_page")
        .map((outline) => outline.id);
    }
    if (generation !== context.readPageGeneration() || pageId !== context.readSelectedPageId()) return;
    const flat = context.readFlatBlockOutlines();
    const initialIds = flat.length < BLOCK_VIRTUALIZATION_THRESHOLD
      ? flat.map((item) => item.outline.id)
      : flat.slice(0, 80).map((item) => item.outline.id);
    await hydrateBlockRange(initialIds, generation);
  }

  async function loadDescendantFrontiers(
    pageId = context.readSelectedPageId(),
    generation = context.readPageGeneration(),
  ): Promise<void> {
    if (!pageId) return;
    let frontier = Object.values(context.readBlocksById())
      .filter((block) => block.has_children && block.type !== "child_page")
      .map((block) => block.id);
    const visited = new Set<string>();
    while (frontier.length > 0) {
      const parentIds = frontier.filter((id) => !visited.has(id));
      if (parentIds.length === 0) return;
      parentIds.forEach((id) => visited.add(id));
      const { blocks } = await getNotesBlockFrontier(parentIds);
      if (generation !== context.readPageGeneration() || pageId !== context.readSelectedPageId()) return;
      if (blocks.length === 0) return;
      const nextBlocks = {
        ...context.readBlocksById(),
        ...Object.fromEntries(blocks.map((block) => [block.id, block])),
      };
      context.replaceHydratedBlocks(nextBlocks, buildNotesChildIdsByParent(Object.values(nextBlocks)));
      frontier = blocks
        .filter((block) => block.has_children && block.type !== "child_page")
        .map((block) => block.id);
    }
  }

  /**
   * Re-read the open page's outline and its loaded blocks from the database and replace every block without local
   * changes, keeping blocks the user is still editing. Callers run it between queued writes so no local write commits
   * meanwhile. Returns the canonical page, or null when the page, generation, or local outline changed while reading.
   */
  async function refreshFromCanonical(
    pageId: string,
    generation = context.readPageGeneration(),
  ): Promise<NotesPage | null> {
    const baseline = context.readBlockOutlines();
    const isCurrent = () => generation === context.readPageGeneration() && pageId === context.readSelectedPageId();
    const opened = await openNotesPage(pageId);
    if (!isCurrent()) return null;
    const outlines = [...opened.outlines];
    let frontier = outlines.filter((outline) => outline.has_children && outline.type !== "child_page").map((outline) => outline.id);
    const visited = new Set<string>();
    while (frontier.length > 0) {
      const parentIds = frontier.filter((id) => !visited.has(id));
      if (parentIds.length === 0) break;
      parentIds.forEach((id) => visited.add(id));
      const children = await getNotesBlockOutlineFrontier(pageId, parentIds);
      if (!isCurrent()) return null;
      outlines.push(...children);
      frontier = children.filter((outline) => outline.has_children && outline.type !== "child_page").map((outline) => outline.id);
    }
    if (context.readBlockOutlines() !== baseline) return null;
    const known = new Set(outlines.map((outline) => outline.id));
    const editing = context.readBlockOutlines().filter((outline) => !known.has(outline.id) && context.hasLocalChanges(outline.id));
    context.replaceBlockOutlines([...outlines, ...editing], pageId);
    const flat = context.readFlatBlockOutlines();
    const refreshIds = new Set(Object.keys(context.readBlocksById()).filter((id) => known.has(id)));
    if (flat.length < BLOCK_VIRTUALIZATION_THRESHOLD) for (const item of flat) refreshIds.add(item.outline.id);
    const ids = [...refreshIds].filter((id) => !context.hasLocalChanges(id));
    const fresh: NotesBlock[] = [];
    for (let offset = 0; offset < ids.length; offset += BLOCK_HYDRATION_LIMIT) {
      fresh.push(...await hydrateNotesBlocks({ page_id: pageId, block_ids: ids.slice(offset, offset + BLOCK_HYDRATION_LIMIT) }));
      if (!isCurrent()) return null;
    }
    const nextBlocks = Object.fromEntries(Object.entries(context.readBlocksById())
      .filter(([id]) => known.has(id) || context.hasLocalChanges(id)));
    for (const block of fresh) {
      if (!context.hasLocalChanges(block.id)) nextBlocks[block.id] = block;
    }
    const ordered = context.readFlatBlockOutlines()
      .map((item) => nextBlocks[item.outline.id])
      .filter((block): block is NotesBlock => block !== undefined);
    const orderedIds = new Set(ordered.map((block) => block.id));
    ordered.push(...Object.values(nextBlocks).filter((block) => !orderedIds.has(block.id)));
    context.replaceHydratedBlocks(nextBlocks, buildNotesChildIdsByParent(ordered));
    return opened.page;
  }

  function queueDescendantHydration(
    pageId = context.readSelectedPageId(),
    generation = context.readPageGeneration(),
  ): void {
    if (!pageId) return;
    void loadDescendantFrontiers(pageId, generation).catch((error) => {
      if (generation === context.readPageGeneration() && pageId === context.readSelectedPageId()) {
        context.setLoadError(errorMessage(error));
      }
    });
  }

  return {
    invalidate,
    hydrateBlockRange,
    loadOutlineDescendantFrontiers,
    loadDescendantFrontiers,
    refreshFromCanonical,
    queueDescendantHydration,
  };
}
