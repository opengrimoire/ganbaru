import { notesErrorDetail } from "$lib/notes/failure";
import { searchNotes } from "$lib/api/notes";
import type { NotesSearchResult } from "$lib/notes/types";
import {
  asyncResourceError,
  failAsyncRequest,
  idleAsyncResource,
  isAsyncResourceLoading,
  resolveAsyncRequest,
  startAsyncRequest,
  type AsyncResource,
} from "$lib/utils/async-resource";

/** The loaded search results and the cursor of the next page, or `null` when there is none. */
interface NotesSearchPages {
  readonly results: NotesSearchResult[];
  readonly nextCursor: string | null;
}

const NO_SEARCH_PAGES: NotesSearchPages = { results: [], nextCursor: null };

/** Own Notes search pagination and stale-result rejection. */
export function createNotesSearchController() {
  let resource = $state<AsyncResource<NotesSearchPages>>(idleAsyncResource(NO_SEARCH_PAGES));
  const loading = $derived(isAsyncResourceLoading(resource));
  let includeResolvedComments = $state(false);
  let activeQuery = "";
  let activePageSize = 20;
  let activeIncludeResolved = false;

  async function search(
    query: string,
    pageSize = 20,
    includeResolved = includeResolvedComments,
  ): Promise<void> {
    const trimmed = query.trim();
    if (!trimmed) {
      resource = idleAsyncResource(NO_SEARCH_PAGES);
      return;
    }
    const started = startAsyncRequest(resource);
    resource = started.resource;
    activeQuery = trimmed;
    activePageSize = pageSize;
    activeIncludeResolved = includeResolved;
    try {
      const window = await searchNotes(trimmed, pageSize, includeResolved);
      resource = resolveAsyncRequest(resource, started.token, {
        results: [...window.results],
        nextCursor: window.next_cursor,
      });
    } catch (caught) {
      resource = failAsyncRequest(resource, started.token, notesErrorDetail(caught), {
        ...resource.data,
        results: [],
      });
    }
  }

  async function loadMore(): Promise<void> {
    const cursor = resource.data.nextCursor;
    if (!cursor || loading || !activeQuery) return;
    const started = startAsyncRequest(resource);
    resource = started.resource;
    try {
      const window = await searchNotes(
        activeQuery,
        activePageSize,
        activeIncludeResolved,
        cursor,
      );
      const loaded = resource.data.results;
      resource = resolveAsyncRequest(resource, started.token, {
        results: [
          ...new Map([...loaded, ...window.results].map((result) => [result.id, result])).values(),
        ],
        nextCursor: window.next_cursor,
      });
    } catch (caught) {
      resource = failAsyncRequest(resource, started.token, notesErrorDetail(caught));
    }
  }

  return {
    get results(): NotesSearchResult[] { return resource.data.results; },
    get hasMore(): boolean { return resource.data.nextCursor !== null; },
    get loading(): boolean { return loading; },
    get error(): string | null { return asyncResourceError(resource); },
    get includeResolvedComments(): boolean { return includeResolvedComments; },
    search,
    loadMoreSearchResults: loadMore,
    setIncludeResolvedComments(value: boolean): void {
      includeResolvedComments = value;
    },
  };
}
