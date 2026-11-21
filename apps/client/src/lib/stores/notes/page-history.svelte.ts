import { notesErrorDetail } from "$lib/notes/failure";
import {
  copyNotesPageHistoryBlocks,
  getNotesPageHistorySettings,
  listNotesPageHistorySnapshots,
  loadNotesPageHistorySnapshot,
  restoreNotesPageHistorySnapshot,
  updateNotesPageHistorySettings,
} from "$lib/api/notes";
import type {
  NotesLoadedPage,
  NotesPageHistorySettings,
  NotesPageHistorySnapshot,
} from "$lib/notes/types";
import type { NotesHistoryRetentionDays } from "$lib/notes/history/retention";
import {
  notesPostAppendResult,
  type NotesPostMutationResult,
} from "$lib/notes/post-mutation";
import {
  IDLE_PAGE_HISTORY_ACTION,
  isCurrentPageHistoryAction,
  transitionPageHistoryAction,
  type NotesPageHistoryAction,
  type NotesPageHistoryActionEvent,
  type NotesPageHistoryActionKind,
} from "./page-history-state";

export interface NotesPageHistoryControllerContext {
  readSelectedPageId: () => string | null;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  requestPageLoadFocus: (requestedBlockId?: string | null) => void;
  flushPendingBlockSaves: () => Promise<void>;
  setLoadError: (message: string) => void;
}

export interface NotesPageHistoryController {
  readonly snapshots: NotesPageHistorySnapshot[];
  readonly snapshotsLoading: boolean;
  readonly snapshotsError: string | null;
  readonly version: NotesLoadedPage | null;
  readonly versionLoading: boolean;
  readonly versionError: string | null;
  readonly settings: NotesPageHistorySettings | null;
  readonly settingsLoading: boolean;
  readonly settingsError: string | null;
  readonly actionLoading: boolean;
  readonly actionError: string | null;
  resetPageState: () => void;
  loadSettings: () => Promise<void>;
  updateRetention: (retentionDays: NotesHistoryRetentionDays) => Promise<void>;
  reloadSnapshots: (pageId?: string | null) => Promise<void>;
  loadVersion: (snapshotId: string) => Promise<void>;
  restoreVersion: (snapshotId: string) => Promise<void>;
  copyBlocks: (snapshotId: string) => Promise<void>;
}

/**
 * Create the page history state boundary for the Notes store.
 */
