import type { NotesDataSourceListItem, NotesPage } from "$lib/notes/types";
import type { NotesNamedMentionTarget } from "$lib/notes/rich-text/core";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import {
  loadNotesDatabaseMentionData,
  type NotesDatabaseMentionData,
} from "./mention-targets";

export type NotesMentionDataLoader = () => Promise<NotesDatabaseMentionData>;

export interface NotesMentionCatalog {
  read: () => NotesNamedMentionTarget[];
  acquire: () => () => void;
}

/**
 * How long database data outlives the last open mention menu. A menu that closes and reopens within this window, for
 * example while the editor briefly loses its selection during a render, keeps showing the loaded databases instead of
 * dropping them until a fresh load finishes.
 */
export const NOTES_MENTION_DATA_RELEASE_DELAY_MS = 30_000;

/** Own mention data loading and reject stale aggregate results. */
export function createNotesMentionDataController(
  load: NotesMentionDataLoader = loadNotesDatabaseMentionData,
  releaseDelayMs = NOTES_MENTION_DATA_RELEASE_DELAY_MS,
) {
  let dataSources = $state.raw<NotesDataSourceListItem[]>([]);
  let rowPages = $state.raw<NotesPage[]>([]);
  let requestId = 0;
  let consumerCount = 0;
  let loading = false;
  let releaseTimer: ReturnType<typeof setTimeout> | null = null;

  async function reload(): Promise<void> {
    const currentRequestId = ++requestId;
    loading = true;
    try {
      const result = await load();
      if (currentRequestId !== requestId) return;
      dataSources = result.dataSources;
      rowPages = result.rowPages;
    } catch (error) {
      if (currentRequestId !== requestId) return;
      console.warn("notes mention data source targets failed", error);
      dataSources = [];
      rowPages = [];
    } finally {
      if (currentRequestId === requestId) loading = false;
    }
  }

  /**
   * Share one request across open menus. Reopening after every menu closed refreshes the data while keeping what is
   * already shown; the data is released once no menu has been open for the release delay.
   */
  function acquire(): () => void {
    if (consumerCount++ === 0) {
      const retained = cancelRelease();
      if (!retained || !loading) void reload();
    }
    let released = false;
    return () => {
      if (released) return;
      released = true;
      if (--consumerCount === 0) scheduleRelease();
    };
  }

  function scheduleRelease(): void {
    cancelRelease();
    releaseTimer = setTimeout(() => {
      releaseTimer = null;
      reset();
    }, releaseDelayMs);
  }

  /** @returns Whether data from an earlier menu was still retained. */
  function cancelRelease(): boolean {
    if (releaseTimer === null) return false;
    clearTimeout(releaseTimer);
    releaseTimer = null;
    return true;
  }

  function reset(): void {
    requestId += 1;
    loading = false;
    dataSources = [];
    rowPages = [];
  }

  function switchVault(): void {
    cancelRelease();
    reset();
    if (consumerCount > 0) void reload();
  }

  return {
    get dataSources() { return dataSources; },
    get rowPages() { return rowPages; },
    reload,
    acquire,
    switchVault,
  };
}

export const notesMentionData = createNotesMentionDataController();
onActiveVaultIdentityChange(() => notesMentionData.switchVault());
