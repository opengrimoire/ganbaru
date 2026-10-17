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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Own backlink, alias, unresolved-link, and destination request identities. */
export function createNotesLinksController(context: NotesLinksControllerContext) {
  let backlinks = $state<NotesBacklink[]>([]);
  let aliases = $state<NotesPageAlias[]>([]);
  let unresolved = $state<NotesUnresolvedLink[]>([]);
  let destinations = $state<NotesPage[]>([]);
  const removedDestinationPageIds = new Set<string>();
  let destinationNextCursor = $state<string | null>(null);
  let destinationQuery = "";
  let backlinksLoading = $state(false);
  let backlinksError = $state<string | null>(null);
  let aliasesLoading = $state(false);
  let aliasesError = $state<string | null>(null);
  let unresolvedLoading = $state(false);
  let unresolvedError = $state<string | null>(null);
  let backlinksRequestId = 0;
  let aliasesRequestId = 0;
  let unresolvedRequestId = 0;
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

  async function reloadBacklinks(pageId = context.readSelectedPageId()): Promise<void> {
    const requestId = ++backlinksRequestId;
    if (!pageId) {
      backlinks = [];
      backlinksError = null;
      backlinksLoading = false;
      return;
    }
    backlinksLoading = true;
    backlinksError = null;
    try {
      const next = await listNotesBacklinks(pageId);
      if (requestId !== backlinksRequestId || pageId !== context.readSelectedPageId()) return;
      backlinks = [...next];
    } catch (error) {
      if (requestId !== backlinksRequestId || pageId !== context.readSelectedPageId()) return;
      backlinks = [];
      backlinksError = errorMessage(error);
    } finally {
      if (requestId === backlinksRequestId) backlinksLoading = false;
    }
  }

  async function reloadAliases(pageId = context.readSelectedPageId()): Promise<void> {
    const requestId = ++aliasesRequestId;
    if (!pageId) {
      aliases = [];
      aliasesError = null;
      aliasesLoading = false;
      return;
    }
    aliasesLoading = true;
    aliasesError = null;
    try {
      const next = await listNotesPageAliases(pageId);
      if (requestId !== aliasesRequestId || pageId !== context.readSelectedPageId()) return;
      aliases = [...next];
    } catch (error) {
      if (requestId !== aliasesRequestId || pageId !== context.readSelectedPageId()) return;
      aliases = [];
      aliasesError = errorMessage(error);
    } finally {
      if (requestId === aliasesRequestId) aliasesLoading = false;
    }
  }

  async function reloadUnresolved(pageId = context.readSelectedPageId()): Promise<void> {
    const requestId = ++unresolvedRequestId;
    if (!pageId) {
      unresolved = [];
      unresolvedError = null;
      unresolvedLoading = false;
      return;
    }
    unresolvedLoading = true;
    unresolvedError = null;
    try {
      const next = await listNotesUnresolvedLinks(pageId);
      if (requestId !== unresolvedRequestId || pageId !== context.readSelectedPageId()) return;
      unresolved = [...next];
    } catch (error) {
      if (requestId !== unresolvedRequestId || pageId !== context.readSelectedPageId()) return;
      unresolved = [];
      unresolvedError = errorMessage(error);
    } finally {
      if (requestId === unresolvedRequestId) unresolvedLoading = false;
    }
  }

  async function addAlias(alias: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    const content = alias.trim();
    if (!pageId || !content) return;
    aliases = await addNotesPageAlias(pageId, { id: crypto.randomUUID(), alias: content });
    aliasesError = null;
    await Promise.all([reloadUnresolved(pageId), reloadBacklinks(pageId)]);
  }

  async function deleteAlias(aliasId: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId) return;
    aliases = await deleteNotesPageAlias(pageId, aliasId);
    aliasesError = null;
    await Promise.all([reloadUnresolved(pageId), reloadBacklinks(pageId)]);
  }

  async function resolveUnresolvedLink(linkId: string, targetPageId: string): Promise<void> {
    const targetId = targetPageId.trim();
    if (!targetId) return;
    unresolved = await resolveNotesUnresolvedLink(linkId, { target_page_id: targetId });
    unresolvedError = null;
    const sourcePageId = context.readSelectedPageId();
    await Promise.all([
      reloadDestinations(),
      sourcePageId ? context.reloadSelectedPage(sourcePageId) : Promise.resolve(),
    ]);
    context.scheduleVisibleMetadataRefresh();
  }

  function resetPageState(): void {
    backlinksRequestId += 1;
    aliasesRequestId += 1;
    unresolvedRequestId += 1;
    backlinks = [];
    aliases = [];
    unresolved = [];
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
    get backlinks(): NotesBacklink[] { return backlinks; },
    get backlinksLoading(): boolean { return backlinksLoading; },
    get backlinksError(): string | null { return backlinksError; },
    get aliases(): NotesPageAlias[] { return aliases; },
    get aliasesLoading(): boolean { return aliasesLoading; },
    get aliasesError(): string | null { return aliasesError; },
    get unresolvedLinks(): NotesUnresolvedLink[] { return unresolved; },
    get unresolvedLoading(): boolean { return unresolvedLoading; },
    get unresolvedError(): string | null { return unresolvedError; },
    get destinations(): NotesPage[] { return destinations; },
    get destinationHasMore(): boolean { return destinationNextCursor !== null; },
    reloadBacklinks,
    reloadPageAliases: reloadAliases,
    reloadUnresolvedLinks: reloadUnresolved,
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
