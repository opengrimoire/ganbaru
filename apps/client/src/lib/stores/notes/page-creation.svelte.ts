import { notesErrorDetail } from "$lib/notes/failure";
import { createNotesPage } from "$lib/api/notes";
import type { NotesLoadedPage, NotesPageCreate } from "$lib/notes/types";
import {
  notesPageCreationError,
  notesPageCreationSaved,
  notesPageCreationStarted,
  transitionPageCreation,
  type NotesPageCreation,
  type NotesPageCreationEvent,
} from "./page-creation-state";


interface CreatedPagePreview {
  loaded: NotesLoadedPage;
  clean: boolean;
}

const MAX_CREATED_PAGE_PREVIEWS = 32;

interface NotesPageCreationContext {
  reconcile: (loaded: NotesLoadedPage) => Promise<void>;
  afterPersisted?: (pageId: string) => Promise<void>;
}

/** Coordinate optimistic page creation and gate mutations until persistence succeeds. */
export function createNotesPageCreationController(context: NotesPageCreationContext) {
  let pending = $state<Record<string, NotesPageCreation>>({});
  const attempts = new Map<string, Promise<void>>();
  const previews = new Map<string, CreatedPagePreview>();
  const completions = new Map<string, {
    promise: Promise<"ready" | "discarded">;
    resolve: (outcome: "ready" | "discarded") => void;
  }>();

  function completionFor(pageId: string) {
    const current = completions.get(pageId);
    if (current) return current;
    let resolve: (outcome: "ready" | "discarded") => void = () => {};
    const promise = new Promise<"ready" | "discarded">((onResolve) => {
      resolve = onResolve;
    });
    const completion = { promise, resolve };
    completions.set(pageId, completion);
    return completion;
  }

  function dispatch(pageId: string, event: NotesPageCreationEvent): void {
    const current = pending[pageId];
    if (!current) return;
    const next = transitionPageCreation(current, event);
    if (next !== current) pending = { ...pending, [pageId]: next };
  }

  function track(pageId: string, attempt: Promise<void>): Promise<void> {
    const barrier = attempt.finally(() => {
      if (attempts.get(pageId) === barrier) attempts.delete(pageId);
    });
    attempts.set(pageId, barrier);
    return barrier;
  }

  async function create(request: NotesPageCreate): Promise<void> {
    let loaded: NotesLoadedPage;
    try {
      loaded = await createNotesPage(request);
    } catch (error) {
      dispatch(request.id, { type: "create-failed", error: notesErrorDetail(error) });
      return;
    }
    dispatch(request.id, { type: "created", loaded });
    await reconcile(request.id, loaded);
  }

  /** Project a created page into the open editors, then release the edits that waited for it. */
  async function reconcile(pageId: string, loaded: NotesLoadedPage): Promise<void> {
    try {
      await context.reconcile(loaded);
    } catch (error) {
      // The page exists, so its provisional draft is stale; reopening it loads the saved page.
      previews.delete(pageId);
      dispatch(pageId, { type: "reconcile-failed", error: notesErrorDetail(error) });
      return;
    }
    const preview = previews.get(pageId);
    if (preview?.clean) {
      previews.delete(pageId);
      previews.set(pageId, { loaded, clean: true });
      for (const [id] of previews) {
        if (previews.size <= MAX_CREATED_PAGE_PREVIEWS) break;
        if (!pending[id] || id === pageId) previews.delete(id);
      }
    } else {
      previews.delete(pageId);
    }
    const next = { ...pending };
    delete next[pageId];
    pending = next;
    const completion = completions.get(pageId);
    completions.delete(pageId);
    completion?.resolve("ready");
    void context.afterPersisted?.(pageId).catch(() => undefined);
  }

  function persist(request: NotesPageCreate): Promise<void> {
    completionFor(request.id);
    pending = { ...pending, [request.id]: notesPageCreationStarted(request) };
    return track(request.id, create(request));
  }

  function begin(request: NotesPageCreate, provisional: NotesLoadedPage): void {
    previews.set(request.id, { loaded: provisional, clean: true });
    void persist(request);
  }

  /** Reuse a draft during creation or a clean result on its first reopen. */
  function previewForSelection(pageId: string): { loaded: NotesLoadedPage; pending: boolean } | null {
    const preview = previews.get(pageId);
    if (!preview) return null;
    const isPending = Boolean(pending[pageId]);
    if (!isPending && !preview.clean) {
      previews.delete(pageId);
      return null;
    }
    if (!isPending) previews.delete(pageId);
    return { loaded: preview.loaded, pending: isPending };
  }

  function markChanged(pageId: string | null): void {
    if (!pageId) return;
    const preview = previews.get(pageId);
    if (preview) previews.set(pageId, { ...preview, clean: false });
  }

  async function awaitReady(pageId: string | null): Promise<void> {
    if (!pageId) return;
    const completion = completions.get(pageId);
    if (completion && (await completion.promise) === "discarded") {
      throw new Error("notes page was discarded before creation completed");
    }
  }

  /**
   * Wait for a creation attempt to finish without waiting for a possible retry. Only a failed create request is
   * `failed`; a created page whose reconcile failed exists and reports `ready`.
   */
  async function awaitAttempt(pageId: string): Promise<"ready" | "failed"> {
    await attempts.get(pageId);
    return pending[pageId]?.phase === "create-failed" ? "failed" : "ready";
  }

  /** Release blocked edits after a draft whose create request failed is removed from Notes. */
  function discardFailed(pageId: string): void {
    if (pending[pageId]?.phase !== "create-failed" || attempts.has(pageId)) return;
    const next = { ...pending };
    delete next[pageId];
    pending = next;
    previews.delete(pageId);
    completions.get(pageId)?.resolve("discarded");
    completions.delete(pageId);
  }

  /** Retry the failed step: the create request, or only the reconcile of a page that was already created. */
  function retry(pageId: string): void {
    const state = pending[pageId];
    if (!state || attempts.has(pageId)) return;
    if (state.phase === "create-failed") {
      dispatch(pageId, { type: "create-started" });
      void track(pageId, create(state.request));
    } else if (state.phase === "reconcile-failed") {
      dispatch(pageId, { type: "reconcile-started" });
      void track(pageId, reconcile(pageId, state.loaded));
    }
  }

  return {
    begin,
    previewForSelection,
    markChanged,
    awaitReady,
    awaitAttempt,
    discardFailed,
    retry,
    isPending(pageId: string | null): boolean {
      return Boolean(pageId && pending[pageId]);
    },
    /** Whether the backend already holds this pending page, so only reconciling it remains. */
    isSaved(pageId: string | null): boolean {
      return pageId ? notesPageCreationSaved(pending[pageId]) : false;
    },
    errorFor(pageId: string | null): string | null {
      return pageId ? notesPageCreationError(pending[pageId]) : null;
    },
  };
}
