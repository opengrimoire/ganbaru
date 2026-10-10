const NOTES_LINK_HASH_PREFIX = "notes";
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/iu;

export interface NotesBlockLinkTarget {
  pageId: string;
  blockId: string;
}

/** A saved view of the database that a child database block shows. */
export interface NotesDatabaseViewLinkTarget extends NotesBlockLinkTarget {
  viewId: string;
}

export interface NotesPageLinkTarget {
  pageId: string;
  blockId?: string;
  /** Saved view to show when the linked block is a database. */
  viewId?: string;
}

export function isNotesUuid(value: unknown): value is string {
  return typeof value === "string" && UUID_PATTERN.test(value);
}

function assertNotesLinkTarget(target: NotesBlockLinkTarget): void {
  if (!isNotesUuid(target.pageId)) throw new Error("notes block link page id must be a UUID");
  if (!isNotesUuid(target.blockId)) throw new Error("notes block link block id must be a UUID");
}

function buildNotesLink(baseHref: string, params: Record<string, string>): string {
  const url = new URL(baseHref);
  url.searchParams.delete("ganbaruWindow");
  url.searchParams.set("view", "notes");
  url.hash = `${NOTES_LINK_HASH_PREFIX}?${new URLSearchParams(params).toString()}`;
  return url.toString();
}

export function buildNotesBlockLink(
  baseHref: string,
  target: NotesBlockLinkTarget,
): string {
  assertNotesLinkTarget(target);
  return buildNotesLink(baseHref, { page: target.pageId, block: target.blockId });
}

/** Build a link that opens a database block with one of its saved views selected. */
export function buildNotesDatabaseViewLink(
  baseHref: string,
  target: NotesDatabaseViewLinkTarget,
): string {
  assertNotesLinkTarget(target);
  if (!isNotesUuid(target.viewId)) throw new Error("notes database view link view id must be a UUID");
  return buildNotesLink(baseHref, { page: target.pageId, block: target.blockId, view: target.viewId });
}

export function buildNotesPageLink(
  baseHref: string,
  target: Pick<NotesPageLinkTarget, "pageId">,
): string {
  if (!isNotesUuid(target.pageId)) throw new Error("notes page link page id must be a UUID");
  return buildNotesLink(baseHref, { page: target.pageId });
}

export function parseNotesLinkHash(hash: string): NotesPageLinkTarget | null {
  const rawHash = hash.startsWith("#") ? hash.slice(1) : hash;
  if (!rawHash.startsWith(`${NOTES_LINK_HASH_PREFIX}?`)) return null;
  const params = new URLSearchParams(rawHash.slice(NOTES_LINK_HASH_PREFIX.length + 1));
  const pageId = params.get("page");
  const blockId = params.get("block");
  const viewId = params.get("view");
  if (!isNotesUuid(pageId)) return null;
  if (blockId !== null && !isNotesUuid(blockId)) return null;
  if (viewId !== null && (!blockId || !isNotesUuid(viewId))) return null;
  if (!blockId) return { pageId };
  return viewId ? { pageId, blockId, viewId } : { pageId, blockId };
}

export function parseNotesBlockLinkHash(hash: string): NotesBlockLinkTarget | null {
  const target = parseNotesLinkHash(hash);
  if (!target?.blockId) return null;
  return { pageId: target.pageId, blockId: target.blockId };
}

export function notesBlockAnchorId(blockId: string): string {
  return `notes-block-${blockId.replace(/[^A-Za-z0-9_-]/gu, "_")}`;
}
