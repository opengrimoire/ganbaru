import { listNotesMentionScope } from "$lib/api/notes";
import type { NotesMentionScope } from "$lib/notes/types";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";

export type NotesMentionScopeLoader = (pageId: string) => Promise<NotesMentionScope>;

const EMPTY_SCOPE: NotesMentionScope = { tasks: [], events: [] };

/**
 * Own the tasks and Calendar events a page may mention. Each request refreshes the scope of one page, sharing a
 * request already running for it; a page only ever reads its own scope, so switching pages never shows another
 * group's tasks or events.
 */
export function createNotesMentionScopeController(load: NotesMentionScopeLoader = listNotesMentionScope) {
  let pageId = $state.raw<string | null>(null);
  let scope = $state.raw<NotesMentionScope>(EMPTY_SCOPE);
  let requestId = 0;
  let pendingPageId: string | null = null;

  async function reload(nextPageId: string): Promise<void> {
    const currentRequestId = ++requestId;
    pendingPageId = nextPageId;
    try {
      const result = await load(nextPageId);
      if (currentRequestId !== requestId) return;
      scope = result;
    } catch (error) {
      if (currentRequestId !== requestId) return;
      console.warn("notes mention scope failed", error);
      scope = EMPTY_SCOPE;
    } finally {
      if (currentRequestId === requestId) pendingPageId = null;
    }
  }

  /** Refresh the scope of a page whose mention menu opened, keeping what it already shows until the result lands. */
  function request(nextPageId: string): void {
    if (nextPageId !== pageId) {
      pageId = nextPageId;
      scope = EMPTY_SCOPE;
    }
    if (pendingPageId !== nextPageId) void reload(nextPageId);
  }

  /** The scope loaded for a page, empty until it loads or when another page's scope is held. */
  function read(forPageId: string): NotesMentionScope {
    return forPageId === pageId ? scope : EMPTY_SCOPE;
  }

  function reset(): void {
    requestId += 1;
    pendingPageId = null;
    pageId = null;
    scope = EMPTY_SCOPE;
  }

  return { request, read, reset };
}

export const notesMentionScope = createNotesMentionScopeController();
onActiveVaultIdentityChange(() => notesMentionScope.reset());
