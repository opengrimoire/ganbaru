import { invoke } from "@tauri-apps/api/core";
import { publishNotesDatabaseChange } from "$lib/notes/database/window-sync";
import { ensureDbUrl } from "$lib/api/db";
import type {
  NotesCommentDisplayName,
  NotesHistoricalPage,
  NotesHistoricalPageSummary,
  NotesHistoryRetentionImpact,
  NotesProjectHistoryRestorePlan,
  NotesProjectHistoryTree,
  NotesProjectHistoryVersion,
  NotesProjectHistoryVersionList,
} from "$lib/notes/types";
import type { NotesHistoryRetentionDays } from "$lib/notes/history/retention";

function record(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

function string(value: unknown, name: string): string {
  if (typeof value !== "string") throw new Error(`${name} must be a string`);
  return value;
}

function optionalString(value: unknown, name: string): string | null {
  return value === null ? null : string(value, name);
}

function number(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${name} must be a finite number`);
  }
  return value;
}

function boolean(value: unknown, name: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${name} must be a boolean`);
  return value;
}

export interface NotesProjectHistorySchedule {
  createdCount: number;
  nextCheckpointAt: string | null;
  nextMaintenanceAt: string;
}

function schedule(value: unknown): NotesProjectHistorySchedule {
  const parsed = record(value, "project history schedule");
  return {
    createdCount: number(parsed.createdCount, "project history schedule.createdCount"),
    nextCheckpointAt: optionalString(
      parsed.nextCheckpointAt,
      "project history schedule.nextCheckpointAt",
    ),
    nextMaintenanceAt: string(
      parsed.nextMaintenanceAt,
      "project history schedule.nextMaintenanceAt",
    ),
  };
}

function displayName(value: unknown): NotesCommentDisplayName {
  const parsed = record(value, "history display name");
  const type = string(parsed.type, "history display name.type");
  if (type !== "user" && type !== "integration" && type !== "custom") {
    throw new Error("history display name.type is unsupported");
  }
  return {
    type,
    resolved_name: string(parsed.resolved_name, "history display name.resolved_name"),
  };
}

function version(value: unknown): NotesProjectHistoryVersion {
  const parsed = record(value, "project history version");
  return {
    id: string(parsed.id, "project history version.id"),
    projectId: string(parsed.projectId, "project history version.projectId"),
    manifestHash: string(parsed.manifestHash, "project history version.manifestHash"),
    reason: string(parsed.reason, "project history version.reason"),
    createdBy: string(parsed.createdBy, "project history version.createdBy"),
    displayName: displayName(parsed.displayName),
    changedNoteSummary: string(
      parsed.changedNoteSummary,
      "project history version.changedNoteSummary",
    ),
    pageCount: number(parsed.pageCount, "project history version.pageCount"),
    activePageCount: number(
      parsed.activePageCount,
      "project history version.activePageCount",
    ),
    archivedPageCount: number(
      parsed.archivedPageCount,
      "project history version.archivedPageCount",
    ),
    deletedPageCount: number(
      parsed.deletedPageCount,
      "project history version.deletedPageCount",
    ),
    createdTime: string(parsed.createdTime, "project history version.createdTime"),
  };
}

function pageSummary(value: unknown): NotesHistoricalPageSummary {
  const parsed = record(value, "historical page summary");
  return {
    id: string(parsed.id, "historical page summary.id"),
    title: string(parsed.title, "historical page summary.title"),
    parentPageId: optionalString(
      parsed.parentPageId,
      "historical page summary.parentPageId",
    ),
    parentDataSourceId: optionalString(
      parsed.parentDataSourceId,
      "historical page summary.parentDataSourceId",
    ),
    inTrash: boolean(parsed.inTrash, "historical page summary.inTrash"),
    archived: boolean(parsed.archived, "historical page summary.archived"),
    icon: (parsed.icon ?? null) as NotesHistoricalPageSummary["icon"],
  };
}

export async function initializeNotesProjectHistory(
  projectId: string,
): Promise<NotesProjectHistoryVersion | null> {
  const dbUrl = await ensureDbUrl();
  const value = await invoke<unknown>("notes_initialize_project_history", { dbUrl, projectId });
  return value === null ? null : version(value);
}

export async function flushDueNotesProjectHistory(): Promise<NotesProjectHistorySchedule> {
  const dbUrl = await ensureDbUrl();
  return schedule(await invoke<unknown>("notes_flush_due_project_history", { dbUrl }));
}

