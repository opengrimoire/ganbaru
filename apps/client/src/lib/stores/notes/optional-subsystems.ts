export type NotesOptionalSubsystem =
  | "templates"
  | "local-user"
  | "history-settings"
  | "undo"
  | "links"
  | "comments"
  | "suggestions"
  | "page-history"
  | "destinations";

export type NotesPagePanelSubsystem = "links" | "suggestions" | "page-history";

const PAGE_SCOPED_SUBSYSTEMS = new Set<NotesOptionalSubsystem>([
  "links",
  "comments",
  "suggestions",
  "page-history",
  "undo",
]);

interface NotesOptionalSubsystemContext {
  readPageGeneration: () => number;
  readSelectedPageId: () => string | null;
  readSelectedProjectId: () => string | null;
  load: (subsystem: NotesOptionalSubsystem, pageId: string | null) => Promise<void>;
}

/** Coordinate lazy Notes subsystems without merging their request identities. */
export function createNotesOptionalSubsystemController(
  context: NotesOptionalSubsystemContext,
) {
  const promises = new Map<string, Promise<void>>();
  const loaded = new Set<string>();
  const openPanels = new Set<NotesPagePanelSubsystem>();

  function keyFor(
    subsystem: NotesOptionalSubsystem,
    pageId: string | null,
    generation: number,
  ): string {
    if (PAGE_SCOPED_SUBSYSTEMS.has(subsystem)) {
      return `${generation}:${subsystem}:${pageId}`;
    }
    if (subsystem === "destinations") {
      return `${subsystem}:${context.readSelectedProjectId() ?? "workspace"}`;
    }
    return subsystem;
  }

  async function ensure(
    subsystem: NotesOptionalSubsystem,
    pageId: string | null = context.readSelectedPageId(),
  ): Promise<void> {
    const pageScoped = PAGE_SCOPED_SUBSYSTEMS.has(subsystem);
    if (pageScoped && !pageId) return;
    const generation = context.readPageGeneration();
    const key = keyFor(subsystem, pageId, generation);
    if (loaded.has(key)) return;
    const existing = promises.get(key);
    if (existing) return existing;
    const promise = context.load(subsystem, pageId).then(() => {
      if (
        !pageScoped
        || (generation === context.readPageGeneration()
          && pageId === context.readSelectedPageId())
      ) {
        loaded.add(key);
      }
    }).finally(() => {
      if (promises.get(key) === promise) promises.delete(key);
    });
    promises.set(key, promise);
    return promise;
  }

  return {
    ensure,
    resetAll(): void {
      promises.clear();
      loaded.clear();
      openPanels.clear();
    },
    resetPageScoped(): void {
      for (const key of loaded) {
        if (/^\d+:/.test(key)) loaded.delete(key);
      }
      openPanels.clear();
    },
    setPanelOpen(subsystem: NotesPagePanelSubsystem, open: boolean): void {
      if (open) openPanels.add(subsystem);
      else openPanels.delete(subsystem);
    },
    isPanelOpen(subsystem: NotesPagePanelSubsystem): boolean {
      return openPanels.has(subsystem);
    },
    markPageSubsystemLoaded(
      subsystem: NotesPagePanelSubsystem,
      pageId: string,
      generation: number,
    ): void {
      loaded.add(keyFor(subsystem, pageId, generation));
    },
  };
}
