import {
  mergeNotesSidebarMetadataImpact,
  type NotesSidebarMetadataImpact,
} from "$lib/notes/post-mutation";

export interface NotesSidebarRefreshClock {
  setTimeout: (callback: () => void, delayMs: number) => ReturnType<typeof setTimeout>;
  clearTimeout: (timer: ReturnType<typeof setTimeout>) => void;
}

export interface NotesSidebarRefreshCoordinator {
  schedule: (impact: NotesSidebarMetadataImpact) => void;
  flush: () => Promise<void>;
  cancel: () => void;
}

const SIDEBAR_REFRESH_DELAY_MS = 50;

/** Coalesces mutation-driven sidebar metadata reads into one bounded refresh. */
export function createNotesSidebarRefreshCoordinator(input: {
  refresh: (impact: Exclude<NotesSidebarMetadataImpact, "none">) => Promise<void>;
  clock?: NotesSidebarRefreshClock;
}): NotesSidebarRefreshCoordinator {
  const clock = input.clock ?? {
    setTimeout: (callback, delayMs) => setTimeout(callback, delayMs),
    clearTimeout: (timer) => clearTimeout(timer),
  };
  let pendingImpact: NotesSidebarMetadataImpact = "none";
  let timer: ReturnType<typeof setTimeout> | null = null;
  let inFlight: Promise<void> | null = null;

  async function run(): Promise<void> {
    if (timer !== null) {
      clock.clearTimeout(timer);
      timer = null;
    }
    if (inFlight) await inFlight;
    const impact = pendingImpact;
    pendingImpact = "none";
    if (impact === "none") return;
    inFlight = input.refresh(impact).finally(() => {
      inFlight = null;
      if (pendingImpact !== "none" && timer === null) {
        timer = clock.setTimeout(() => {
          timer = null;
          void run();
        }, SIDEBAR_REFRESH_DELAY_MS);
      }
    });
    await inFlight;
  }

  return {
    schedule(impact) {
      pendingImpact = mergeNotesSidebarMetadataImpact(pendingImpact, impact);
      if (pendingImpact === "none" || timer !== null || inFlight) return;
      timer = clock.setTimeout(() => {
        timer = null;
        void run();
      }, SIDEBAR_REFRESH_DELAY_MS);
    },
    flush: run,
    cancel() {
      if (timer !== null) clock.clearTimeout(timer);
      timer = null;
      pendingImpact = "none";
    },
  };
}
