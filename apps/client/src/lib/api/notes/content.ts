import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import {
  invalidateAssetUrlKind,
  invalidateNotesAssetUrls,
} from "$lib/api/asset-url-cache";
import { invalidateNotesNotificationSchedule } from "$lib/notes/collaboration/notification-schedule.svelte";
import {
  mapNotesBlockDto,
  mapNotesBlockListDto,
  mapNotesLoadedPageDto,
  mapNotesPageOpenResponseDto,
  mapNotesBlockFrontierDto,
  mapNotesBlockOutlineDto,
  mapNotesPageDto,
} from "$lib/notes/validation/response-mappers";
import type {
  NotesAppendBlockChildrenRequest,
  NotesBlock,
  NotesChildPageFromBlockCreate,
  NotesDuplicatePageRequest,
  NotesBlockUpdate,
  NotesLoadedPage,
  NotesPageOpenResponse,
  NotesBlockFrontier,
  NotesBlockOutline,
  NotesBlockHydrationRequest,
  NotesMovePageRequest,
  NotesMoveBlockRequest,
  NotesPage,
  NotesPageCreate,
  NotesPageUpdate,
  NotesPaginatedBlockList,
  NotesTrashBlocksRequest,
} from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";

export async function createNotesPage(page: NotesPageCreate): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(await invokeNotesMutation("notes_create_page", { dbUrl, page }));
}

export async function createNotesChildPageFromBlock(
  blockId: string,
  request: NotesChildPageFromBlockCreate,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(
    await invokeNotesMutation("notes_create_child_page_from_block", { dbUrl, blockId, request }),
  );
}

export async function duplicateNotesPage(
  pageId: string,
  request: NotesDuplicatePageRequest,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(await invokeNotesMutation("notes_duplicate_page", { dbUrl, pageId, request }));
}

export async function moveNotesPage(
  pageId: string,
  request: NotesMovePageRequest,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(await invokeNotesMutation("notes_move_page", { dbUrl, pageId, request }));
}

export async function updateNotesPage(
  pageId: string,
  update: NotesPageUpdate,
): Promise<NotesPage> {
  const dbUrl = await ensureDbUrl();
  const page = mapNotesPageDto(await invokeNotesMutation("notes_update_page", { dbUrl, pageId, update }));
  if (update.properties !== undefined || update.title !== undefined) invalidateNotesNotificationSchedule();
  return page;
}

export async function trashNotesPage(
  pageId: string,
  inTrash = true,
): Promise<NotesPage> {
  const dbUrl = await ensureDbUrl();
  const page = mapNotesPageDto(
    await invokeNotesMutation("notes_trash_page", { dbUrl, pageId, inTrash }),
  );
  if (inTrash) invalidateNotesAssetUrls();
  return page;
}

export async function archiveNotesPage(
  pageId: string,
  archived = true,
): Promise<NotesPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageDto(await invokeNotesMutation("notes_archive_page", { dbUrl, pageId, archived }));
}

export async function permanentlyDeleteNotesPage(pageId: string): Promise<string[]> {
  const dbUrl = await ensureDbUrl();
  const deletedPageIds = await invokeNotesMutation(
    "notes_permanently_delete_page",
    { dbUrl, pageId },
  );
  if (!Array.isArray(deletedPageIds)) {
    throw new Error("notes_permanently_delete_page returned a non-array payload");
  }
  if (!deletedPageIds.every((deletedPageId): deletedPageId is string => typeof deletedPageId === "string")) {
    throw new Error("notes_permanently_delete_page returned invalid page ids");
  }
  invalidateNotesAssetUrls();
  return deletedPageIds;
}

export async function loadNotesPage(pageId: string): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(await invoke<unknown>("notes_load_page", { dbUrl, pageId }));
}

export async function openNotesPage(pageId: string): Promise<NotesPageOpenResponse> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageOpenResponseDto(
    await invoke<unknown>("notes_open_page", { dbUrl, pageId }),
  );
}

export async function getNotesBlockFrontier(
  parentIds: readonly string[],
): Promise<NotesBlockFrontier> {
  const dbUrl = await ensureDbUrl();
  return mapNotesBlockFrontierDto(
    await invoke<unknown>("notes_block_frontier", { dbUrl, parentIds: [...parentIds] }),
  );
}