export function createNotesPageHistoryController(
  context: NotesPageHistoryControllerContext,
): NotesPageHistoryController {
  let snapshots = $state<NotesPageHistorySnapshot[]>([]);
  let snapshotsLoading = $state(false);
  let snapshotsError = $state<string | null>(null);
  let version = $state<NotesLoadedPage | null>(null);
  let versionSnapshotId: string | null = null;
  let versionLoading = $state(false);
  let versionError = $state<string | null>(null);
  let settings = $state<NotesPageHistorySettings | null>(null);
  let settingsLoading = $state(false);
  let settingsError = $state<string | null>(null);
  let action = $state<NotesPageHistoryAction>(IDLE_PAGE_HISTORY_ACTION);
  let actionToken = 0;
  let snapshotsRequestId = 0;
  let versionRequestId = 0;
  let settingsRequestId = 0;

  function resetPageState(): void {
    snapshotsRequestId += 1;
    versionRequestId += 1;
    snapshots = [];
    snapshotsLoading = false;
    snapshotsError = null;
    version = null;
    versionSnapshotId = null;
    versionLoading = false;
    versionError = null;
    dispatchAction({ type: "reset" });
  }

  async function loadSettings(): Promise<void> {
    const requestId = ++settingsRequestId;
    settingsLoading = true;
    settingsError = null;
    try {
      const nextSettings = await getNotesPageHistorySettings();
      if (requestId !== settingsRequestId) return;
      settings = nextSettings;
    } catch (error) {
      if (requestId !== settingsRequestId) return;
      settingsError = notesErrorDetail(error);
      context.setLoadError(settingsError);
    } finally {
      if (requestId === settingsRequestId) settingsLoading = false;
    }
  }

  async function updateRetention(retentionDays: NotesHistoryRetentionDays): Promise<void> {
    const requestId = ++settingsRequestId;
    settingsLoading = true;
    settingsError = null;
    try {
      const nextSettings = await updateNotesPageHistorySettings({ retention_days: retentionDays });
      if (requestId !== settingsRequestId) return;
      settings = nextSettings;
      await reloadSnapshots();
    } catch (error) {
      if (requestId !== settingsRequestId) return;
      settingsError = notesErrorDetail(error);
      context.setLoadError(settingsError);
    } finally {
      if (requestId === settingsRequestId) settingsLoading = false;
    }
  }

  async function reloadSnapshots(pageId: string | null = context.readSelectedPageId()): Promise<void> {
    const requestId = ++snapshotsRequestId;
    if (!pageId) {
      resetPageState();
      return;
    }
    snapshotsLoading = true;
    snapshotsError = null;
    try {
      const nextSnapshots = await listNotesPageHistorySnapshots(pageId);
      if (requestId !== snapshotsRequestId || pageId !== context.readSelectedPageId()) return;
      snapshots = [...nextSnapshots];
      if (version && version.page.id !== pageId) {
        version = null;
        versionSnapshotId = null;
      }
      if (version && !nextSnapshots.some((snapshot) => snapshot.id === versionSnapshotId)) {
        version = null;
        versionSnapshotId = null;
      }
    } catch (error) {
      if (requestId !== snapshotsRequestId || pageId !== context.readSelectedPageId()) return;
      snapshots = [];
      snapshotsError = notesErrorDetail(error);
    } finally {
      if (requestId === snapshotsRequestId) snapshotsLoading = false;
    }
  }

  async function loadVersion(snapshotId: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    const requestId = ++versionRequestId;
    if (!pageId) {
      version = null;
      return;
    }
    versionLoading = true;
    versionError = null;
    try {
      const loaded = await loadNotesPageHistorySnapshot(pageId, snapshotId);
      if (requestId !== versionRequestId || pageId !== context.readSelectedPageId()) return;
      version = loaded;
      versionSnapshotId = snapshotId;
    } catch (error) {
      if (requestId !== versionRequestId || pageId !== context.readSelectedPageId()) return;
      version = null;
      versionSnapshotId = null;
      versionError = notesErrorDetail(error);
    } finally {
      if (requestId === versionRequestId) versionLoading = false;
    }
  }

  function dispatchAction(event: NotesPageHistoryActionEvent): void {
    action = transitionPageHistoryAction(action, event);
  }

  /**
   * Run one write to the selected page's history, refusing a second while one runs. `apply` receives whether the
   * result still belongs to the open page; a failure is shown only for the current action.
   */
  async function runAction(
    kind: NotesPageHistoryActionKind,
    write: (pageId: string) => Promise<(current: boolean) => void>,
  ): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId || action.phase === "running") return;
    const token = ++actionToken;
    dispatchAction({ type: "started", kind, pageId, token });
    const current = () => isCurrentPageHistoryAction(action, token) && context.readSelectedPageId() === pageId;
    try {
      await context.flushPendingBlockSaves();
      const apply = await write(pageId);
      apply(current());
      dispatchAction({ type: "finished", token });
    } catch (error) {
      if (!current()) return;
      const message = notesErrorDetail(error);
      dispatchAction({ type: "failed", token, error: message });
      context.setLoadError(message);
    }
  }

  function restoreVersion(snapshotId: string): Promise<void> {
    return runAction("restore", async (pageId) => {
      const loaded = await restoreNotesPageHistorySnapshot(pageId, snapshotId);
      return (current) => {
        // A restore for a page no longer open still updates its metadata, but never replaces the open page.
        if (!current) {
          context.applyPostMutation({ pages: [loaded.page], sidebarImpact: "visible-metadata" });
          return;
        }
        context.applyPostMutation({ loadedPage: loaded, pages: [loaded.page], sidebarImpact: "visible-metadata" });
        context.requestPageLoadFocus();
      };
    });
  }

  function copyBlocks(snapshotId: string): Promise<void> {
    return runAction("copy", async (pageId) => {
      const request = {
        parent: { type: "page_id", page_id: pageId } as const,
        after_block_id: null,
      };
      const copied = await copyNotesPageHistoryBlocks(pageId, snapshotId, request);
      return (current) => {
        if (!current) return;
        context.applyPostMutation(notesPostAppendResult({
          parent: request.parent,
          after: request.after_block_id,
          children: [],
        }, copied));
        context.requestPageLoadFocus(copied.results[0]?.id ?? null);
      };
    });
  }

  return {
    get snapshots() {
      return snapshots;
    },
    get snapshotsLoading() {
      return snapshotsLoading;
    },
    get snapshotsError() {
      return snapshotsError;
    },
    get version() {
      return version;
    },
    get versionLoading() {
      return versionLoading;
    },
    get versionError() {
      return versionError;
    },
    get settings() {
      return settings;
    },
    get settingsLoading() {
      return settingsLoading;
    },
    get settingsError() {
      return settingsError;
    },
    get actionLoading() {
      return action.phase === "running";
    },
    get actionError() {
      return action.phase === "failed" ? action.error : null;
    },
    resetPageState,
    loadSettings,
    updateRetention,
    reloadSnapshots,
    loadVersion,
    restoreVersion,
    copyBlocks,
  };
}
