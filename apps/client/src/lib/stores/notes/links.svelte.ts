import { notesErrorDetail } from "$lib/notes/failure";
import {
  abandonAsyncRequest,
  asyncResourceError,
  failAsyncRequest,
  idleAsyncResource,
  isAsyncResourceLoading,
  resolveAsyncRequest,
  startAsyncRequest,
  type AsyncResource,
} from "$lib/utils/async-resource";
import {
  addNotesPageAlias,
  deleteNotesPageAlias,
  listNotesBacklinks,
  listNotesDestinationCandidates,
  listNotesPageAliases,
  listNotesUnresolvedLinks,
  resolveNotesUnresolvedLink,
} from "$lib/api/notes";
import type {
  NotesBacklink,
  NotesPage,
  NotesPageAlias,
  NotesUnresolvedLink,
  NotesWorkspaceShell,
} from "$lib/notes/types";

interface NotesLinksControllerContext {
  readSelectedPageId: () => string | null;
  readSelectedProjectId: () => string | null;
  readAllPages: () => NotesPage[];
  reloadSelectedPage: (pageId: string) => Promise<void>;
  scheduleVisibleMetadataRefresh: () => void;
}

/** Own backlink, alias, unresolved-link, and destination request identities. */
export function createNotesLinksController(context: NotesLinksControllerContext) {
  const backlinks = createPageLinkResource<NotesBacklink>(listNotesBacklinks, context.readSelectedPageId);
  const aliases = createPageLinkResource<NotesPageAlias>(listNotesPageAliases, context.readSelectedPageId);
  const unresolved = createPageLinkResource<NotesUnresolvedLink>(
    listNotesUnresolvedLinks,
    context.readSelectedPageId,
  );
  let destinations = $state<NotesPage[]>([]);
  const removedDestinationPageIds = new Set<string>();
  let destinationNextCursor = $state<string | null>(null);
  let destinationQuery = "";
  let destinationsRequestId = 0;

  function currentDestinationPages(candidates: readonly NotesPage[]): NotesPage[] {
    const workspacePages = new Map(context.readAllPages().map((page) => [page.id, page]));
    return candidates
      .filter((page) => !removedDestinationPageIds.has(page.id))
      .map((page) => workspacePages.get(page.id) ?? page);
  }

  function reconcileDestinationPages(
    changedPages: readonly NotesPage[],
    removedPageIds: readonly string[],
  ): void {
    if (changedPages.length === 0 && removedPageIds.length === 0) return;
    for (const pageId of removedPageIds) removedDestinationPageIds.add(pageId);
    for (const page of changedPages) removedDestinationPageIds.delete(page.id);
    destinations = currentDestinationPages(destinations);
  }

  async function reloadDestinations(query = ""): Promise<void> {
    const requestId = ++destinationsRequestId;
    const projectId = context.readSelectedProjectId();
    destinationQuery = query.trim();
    try {
      const result = await listNotesDestinationCandidates(projectId, null, destinationQuery);
      if (requestId !== destinationsRequestId || projectId !== context.readSelectedProjectId()) return;
      destinations = currentDestinationPages(result.pages);
      destinationNextCursor = result.next_page_cursor;
    } catch {
      if (requestId !== destinationsRequestId || projectId !== context.readSelectedProjectId()) return;
      destinations = currentDestinationPages(context.readAllPages());
      destinationNextCursor = null;
    }
  }

  async function loadMoreDestinations(): Promise<void> {
    const cursor = destinationNextCursor;
    if (!cursor) return;
    const requestId = ++destinationsRequestId;
    const projectId = context.readSelectedProjectId();
    let result: NotesWorkspaceShell;
    try {
      result = await listNotesDestinationCandidates(projectId, cursor, destinationQuery);
    } catch (error) {
      // Keep the loaded candidates and cursor so the same page can be requested again.
      console.warn("Notes destination page load failed", error);
      return;
    }
    if (requestId !== destinationsRequestId || projectId !== context.readSelectedProjectId()) return;
    destinations = currentDestinationPages([
      ...new Map([...destinations, ...result.pages].map((page) => [page.id, page])).values(),
    ]);
    destinationNextCursor = result.next_page_cursor;
  }

  async function addAlias(alias: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    const content = alias.trim();
    if (!pageId || !content) return;
    const next = await addNotesPageAlias(pageId, { id: crypto.randomUUID(), alias: content });
    await showAliasWrite(pageId, next);
  }

  async function deleteAlias(aliasId: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId) return;
    const next = await deleteNotesPageAlias(pageId, aliasId);
    await showAliasWrite(pageId, next);
  }

  /** Shows an alias write and the links it changes, unless another page was selected while it ran. */
  async function showAliasWrite(pageId: string, next: readonly NotesPageAlias[]): Promise<void> {
    if (pageId !== context.readSelectedPageId()) return;
    aliases.replace(next);
    await Promise.all([unresolved.reload(pageId), backlinks.reload(pageId)]);
  }

  async function resolveUnresolvedLink(linkId: string, targetPageId: string): Promise<void> {
    const targetId = targetPageId.trim();
    if (!targetId) return;
    const sourcePageId = context.readSelectedPageId();
    const next = await resolveNotesUnresolvedLink(linkId, { target_page_id: targetId });
    const sourceStillShown = sourcePageId !== null && sourcePageId === context.readSelectedPageId();
    if (sourceStillShown) unresolved.replace(next);
    await Promise.all([
      reloadDestinations(),
      sourceStillShown ? context.reloadSelectedPage(sourcePageId) : Promise.resolve(),
    ]);
    context.scheduleVisibleMetadataRefresh();
  }

  function resetPageState(): void {
    backlinks.reset();
    aliases.reset();
    unresolved.reset();
  }

  function resetAll(): void {
    resetPageState();
    destinationsRequestId += 1;
    destinations = [];
    removedDestinationPageIds.clear();
    destinationNextCursor = null;
    destinationQuery = "";
  }

  return {
    get backlinks(): NotesBacklink[] { return backlinks.items; },
    get backlinksLoading(): boolean { return backlinks.loading; },
    get backlinksError(): string | null { return backlinks.error; },
    get aliases(): NotesPageAlias[] { return aliases.items; },
    get aliasesLoading(): boolean { return aliases.loading; },
    get aliasesError(): string | null { return aliases.error; },
    get unresolvedLinks(): NotesUnresolvedLink[] { return unresolved.items; },
    get unresolvedLoading(): boolean { return unresolved.loading; },
    get unresolvedError(): string | null { return unresolved.error; },
    get destinations(): NotesPage[] { return destinations; },
    get destinationHasMore(): boolean { return destinationNextCursor !== null; },
    reloadBacklinks: (pageId = context.readSelectedPageId()) => backlinks.reload(pageId),
    reloadPageAliases: (pageId = context.readSelectedPageId()) => aliases.reload(pageId),
    reloadUnresolvedLinks: (pageId = context.readSelectedPageId()) => unresolved.reload(pageId),
    reloadLinkResolutionPages: reloadDestinations,
    loadMoreDestinationCandidates: loadMoreDestinations,
    reconcileDestinationPages,
    addPageAlias: addAlias,
    deletePageAlias: deleteAlias,
    resolveUnresolvedLink,
    resetPageState,
    resetAll,
  };
}

