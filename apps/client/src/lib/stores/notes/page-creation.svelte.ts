import { createNotesPage } from "$lib/api/notes";
import type { NotesLoadedPage, NotesPageCreate } from "$lib/notes/types";

interface PendingPageCreation {
  request: NotesPageCreate;
  status: "pending" | "failed";
  error: string | null;
}

interface CreatedPagePreview {
  loaded: NotesLoadedPage;
  clean: boolean;
}

const MAX_CREATED_PAGE_PREVIEWS = 32;

interface NotesPageCreationContext {
  reconcile: (loaded: NotesLoadedPage) => Promise<void>;
  afterPersisted?: (pageId: string) => Promise<void>;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Coordinate optimistic page creation and gate mutations until persistence succeeds. */
export function createNotesPageCreationController(context: NotesPageCreationContext) {
  let pending = $state<Record<string, PendingPageCreation>>({});
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

  function persist(request: NotesPageCreate): Promise<void> {
    let succeeded = false;
    const completion = completionFor(request.id);
    pending = {
      ...pending,
      [request.id]: { request, status: "pending", error: null },
    };
    const barrier = createNotesPage(request)
      .then(async (loaded) => {
        await context.reconcile(loaded);
        const preview = previews.get(request.id);
        if (preview?.clean) {
          previews.delete(request.id);
          previews.set(request.id, { loaded, clean: true });
          for (const [id] of previews) {
            if (previews.size <= MAX_CREATED_PAGE_PREVIEWS) break;
            if (!pending[id] || id === request.id) previews.delete(id);
          }
        } else {
          previews.delete(request.id);
        }
        const next = { ...pending };
        delete next[request.id];
        pending = next;
        succeeded = true;
        completion.resolve("ready");
        completions.delete(request.id);
      })
      .catch((error: unknown) => {
        pending = {
          ...pending,
          [request.id]: { request, status: "failed", error: errorMessage(error) },
        };
      })
      .finally(() => {
        if (attempts.get(request.id) === barrier) attempts.delete(request.id);
        if (succeeded) {
          void context.afterPersisted?.(request.id).catch(() => undefined);
        }
      });
    attempts.set(request.id, barrier);
    return barrier;
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

  /** Wait for a creation attempt to finish without waiting for a possible retry. */
  async function awaitAttempt(pageId: string): Promise<"ready" | "failed"> {
    await attempts.get(pageId);
    return pending[pageId]?.status === "failed" ? "failed" : "ready";
  }

  /** Release blocked edits after a failed draft is removed from Notes. */
  function discardFailed(pageId: string): void {
    if (pending[pageId]?.status !== "failed" || attempts.has(pageId)) return;
    const next = { ...pending };
    delete next[pageId];
    pending = next;
    previews.delete(pageId);
    completions.get(pageId)?.resolve("discarded");
    completions.delete(pageId);
  }

  function retry(pageId: string): void {
    const state = pending[pageId];
    if (!state || attempts.has(pageId)) return;
    void persist(state.request);
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
    errorFor(pageId: string | null): string | null {
      return pageId ? pending[pageId]?.error ?? null : null;
    },
  };
}
