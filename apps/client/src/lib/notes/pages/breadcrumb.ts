import { notesPageTitle } from "./title";
import type { NotesPage, NotesPageBreadcrumbItem } from "$lib/notes/types";

export function buildNotesPageBreadcrumb(
  currentPage: NotesPage | null,
  pages: readonly NotesPage[],
  workspaceTitle: string,
  untitledTitle: string,
): NotesPageBreadcrumbItem[] {
  if (!currentPage) return [{ id: null, title: workspaceTitle, current: true, status: "workspace" }];
  const pagesById = new Map(pages.map((page) => [page.id, page]));
  pagesById.set(currentPage.id, currentPage);
  const chain: NotesPage[] = [];
  const seen = new Set<string>();
  let cursor: NotesPage | undefined = currentPage;
  while (cursor && !seen.has(cursor.id)) {
    seen.add(cursor.id);
    chain.unshift(cursor);
    cursor = cursor.parent.type === "page_id" ? pagesById.get(cursor.parent.page_id) : undefined;
  }
  return [
    { id: null, title: workspaceTitle, current: false, status: "workspace" },
    ...chain.map((page) => ({
      id: page.id,
      title: notesPageTitle(page, untitledTitle),
      current: page.id === currentPage.id,
      status: "active" as const,
    })),
  ];
}

function breadcrumbDisplayTitle(
  item: NotesPageBreadcrumbItem,
  untitledTitle: string,
  missingTitle: string,
): string {
  if (item.status === "missing") return missingTitle;
  return item.title.trim() || untitledTitle;
}

export function addNotesWorkspaceBreadcrumb(
  items: readonly NotesPageBreadcrumbItem[],
  workspaceTitle: string,
  untitledTitle: string,
  missingTitle: string,
): NotesPageBreadcrumbItem[] {
  return [
    { id: null, title: workspaceTitle, current: items.length === 0, status: "workspace" },
    ...items.map((item) => ({
      ...item,
      title: breadcrumbDisplayTitle(item, untitledTitle, missingTitle),
    })),
  ];
}
