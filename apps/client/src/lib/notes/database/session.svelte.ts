import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import { tick } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import type {
  NotesDatabaseView, NotesDatabaseViewScope, NotesDataSourceBoardView,
  NotesDataSourceCalendarView, NotesDataSourceGalleryView, NotesDataSourceListView,
  NotesDataSourceTableView, NotesDataSourceTemplate, NotesDataSourceTimelineView,
} from "$lib/notes/types";

export const MAX_DATABASE_SESSION_RESOURCES = 32;
export const MAX_DATABASE_SESSION_BYTES = 4 * 1024 * 1024;

interface DatabaseResources {
  table: NotesDataSourceTableView;
  board: NotesDataSourceBoardView;
  gallery: NotesDataSourceGalleryView;
  list: NotesDataSourceListView;
  calendar: NotesDataSourceCalendarView;
  timeline: NotesDataSourceTimelineView;
  views: NotesDatabaseView[];
  templates: NotesDataSourceTemplate[];
}

export interface DatabaseResource<K extends keyof DatabaseResources> {
  kind: K;
  key: string;
}

interface ResourceEntry {
  value: unknown;
  generation: number;
  bytes: number;
  pending?: { generation: number; promise: Promise<unknown> };
}

/** Identify a row window or metadata list independently of its mounted renderer. */
export function databaseResource<K extends keyof DatabaseResources>(
  kind: K, dataSourceId: string, scope?: NotesDatabaseViewScope | null,
): DatabaseResource<K> {
  return { kind, key: JSON.stringify([kind, dataSourceId, scope?.databaseId ?? null, scope?.viewId ?? null]) };
}

/** Own bounded, disposable database snapshots and coalesce concurrent reads. */
export function createNotesDatabaseSession(
  maxResources = MAX_DATABASE_SESSION_RESOURCES,
  maxBytes = MAX_DATABASE_SESSION_BYTES,
) {
  const entries = new Map<string, ResourceEntry>();
  const presentation = new SvelteMap<string, { viewId: string | null; scrollLeft: number }>();
  let generation = $state(0);
  let epoch = 0;

  function prune(): void {
    let bytes = [...entries.values()].reduce((sum, entry) => sum + entry.bytes, 0);
    for (const [key, entry] of entries) {
      if (entries.size <= maxResources && bytes <= maxBytes) break;
      entries.delete(key);
      bytes -= entry.bytes;
    }
  }

  function touch(key: string, entry: ResourceEntry): void {
    entries.delete(key);
    entries.set(key, entry);
  }

  function read<K extends keyof DatabaseResources>(resource: DatabaseResource<K>): DatabaseResources[K] | null {
    const entry = entries.get(resource.key);
    if (!entry || entry.value === undefined) return null;
    touch(resource.key, entry);
    // Values enter only through the loader or writer for this typed resource.
    return structuredClone(entry.value) as DatabaseResources[K];
  }

  function write<K extends keyof DatabaseResources>(
    resource: DatabaseResource<K>, value: DatabaseResources[K], expectedEpoch = epoch,
  ): void {
    if (expectedEpoch !== epoch) return;
    const snapshot = structuredClone($state.snapshot(value));
    const bytes = new TextEncoder().encode(JSON.stringify(snapshot)).byteLength;
    if (bytes > maxBytes) {
      entries.delete(resource.key);
      return;
    }
    const entry = entries.get(resource.key) ?? { value: undefined, generation: -1, bytes: 0 };
    entry.value = snapshot;
    entry.generation = generation;
    entry.bytes = bytes;
    touch(resource.key, entry);
    prune();
  }

  function load<K extends keyof DatabaseResources>(
    resource: DatabaseResource<K>, loader: () => Promise<DatabaseResources[K]>, force = false,
  ): Promise<DatabaseResources[K]> {
    const entry = entries.get(resource.key) ?? { value: undefined, generation: -1, bytes: 0 };
    if (!force && entry.generation === generation && entry.value !== undefined) {
      return Promise.resolve(read(resource)!);
    }
    if (entry.pending?.generation === generation) {
      return entry.pending.promise.then((value) => structuredClone(value) as DatabaseResources[K]);
    }
    const requestGeneration = generation;
    const requestEpoch = epoch;
    const promise = Promise.resolve().then(loader).then((value) => {
      if (requestEpoch !== epoch) throw new Error("Database session changed while loading");
      if (requestGeneration !== generation) return load(resource, loader);
      if (entries.get(resource.key)?.pending?.promise === promise) write(resource, value);
      return structuredClone(value);
    });
    entry.pending = { generation: requestGeneration, promise };
    touch(resource.key, entry);
    prune();
    function complete(): void {
      if (entry.pending?.promise === promise) entry.pending = undefined;
    }
    void promise.then(complete, complete);
    return promise;
  }

  function remember(key: string, value: { viewId: string | null; scrollLeft: number }): void {
    presentation.delete(key);
    presentation.set(key, { ...value });
    while (presentation.size > maxResources) presentation.delete(presentation.keys().next().value!);
  }

  return {
    read, write, load, remember,
    /** Preselect a saved view for every renderer of this database without invalidating rows. */
    selectView(key: string, viewId: string): void {
      const current = presentation.get(key);
      if (current?.viewId === viewId) return;
      remember(key, { viewId, scrollLeft: current?.scrollLeft ?? 0 });
    },
    recall: (key: string) => presentation.get(key),
    get revision(): number { return generation; },
    get epoch(): number { return epoch; },
    /** Keep existing rows visible, but require canonical reads after a write. */
    invalidate(): void { generation += 1; },
    /** Release all vault-owned data and reject requests from the previous vault. */
    clear(): void {
      epoch += 1;
      entries.clear();
      presentation.clear();
      generation += 1;
    },
  };
}

export const notesDatabaseSession = createNotesDatabaseSession();
onActiveVaultIdentityChange(() => notesDatabaseSession.clear());

/** Restore horizontal scroll after cached rows mount, and retain it without retaining DOM. */
export function rememberDatabaseScroll(node: HTMLElement, key: string) {
  let disposed = false;
  let scrollKey = key;
  let restoreRequest = 0;
  function restore(): void {
    const request = ++restoreRequest;
    void tick().then(() => {
      if (disposed || request !== restoreRequest) return;
      const viewport = node.querySelector<HTMLElement>("[data-collection-scroll]") ?? node;
      viewport.scrollLeft = notesDatabaseSession.recall(scrollKey)?.scrollLeft ?? 0;
    });
  }
  restore();
  function remember(event: Event): void {
    if (!(event.target instanceof HTMLElement)) return;
    if (event.target !== node && !event.target.hasAttribute("data-collection-scroll")) return;
    notesDatabaseSession.remember(scrollKey, { viewId: null, scrollLeft: event.target.scrollLeft });
  }
  node.addEventListener("scroll", remember, true);
  return {
    update(nextKey: string): void {
      if (nextKey === scrollKey) return;
      scrollKey = nextKey;
      restore();
    },
    destroy(): void { disposed = true; node.removeEventListener("scroll", remember, true); },
  };
}
