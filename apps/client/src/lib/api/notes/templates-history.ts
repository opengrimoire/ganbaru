import { invoke } from "@tauri-apps/api/core";
import { publishNotesDatabaseChange } from "$lib/notes/database/window-sync";
import { ensureDbUrl } from "$lib/api/db";
import {
  mapNotesBlockListDto,
  mapNotesLoadedPageDto,
  mapNotesPageHistorySettingsDto,
  mapNotesPageHistorySnapshotDto,
  mapNotesPageTemplateDto,
} from "$lib/notes/validation/response-mappers";
import type {
  NotesLoadedPage,
  NotesPageHistoryCopyBlocksRequest,
  NotesPageHistorySettings,
  NotesPageHistorySettingsUpdate,
  NotesPageHistorySnapshot,
  NotesPageTemplate,
  NotesPageTemplateApplyRequest,
  NotesPageTemplateCreateFromPageRequest,
  NotesPageTemplateDuplicateRequest,
  NotesPageTemplateUpdateRequest,
  NotesPaginatedBlockList,
} from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";

export async function listNotesPageTemplates(): Promise<NotesPageTemplate[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_page_templates", { dbUrl });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_page_templates returned a non-array payload");
  }
  return rows.map(mapNotesPageTemplateDto);
}

export async function createNotesPageTemplateFromPage(
  request: NotesPageTemplateCreateFromPageRequest,
): Promise<NotesPageTemplate> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageTemplateDto(
    await invoke<unknown>("notes_create_page_template_from_page", { dbUrl, request }),
  );
}

export async function applyNotesPageTemplate(
  templateId: string,
  request: NotesPageTemplateApplyRequest,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  const applied = await invoke<unknown>("notes_apply_page_template", { dbUrl, templateId, request });
  publishNotesDatabaseChange();
  return mapNotesLoadedPageDto(applied);
}

export async function updateNotesPageTemplate(
  templateId: string,
  update: NotesPageTemplateUpdateRequest,
): Promise<NotesPageTemplate> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageTemplateDto(
    await invoke<unknown>("notes_update_page_template", { dbUrl, templateId, update }),
  );
}

export async function duplicateNotesPageTemplate(
  templateId: string,
  request: NotesPageTemplateDuplicateRequest,
): Promise<NotesPageTemplate> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageTemplateDto(
    await invoke<unknown>("notes_duplicate_page_template", { dbUrl, templateId, request }),
  );
}

export async function deleteNotesPageTemplate(templateId: string): Promise<string> {
  const dbUrl = await ensureDbUrl();
  const deletedTemplateId = await invoke<unknown>("notes_delete_page_template", { dbUrl, templateId });
  if (typeof deletedTemplateId !== "string") {
    throw new Error("notes_delete_page_template returned an invalid template id");
  }
  return deletedTemplateId;
}

export async function getNotesPageHistorySettings(): Promise<NotesPageHistorySettings> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageHistorySettingsDto(
    await invoke<unknown>("notes_load_page_history_settings", { dbUrl }),
  );
}

export async function updateNotesPageHistorySettings(
  update: NotesPageHistorySettingsUpdate,
): Promise<NotesPageHistorySettings> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageHistorySettingsDto(
    await invoke<unknown>("notes_update_page_history_settings", { dbUrl, update }),
  );
}

export async function listNotesPageHistorySnapshots(
  pageId: string,
): Promise<NotesPageHistorySnapshot[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_page_history_snapshots", { dbUrl, pageId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_page_history_snapshots returned a non-array payload");
  }
  return rows.map(mapNotesPageHistorySnapshotDto);
}

export async function loadNotesPageHistorySnapshot(
  pageId: string,
  snapshotId: string,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(
    await invoke<unknown>("notes_load_page_history_snapshot", { dbUrl, pageId, snapshotId }),
  );
}

export async function restoreNotesPageHistorySnapshot(
  pageId: string,
  snapshotId: string,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(
    await invokeNotesMutation(
      "notes_restore_page_history_snapshot",
      { dbUrl, pageId, snapshotId },
    ),
  );
}
export async function copyNotesPageHistoryBlocks(
  pageId: string,
  snapshotId: string,
  request: NotesPageHistoryCopyBlocksRequest,
): Promise<NotesPaginatedBlockList> {
  const dbUrl = await ensureDbUrl();
  return mapNotesBlockListDto(
    await invokeNotesMutation(
      "notes_copy_page_history_blocks",
      { dbUrl, pageId, snapshotId, request },
    ),
  );
}
