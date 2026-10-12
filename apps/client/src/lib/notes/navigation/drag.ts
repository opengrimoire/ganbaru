import { notesPageProjectId } from "$lib/notes/project-membership";
import type { NotesFolder, NotesPage } from "$lib/notes/types";

export type NotesNavigationDragItem =
  | { kind: "folder"; id: string }
  | { kind: "page"; id: string };

export type NotesNavigationDropTarget =
  | { kind: "root" }
  | { kind: "folder"; id: string }
  | { kind: "page"; id: string };

/** Serialize a Notes navigation drag item for the browser data-transfer boundary. */
export function serializeNotesNavigationDragItem(item: NotesNavigationDragItem): string {
  return JSON.stringify(item);
}

/** Parse and validate a Notes navigation drag item from an unknown transfer value. */
export function parseNotesNavigationDragItem(value: string): NotesNavigationDragItem | null {
  try {
    const parsed: unknown = JSON.parse(value);
    if (!parsed || typeof parsed !== "object") return null;
    const record = parsed as Record<string, unknown>;
    if ((record.kind !== "folder" && record.kind !== "page") || typeof record.id !== "string") {
      return null;
    }
    const id = record.id.trim();
    return id ? { kind: record.kind, id } : null;
  } catch {
    return null;
  }
}

function descendantPageIds(pages: readonly NotesPage[], sourcePageId: string): Set<string> {
  const childrenByParentId = new Map<string, string[]>();
  for (const page of pages) {
    if (page.parent.type !== "page_id") continue;
    const children = childrenByParentId.get(page.parent.page_id) ?? [];
    children.push(page.id);
    childrenByParentId.set(page.parent.page_id, children);
  }
  const descendants = new Set<string>();
  const pending = [...(childrenByParentId.get(sourcePageId) ?? [])];
  while (pending.length > 0) {
    const pageId = pending.shift();
    if (!pageId || descendants.has(pageId)) continue;
    descendants.add(pageId);
    pending.push(...(childrenByParentId.get(pageId) ?? []));
  }
  return descendants;
}

function descendantFolderIds(
  folders: readonly NotesFolder[],
  sourceFolderId: string,
): Set<string> {
  const childrenByParentId = new Map<string, string[]>();
  for (const folder of folders) {
    if (!folder.parent_folder_id) continue;
    const children = childrenByParentId.get(folder.parent_folder_id) ?? [];
    children.push(folder.id);
    childrenByParentId.set(folder.parent_folder_id, children);
  }
  const descendants = new Set<string>();
  const pending = [...(childrenByParentId.get(sourceFolderId) ?? [])];
  while (pending.length > 0) {
    const folderId = pending.shift();
    if (!folderId || descendants.has(folderId)) continue;
    descendants.add(folderId);
    pending.push(...(childrenByParentId.get(folderId) ?? []));
  }
  return descendants;
}

/** Return whether a navigation item can be moved onto the requested destination. */
export function canDropNotesNavigationItem(
  source: NotesNavigationDragItem,
  target: NotesNavigationDropTarget,
  pages: readonly NotesPage[],
  folders: readonly NotesFolder[],
): boolean {
  if (source.kind === "folder") {
    const sourceFolder = folders.find((folder) => folder.id === source.id);
    if (!sourceFolder || target.kind === "page") return false;
    if (target.kind === "root") return sourceFolder.parent_folder_id !== null;
    const targetFolder = folders.find((folder) => folder.id === target.id);
    if (!targetFolder || targetFolder.project_id !== sourceFolder.project_id) return false;
    if (targetFolder.id === sourceFolder.id || sourceFolder.parent_folder_id === targetFolder.id) {
      return false;
    }
    return !descendantFolderIds(folders, sourceFolder.id).has(targetFolder.id);
  }

  const sourcePage = pages.find((page) => page.id === source.id);
  if (!sourcePage) return false;
  if (target.kind === "root") {
    return sourcePage.parent.type !== "workspace" || sourcePage.folder_id !== null;
  }
  if (target.kind === "folder") {
    const targetFolder = folders.find((folder) => folder.id === target.id);
    if (!targetFolder || targetFolder.project_id !== notesPageProjectId(sourcePage)) return false;
    return sourcePage.parent.type !== "workspace" || sourcePage.folder_id !== targetFolder.id;
  }
  const targetPage = pages.find((page) => page.id === target.id);
  if (!targetPage || notesPageProjectId(targetPage) !== notesPageProjectId(sourcePage)) return false;
  if (sourcePage.id === targetPage.id) return false;
  if (sourcePage.parent.type === "page_id" && sourcePage.parent.page_id === targetPage.id) {
    return false;
  }
  return !descendantPageIds(pages, sourcePage.id).has(targetPage.id);
}