export async function listNotesProjectHistoryVersions(args: {
  projectId: string;
  cursorTime?: string | null;
  cursorId?: string | null;
  pageSize?: number;
}): Promise<NotesProjectHistoryVersionList> {
  const dbUrl = await ensureDbUrl();
  const parsed = record(
    await invoke<unknown>("notes_list_project_history_versions", { dbUrl, ...args }),
    "project history version list",
  );
  if (!Array.isArray(parsed.versions)) {
    throw new Error("project history version list.versions must be an array");
  }
  return {
    versions: parsed.versions.map(version),
    nextCursorTime: optionalString(
      parsed.nextCursorTime,
      "project history version list.nextCursorTime",
    ),
    nextCursorId: optionalString(
      parsed.nextCursorId,
      "project history version list.nextCursorId",
    ),
  };
}

export async function loadNotesProjectHistoryTree(
  projectId: string,
  versionId: string,
): Promise<NotesProjectHistoryTree> {
  const dbUrl = await ensureDbUrl();
  const parsed = record(
    await invoke<unknown>("notes_load_project_history_tree", { dbUrl, projectId, versionId }),
    "project history tree",
  );
  if (!Array.isArray(parsed.pages)) throw new Error("project history tree.pages must be an array");
  return {
    version: version(parsed.version),
    pages: parsed.pages.map(pageSummary),
  };
}

export async function loadNotesProjectHistoryPage(
  projectId: string,
  versionId: string,
  pageId: string,
): Promise<NotesHistoricalPage> {
  const dbUrl = await ensureDbUrl();
  const parsed = record(
    await invoke<unknown>("notes_load_project_history_page", {
      dbUrl,
      projectId,
      versionId,
      pageId,
    }),
    "historical page",
  );
  function objectArray(value: unknown, name: string): Record<string, unknown>[] {
    if (!Array.isArray(value)) throw new Error(`${name} must be an array`);
    return value.map((item) => record(item, name));
  }
  return {
    id: string(parsed.id, "historical page.id"),
    title: string(parsed.title, "historical page.title"),
    properties: record(parsed.properties, "historical page.properties"),
    icon: (parsed.icon ?? null) as NotesHistoricalPage["icon"],
    cover: (parsed.cover ?? null) as NotesHistoricalPage["cover"],
    inTrash: boolean(parsed.inTrash, "historical page.inTrash"),
    archived: boolean(parsed.archived, "historical page.archived"),
    blocks: objectArray(parsed.blocks, "historical page.blocks"),
    databases: objectArray(parsed.databases, "historical page.databases"),
    dataSources: objectArray(parsed.dataSources, "historical page.dataSources"),
    databaseViews: objectArray(parsed.databaseViews, "historical page.databaseViews"),
  };
}

export async function getNotesHistoryRetentionImpact(
  retentionDays: NotesHistoryRetentionDays,
  projectId?: string,
): Promise<NotesHistoryRetentionImpact> {
  const dbUrl = await ensureDbUrl();
  const parsed = record(
    await invoke<unknown>("notes_history_retention_impact", {
      dbUrl,
      projectId: projectId ?? null,
      retentionDays,
    }),
    "history retention impact",
  );
  return {
    versionCount: number(parsed.versionCount, "history retention impact.versionCount"),
    storedBytes: number(parsed.storedBytes, "history retention impact.storedBytes"),
  };
}

export async function pruneNotesProjectHistory(projectId: string): Promise<number> {
  const dbUrl = await ensureDbUrl();
  return number(
    await invoke<unknown>("notes_prune_project_history", { dbUrl, projectId }),
    "pruned project history count",
  );
}

export async function previewNotesProjectHistoryRestore(
  projectId: string,
  versionId: string,
): Promise<NotesProjectHistoryRestorePlan> {
  const dbUrl = await ensureDbUrl();
  const parsed = record(
    await invoke<unknown>("notes_preview_project_history_restore", {
      dbUrl,
      projectId,
      versionId,
    }),
    "project history restore plan",
  );
  return {
    versionId: string(parsed.versionId, "project history restore plan.versionId"),
    removeCount: number(parsed.removeCount, "project history restore plan.removeCount"),
    recreateCount: number(parsed.recreateCount, "project history restore plan.recreateCount"),
    changeCount: number(parsed.changeCount, "project history restore plan.changeCount"),
    copyCount: number(parsed.copyCount, "project history restore plan.copyCount"),
    safetyVersionWillBeCreated: boolean(
      parsed.safetyVersionWillBeCreated,
      "project history restore plan.safetyVersionWillBeCreated",
    ),
  };
}

export async function restoreNotesProjectHistoryVersion(
  projectId: string,
  versionId: string,
): Promise<NotesProjectHistoryVersion> {
  const dbUrl = await ensureDbUrl();
  const restored = await invoke<unknown>("notes_restore_project_history_version", { dbUrl, projectId, versionId });
  publishNotesDatabaseChange();
  return version(restored);
}
