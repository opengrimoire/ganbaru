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

/** Every archived page across projects, read whole for the hierarchy pickers while their eye button is on. */
interface NotesPickerArchiveState {
  shown: boolean;
  pages: NotesPage[];
  loaded: boolean;
  loading: boolean;
  error: string | null;
  requestId: number;
}

/** Largest summary window the archive read accepts, so the pickers need as few reads as possible. */
const PICKER_ARCHIVE_WINDOW_SIZE = 100;

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

  let pickerArchive = $state<NotesPickerArchiveState>({
    shown: false,
    pages: [],
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

  /** Read every archived page window for the pickers, ignoring a load that a newer one replaced. */
  async function reloadPickerArchive(): Promise<void> {
    const requestId = pickerArchive.requestId + 1;
    pickerArchive = { ...pickerArchive, loading: true, error: null, requestId };
    try {
      const pages: NotesPage[] = [];
      let cursor: string | null = null;
      do {
        const window = await listArchivedNotesPages({ cursor, page_size: PICKER_ARCHIVE_WINDOW_SIZE });
        if (pickerArchive.requestId !== requestId) return;
        pages.push(...window.pages);
        cursor = window.next_cursor;
      } while (cursor !== null);
      pickerArchive = { ...pickerArchive, pages: [...new Map(pages.map((page) => [page.id, page])).values()], loaded: true };
    } catch (error) {
      if (pickerArchive.requestId === requestId) pickerArchive = { ...pickerArchive, error: errorMessage(error) };
    } finally {
      if (pickerArchive.requestId === requestId) pickerArchive = { ...pickerArchive, loading: false };
    }
  }

  function prependPickerArchive(page: NotesPage): void {
    if (!pickerArchive.loaded) return;
    pickerArchive = { ...pickerArchive, pages: [page, ...pickerArchive.pages.filter((candidate) => candidate.id !== page.id)] };
  }

  function removePickerArchive(pageIds: ReadonlySet<string>): void {
    pickerArchive = { ...pickerArchive, pages: pickerArchive.pages.filter((page) => !pageIds.has(page.id)) };
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
    get showArchivedInPickers(): boolean { return pickerArchive.shown; },
    /** Turning the picker eye on reads the archive again so pickers never list a stale archive. */
    set showArchivedInPickers(shown: boolean) {
      pickerArchive = { ...pickerArchive, shown };
      if (shown) void reloadPickerArchive();
    },
    get pickerArchivedPages(): NotesPage[] { return pickerArchive.pages; },
    get pickerArchiveLoading(): boolean { return pickerArchive.loading; },
    get pickerArchiveError(): string | null { return pickerArchive.error; },
    reloadPickerArchivedPages: reloadPickerArchive,
    prependArchivedPage: (page: NotesPage) => {
      prepend("archive", page);
      prependPickerArchive(page);
    },
    prependTrashedPage: (page: NotesPage) => prepend("trash", page),
    removeArchivedPages: (pageIds: ReadonlySet<string>) => {
      remove("archive", pageIds);
      removePickerArchive(pageIds);
    },
    removeTrashedPages: (pageIds: ReadonlySet<string>) => remove("trash", pageIds),
  };
}