/**
 * One list that belongs to the selected page. A response settles it only while its request is current and its page
 * is still selected; a failed read empties the list and shows the error.
 *
 * @param load - Reads the list for a page.
 * @param readSelectedPageId - The page shown now.
 */
function createPageLinkResource<T>(
  load: (pageId: string) => Promise<readonly T[]>,
  readSelectedPageId: () => string | null,
) {
  let resource = $state<AsyncResource<T[]>>(idleAsyncResource([]));
  const loading = $derived(isAsyncResourceLoading(resource));
  const error = $derived(asyncResourceError(resource));

  async function reload(pageId: string | null): Promise<void> {
    if (!pageId) {
      reset();
      return;
    }
    const started = startAsyncRequest(resource);
    resource = started.resource;
    try {
      const next = await load(pageId);
      resource = pageId === readSelectedPageId()
        ? resolveAsyncRequest(resource, started.token, [...next])
        : abandonAsyncRequest(resource, started.token);
    } catch (failure) {
      resource = pageId === readSelectedPageId()
        ? failAsyncRequest(resource, started.token, notesErrorDetail(failure), [])
        : abandonAsyncRequest(resource, started.token);
    }
  }

  /** Shows `items` from a local write; this supersedes any read in flight. */
  function replace(items: readonly T[]): void {
    resource = idleAsyncResource([...items]);
  }

  function reset(): void {
    resource = idleAsyncResource([]);
  }

  return {
    get items(): T[] { return resource.data; },
    get loading(): boolean { return loading; },
    get error(): string | null { return error; },
    reload,
    replace,
    reset,
  };
}
