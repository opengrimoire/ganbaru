import { searchNotes } from "$lib/api/notes";
import type { NotesSearchResult } from "$lib/notes/types";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Own Notes search pagination and stale-result rejection. */
export function createNotesSearchController() {
  let results = $state<NotesSearchResult[]>([]);
  let nextCursor = $state<string | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let includeResolvedComments = $state(false);
  let activeQuery = "";
  let activePageSize = 20;
  let activeIncludeResolved = false;
  let requestId = 0;

  async function search(
    query: string,
    pageSize = 20,
    includeResolved = includeResolvedComments,
  ): Promise<void> {
    const trimmed = query.trim();
    const currentRequestId = ++requestId;
    if (!trimmed) {
      results = [];
      nextCursor = null;
      error = null;
      loading = false;
      return;
    }
    loading = true;
    activeQuery = trimmed;
    activePageSize = pageSize;
    activeIncludeResolved = includeResolved;
    error = null;
    try {
      const window = await searchNotes(trimmed, pageSize, includeResolved);
      if (currentRequestId !== requestId) return;
      results = [...window.results];
      nextCursor = window.next_cursor;
    } catch (caught) {
      if (currentRequestId !== requestId) return;
      results = [];
      error = errorMessage(caught);
    } finally {
      if (currentRequestId === requestId) loading = false;
    }
  }

  async function loadMore(): Promise<void> {
    const cursor = nextCursor;
    if (!cursor || loading || !activeQuery) return;
    const currentRequestId = ++requestId;
    loading = true;
    try {
      const window = await searchNotes(
        activeQuery,
        activePageSize,
        activeIncludeResolved,
        cursor,
      );
      if (currentRequestId !== requestId) return;
      results = [
        ...new Map([...results, ...window.results].map((result) => [result.id, result])).values(),
      ];
      nextCursor = window.next_cursor;
    } catch (caught) {
      if (currentRequestId === requestId) error = errorMessage(caught);
    } finally {
      if (currentRequestId === requestId) loading = false;
    }
  }

  return {
    get results(): NotesSearchResult[] { return results; },
    get hasMore(): boolean { return nextCursor !== null; },
    get loading(): boolean { return loading; },
    get error(): string | null { return error; },
    get includeResolvedComments(): boolean { return includeResolvedComments; },
    search,
    loadMoreSearchResults: loadMore,
    setIncludeResolvedComments(value: boolean): void {
      includeResolvedComments = value;
    },
  };
}
