import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import {
  mapNotesHtmlArchiveSaveDto,
  mapNotesHtmlImportDto,
  mapNotesAgentBridgeExportSaveDto,
  mapNotesJsonGraphExportSaveDto,
  mapNotesNotionApiImportDto,
  mapNotesNotionExportImportDto,
} from "$lib/notes/validation/response-mappers";
import type {
  NotesHtmlArchiveSaveResult,
  NotesHtmlExportRequest,
  NotesAgentBridgeExportRequest,
  NotesAgentBridgeExportSaveResult,
  NotesJsonGraphExportRequest,
  NotesJsonGraphExportSaveResult,
  NotesHtmlImportRequest,
  NotesHtmlImportResult,
  NotesNotionApiImportRequest,
  NotesNotionApiImportResult,
  NotesNotionExportImportRequest,
  NotesNotionExportImportResult,
} from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";

export async function importNotesHtmlPage(
  request: NotesHtmlImportRequest,
): Promise<NotesHtmlImportResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesHtmlImportDto(
    await invokeNotesMutation("notes_import_html_page", { dbUrl, request }),
  );
}

export async function importNotesNotionApi(
  request: NotesNotionApiImportRequest,
): Promise<NotesNotionApiImportResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesNotionApiImportDto(
    await invokeNotesMutation("notes_import_notion_api", { dbUrl, request }),
  );
}

/**
 * Stops a running Notion API import started with `import_id`; sources finished before the stop
 * stay imported. Resolves `false` when no import with that ID is running.
 */
export async function cancelNotesNotionApiImport(importId: string): Promise<boolean> {
  const value = await invoke<unknown>("notes_cancel_notion_api_import", { importId });
  if (typeof value !== "boolean") {
    throw new Error("Notion import stop returned an invalid response");
  }
  return value;
}

/** Pick a Notion export folder natively and import it; `null` means the picker was canceled. */
export async function importNotesNotionExportFolder(
  request: NotesNotionExportImportRequest,
): Promise<NotesNotionExportImportResult | null> {
  const dbUrl = await ensureDbUrl();
  const value = await invokeNotesMutation("notes_import_notion_export_folder", { dbUrl, request });
  return value === null ? null : mapNotesNotionExportImportDto(value);
}

export async function saveNotesHtmlArchive(
  request: NotesHtmlExportRequest,
): Promise<NotesHtmlArchiveSaveResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesHtmlArchiveSaveDto(
    await invoke<unknown>("notes_pick_and_write_html_archive", { dbUrl, request }),
  );
}

export async function saveNotesJsonGraph(
  request: NotesJsonGraphExportRequest,
): Promise<NotesJsonGraphExportSaveResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesJsonGraphExportSaveDto(
    await invoke<unknown>("notes_pick_and_write_json_graph", { dbUrl, request }),
  );
}

export async function saveNotesAgentBridge(
  request: NotesAgentBridgeExportRequest,
): Promise<NotesAgentBridgeExportSaveResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesAgentBridgeExportSaveDto(
    await invoke<unknown>("notes_pick_and_write_agent_bridge", { dbUrl, request }),
  );
}
