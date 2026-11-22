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
  abandonAsyncRequest,
  asyncResourceError,
  failAsyncRequest,
  idleAsyncResource,
  isAsyncResourceLoading,
  isCurrentAsyncRequest,
  resolveAsyncRequest,
  startAsyncRequest,
  updateAsyncResourceData,
  type AsyncResource,
} from "$lib/utils/async-resource";
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

/** A loaded history version and the snapshot it was loaded from. */
interface NotesPageHistoryVersion {
  readonly page: NotesLoadedPage;
  readonly snapshotId: string;
}

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
  let snapshots = $state<AsyncResource<NotesPageHistorySnapshot[]>>(idleAsyncResource([]));
  const snapshotList = $derived(snapshots.data);
  const snapshotsLoading = $derived(isAsyncResourceLoading(snapshots));
  const snapshotsError = $derived(asyncResourceError(snapshots));
  let version = $state<AsyncResource<NotesPageHistoryVersion | null>>(idleAsyncResource(null));
  const versionPage = $derived(version.data?.page ?? null);
  const versionLoading = $derived(isAsyncResourceLoading(version));
  const versionError = $derived(asyncResourceError(version));
  let settings = $state<NotesPageHistorySettings | null>(null);
  let settingsLoading = $state(false);
  let settingsError = $state<string | null>(null);
  let action = $state<NotesPageHistoryAction>(IDLE_PAGE_HISTORY_ACTION);
  let actionToken = 0;
  let settingsRequestId = 0;

  function resetPageState(): void {
    snapshots = idleAsyncResource([]);
    version = idleAsyncResource(null);
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
    if (!pageId) {
      resetPageState();
      return;
    }
    const started = startAsyncRequest(snapshots);
    snapshots = started.resource;
    try {
      const nextSnapshots = await listNotesPageHistorySnapshots(pageId);
      if (pageId !== context.readSelectedPageId()) {
        snapshots = abandonAsyncRequest(snapshots, started.token);
        return;
      }
      if (!isCurrentAsyncRequest(snapshots, started.token)) return;
      snapshots = resolveAsyncRequest(snapshots, started.token, [...nextSnapshots]);
      version = updateAsyncResourceData(version, (shown) => (
        shown
        && shown.page.page.id === pageId
        && nextSnapshots.some((snapshot) => snapshot.id === shown.snapshotId)
          ? shown
          : null
      ));
    } catch (error) {
      snapshots = pageId === context.readSelectedPageId()
        ? failAsyncRequest(snapshots, started.token, notesErrorDetail(error), [])
        : abandonAsyncRequest(snapshots, started.token);
    }
  }

  async function loadVersion(snapshotId: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId) {
      version = idleAsyncResource(null);
      return;
    }
    const started = startAsyncRequest(version);
    version = started.resource;
    try {
      const loaded = await loadNotesPageHistorySnapshot(pageId, snapshotId);
      version = pageId === context.readSelectedPageId()
        ? resolveAsyncRequest(version, started.token, { page: loaded, snapshotId })
        : abandonAsyncRequest(version, started.token);
    } catch (error) {
      version = pageId === context.readSelectedPageId()
        ? failAsyncRequest(version, started.token, notesErrorDetail(error), null)
        : abandonAsyncRequest(version, started.token);
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
      if (!isCurrentPageHistoryAction(action, token)) return;
      // A failure for a page no longer open ends the action without showing it over the open page.
      if (context.readSelectedPageId() !== pageId) {
        dispatchAction({ type: "finished", token });
        return;
      }
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
      return snapshotList;
    },
    get snapshotsLoading() {
      return snapshotsLoading;
    },
    get snapshotsError() {
      return snapshotsError;
    },
    get version() {
      return versionPage;
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
