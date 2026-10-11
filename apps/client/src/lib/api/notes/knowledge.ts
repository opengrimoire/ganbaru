import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import {
  mapNotesBacklinkDto,
  mapNotesPageBreadcrumbItemDto,
  mapNotesPageAliasDto,
  mapNotesSearchResultDto,
  mapNotesUnresolvedLinkDto,
} from "$lib/notes/validation/response-mappers";
import { parseNotesMentionScope } from "$lib/notes/validation/knowledge";
import type {
  NotesBacklink,
  NotesMentionScope,
  NotesPageAlias,
  NotesPageAliasCreate,
  NotesPageBreadcrumbItem,
  NotesSearchWindow,
  NotesUnresolvedLink,
  NotesUnresolvedLinkResolve,
} from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";
import {
  mapPageSummary,
  notesWorkspaceShellRecord,
  shellNullableString,
} from "./workspace-parsing";

export async function listNotesBacklinks(pageId: string): Promise<NotesBacklink[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_backlinks", { dbUrl, pageId });
  if (!Array.isArray(rows)) throw new Error("notes_list_backlinks returned a non-array payload");
  return rows.map(mapNotesBacklinkDto);
}

/** Open a user-activated web or email link through the validating native Notes adapter. */
export async function openNotesExternalUrl(url: string): Promise<void> {
  await invoke<void>("notes_open_external_url", { url });
}

export async function listNotesPageAliases(pageId: string): Promise<NotesPageAlias[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_page_aliases", { dbUrl, pageId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_page_aliases returned a non-array payload");
  }
  return rows.map(mapNotesPageAliasDto);
}

export async function addNotesPageAlias(
  pageId: string,
  request: NotesPageAliasCreate,
): Promise<NotesPageAlias[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invokeNotesMutation("notes_add_page_alias", { dbUrl, pageId, request });
  if (!Array.isArray(rows)) {
    throw new Error("notes_add_page_alias returned a non-array payload");
  }
  return rows.map(mapNotesPageAliasDto);
}

export async function deleteNotesPageAlias(
  pageId: string,
  aliasId: string,
): Promise<NotesPageAlias[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invokeNotesMutation("notes_delete_page_alias", { dbUrl, pageId, aliasId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_delete_page_alias returned a non-array payload");
  }
  return rows.map(mapNotesPageAliasDto);
}

export async function listNotesUnresolvedLinks(pageId: string): Promise<NotesUnresolvedLink[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_unresolved_links", { dbUrl, pageId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_unresolved_links returned a non-array payload");
  }
  return rows.map(mapNotesUnresolvedLinkDto);
}

export async function resolveNotesUnresolvedLink(
  linkId: string,
  request: NotesUnresolvedLinkResolve,
): Promise<NotesUnresolvedLink[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invokeNotesMutation("notes_resolve_unresolved_link", {
    dbUrl,
    linkId,
    request,
  });
  if (!Array.isArray(rows)) {
    throw new Error("notes_resolve_unresolved_link returned a non-array payload");
  }
  return rows.map(mapNotesUnresolvedLinkDto);
}

export async function getNotesPageBreadcrumb(pageId: string): Promise<NotesPageBreadcrumbItem[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_page_breadcrumb", { dbUrl, pageId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_page_breadcrumb returned a non-array payload");
  }
  return rows.map(mapNotesPageBreadcrumbItemDto);
}

/**
 * Read where a page lives, outermost first and ending with its own title: project group, project, folders, then the
 * pages and row databases above it. Untitled pages appear as empty strings for the caller to label.
 */
export async function getNotesPageLocation(pageId: string): Promise<string[]> {
  const dbUrl = await ensureDbUrl();
  const segments = await invoke<unknown>("notes_page_location", { dbUrl, pageId });
  if (!Array.isArray(segments) || !segments.every((segment): segment is string => typeof segment === "string")) {
    throw new Error("notes_page_location returned an invalid payload");
  }
  return segments;
}

export async function searchNotes(
  query: string,
  pageSize = 20,
  includeResolvedComments = false,
  cursor: string | null = null,
): Promise<NotesSearchWindow> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_search", {
    dbUrl,
    query,
    pageSize,
    includeResolvedComments,
    cursor,
  });
  const record = notesWorkspaceShellRecord(rows, "notes_search response");
  if (!Array.isArray(record.results)) throw new Error("notes_search returned invalid results");
  return {
    results: record.results.map((value) => {
    const record = notesWorkspaceShellRecord(value, "notes_search result");
    return mapNotesSearchResultDto({ ...record, page: mapPageSummary(record.page) });
    }),
    next_cursor: shellNullableString(record.next_cursor, "notes_search.next_cursor"),
  };
}

/** Read the tasks and Calendar events a page may mention: those of the projects in its project group. */
export async function listNotesMentionScope(pageId: string): Promise<NotesMentionScope> {
  const dbUrl = await ensureDbUrl();
  return parseNotesMentionScope(await invoke<unknown>("notes_list_mention_scope", { dbUrl, pageId }));
}