export async function getNotesBlockOutlineFrontier(
  pageId: string,
  parentIds: readonly string[],
): Promise<NotesBlockOutline[]> {
  const dbUrl = await ensureDbUrl();
  const value = await invoke<unknown>("notes_block_outline_frontier", {
    dbUrl,
    pageId,
    parentIds: [...parentIds],
  });
  if (!Array.isArray(value)) throw new Error("notes_block_outline_frontier returned a non-array payload");
  return value.map(mapNotesBlockOutlineDto);
}

export async function hydrateNotesBlocks(
  request: NotesBlockHydrationRequest,
): Promise<NotesBlock[]> {
  const dbUrl = await ensureDbUrl();
  const value = await invoke<unknown>("notes_hydrate_blocks", { dbUrl, request });
  if (!Array.isArray(value)) throw new Error("notes_hydrate_blocks returned a non-array payload");
  return value.map(mapNotesBlockDto);
}

export async function getNotesBlockChildren(
  parentId: string,
  startCursor: string | null = null,
  pageSize = 50,
): Promise<NotesPaginatedBlockList> {
  const dbUrl = await ensureDbUrl();
  return mapNotesBlockListDto(
    await invoke<unknown>("notes_block_children", {
      dbUrl,
      parentId,
      startCursor,
      pageSize,
    }),
  );
}

export async function appendNotesBlockChildren(
  request: NotesAppendBlockChildrenRequest,
): Promise<NotesPaginatedBlockList> {
  const dbUrl = await ensureDbUrl();
  const blocks = mapNotesBlockListDto(
    await invokeNotesMutation("notes_append_block_children", { dbUrl, request }),
  );
  invalidateNotesNotificationSchedule();
  return blocks;
}

export async function updateNotesBlock(
  blockId: string,
  update: NotesBlockUpdate,
): Promise<NotesBlock> {
  const dbUrl = await ensureDbUrl();
  const block = mapNotesBlockDto(
    await invokeNotesMutation("notes_update_block", { dbUrl, blockId, update }),
  );
  invalidateNotesNotificationSchedule();
  return block;
}

export async function trashNotesBlock(
  blockId: string,
  inTrash = true,
): Promise<NotesBlock> {
  const dbUrl = await ensureDbUrl();
  const block = mapNotesBlockDto(
    await invokeNotesMutation("notes_trash_block", { dbUrl, blockId, inTrash }),
  );
  if (inTrash) invalidateAssetUrlKind("notes-file");
  invalidateNotesNotificationSchedule();
  return block;
}

export async function trashNotesBlocks(
  request: NotesTrashBlocksRequest,
): Promise<NotesPaginatedBlockList> {
  const dbUrl = await ensureDbUrl();
  const blocks = mapNotesBlockListDto(
    await invokeNotesMutation("notes_trash_blocks", { dbUrl, request }),
  );
  if (request.in_trash) invalidateAssetUrlKind("notes-file");
  invalidateNotesNotificationSchedule();
  return blocks;
}

export async function moveNotesBlock(
  blockId: string,
  request: NotesMoveBlockRequest,
): Promise<NotesBlock> {
  const dbUrl = await ensureDbUrl();
  const block = mapNotesBlockDto(
    await invokeNotesMutation("notes_move_block", { dbUrl, blockId, request }),
  );
  invalidateNotesNotificationSchedule();
  return block;
}

export async function loadNotesUndoState(pageId: string): Promise<string | null> {
  const dbUrl = await ensureDbUrl();
  const state = await invoke<unknown>("notes_load_undo_state", { dbUrl, pageId });
  if (state === null) return null;
  if (typeof state !== "string") throw new Error("notes_load_undo_state returned invalid state");
  return state;
}

export async function saveNotesUndoState(
  pageId: string,
  stateJson: string,
): Promise<void> {
  const dbUrl = await ensureDbUrl();
  await invoke("notes_save_undo_state", { dbUrl, pageId, stateJson });
}

export async function clearNotesUndoState(pageId: string): Promise<void> {
  const dbUrl = await ensureDbUrl();
  await invoke("notes_clear_undo_state", { dbUrl, pageId });
}
