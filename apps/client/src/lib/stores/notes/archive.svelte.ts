import {
  listArchivedNotesPages,
  listTrashedNotesPages,
} from "$lib/api/notes";
import type { NotesPage } from "$lib/notes/types";

type NotesArchiveKind = "archive" | "trash";

interface NotesArchiveCollectionState {
  pages: NotesPage[];
  nextCursor: string | null;
  query: string;
  loaded: boolean;
  loading: boolean;
  error: string | null;
  requestId: number;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Own archive and trash query windows with independent stale-request guards. */
export function createNotesArchiveController() {
  let archive = $state<NotesArchiveCollectionState>({
    pages: [],
    nextCursor: null,
    query: "",
    loaded: false,
    loading: false,
    error: null,
    requestId: 0,
  });
  let trash = $state<NotesArchiveCollectionState>({
    pages: [],
    nextCursor: null,
    query: "",
    loaded: false,
    loading: false,
    error: null,
    requestId: 0,
  });

  function read(kind: NotesArchiveKind): NotesArchiveCollectionState {
    return kind === "archive" ? archive : trash;
  }

  function write(kind: NotesArchiveKind, state: NotesArchiveCollectionState): void {
    if (kind === "archive") archive = state;
    else trash = state;
  }

  async function list(
    kind: NotesArchiveKind,
    cursor: string | null,
    query: string,
  ) {
    const request = { cursor: cursor ?? undefined, query };
    return kind === "archive"
      ? listArchivedNotesPages(request)
      : listTrashedNotesPages(request);
  }

  async function reload(kind: NotesArchiveKind, query = ""): Promise<void> {
    const current = read(kind);
    const requestId = current.requestId + 1;
    const normalizedQuery = query.trim();
    write(kind, {
      ...current,
      query: normalizedQuery,
      loading: true,
      error: null,
      requestId,
    });
    try {
      const window = await list(kind, null, normalizedQuery);
      if (read(kind).requestId !== requestId) return;
      write(kind, {
        ...read(kind),
        pages: [...window.pages],
        nextCursor: window.next_cursor,
        loaded: true,
      });
    } catch (error) {
      if (read(kind).requestId !== requestId) return;
      write(kind, { ...read(kind), error: errorMessage(error) });
      throw error;
    } finally {
      if (read(kind).requestId === requestId) {
        write(kind, { ...read(kind), loading: false });
      }
    }
  }

  async function loadMore(kind: NotesArchiveKind): Promise<void> {
    const current = read(kind);
    if (!current.nextCursor || current.loading) return;
    const requestId = current.requestId + 1;
    const cursor = current.nextCursor;
    write(kind, { ...current, loading: true, error: null, requestId });
    try {
      const window = await list(kind, cursor, current.query);
      if (read(kind).requestId !== requestId) return;
      const pages = [
        ...new Map(
          [...read(kind).pages, ...window.pages].map((page) => [page.id, page]),
        ).values(),
      ];
      write(kind, { ...read(kind), pages, nextCursor: window.next_cursor });
    } catch (error) {
      // Report the failure like a reload while keeping the pages and cursor already loaded.
      if (read(kind).requestId === requestId) write(kind, { ...read(kind), error: errorMessage(error) });
    } finally {
      if (read(kind).requestId === requestId) {
        write(kind, { ...read(kind), loading: false });
      }
    }
  }

  function prepend(kind: NotesArchiveKind, page: NotesPage): void {
    const current = read(kind);
    if (!current.loaded) return;
    write(kind, {
      ...current,
      pages: [page, ...current.pages.filter((candidate) => candidate.id !== page.id)],
    });
  }

  function remove(kind: NotesArchiveKind, pageIds: ReadonlySet<string>): void {
    const current = read(kind);
    write(kind, {
      ...current,
      pages: current.pages.filter((page) => !pageIds.has(page.id)),
    });
  }

  return {
    get archivedPages(): NotesPage[] { return archive.pages; },
    get archiveHasMore(): boolean { return archive.nextCursor !== null; },
    get archiveLoaded(): boolean { return archive.loaded; },
    get archiveLoading(): boolean { return archive.loading; },
    get archiveError(): string | null { return archive.error; },
    get trashedPages(): NotesPage[] { return trash.pages; },
    get trashHasMore(): boolean { return trash.nextCursor !== null; },
    get trashLoaded(): boolean { return trash.loaded; },
    get trashLoading(): boolean { return trash.loading; },
    get trashError(): string | null { return trash.error; },
    reloadArchivedPages: (query = "") => reload("archive", query),
    loadMoreArchivedPages: () => loadMore("archive"),
    reloadTrashedPages: (query = "") => reload("trash", query),
    loadMoreTrashedPages: () => loadMore("trash"),
    prependArchivedPage: (page: NotesPage) => prepend("archive", page),
    prependTrashedPage: (page: NotesPage) => prepend("trash", page),
    removeArchivedPages: (pageIds: ReadonlySet<string>) => remove("archive", pageIds),
    removeTrashedPages: (pageIds: ReadonlySet<string>) => remove("trash", pageIds),
  };
}
