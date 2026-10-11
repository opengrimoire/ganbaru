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

/** Own mention data loading and reject stale aggregate results. */
export function createNotesMentionDataController(
  load: NotesMentionDataLoader = loadNotesDatabaseMentionData,
) {
  let dataSources = $state.raw<NotesDataSourceListItem[]>([]);
  let rowPages = $state.raw<NotesPage[]>([]);
  let requestId = 0;
  let consumerCount = 0;

  async function reload(): Promise<void> {
    const currentRequestId = ++requestId;
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
    }
  }

  /** Share one request across open menus and release the catalog after the last closes. */
  function acquire(): () => void {
    if (consumerCount++ === 0) void reload();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      if (--consumerCount === 0) reset();
    };
  }

  function reset(): void {
    requestId += 1;
    dataSources = [];
    rowPages = [];
  }

  function switchVault(): void {
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
