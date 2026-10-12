<script lang="ts">
  import { onMount, tick } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import { FALLBACK_COLOR_INDEX } from "$lib/calendar/types";
  import {
    archiveQuickNote,
    createQuickNoteTag,
    deleteQuickNoteTag,
    deleteQuickNotePermanently,
    emptyQuickNotesTrash,
    getQuickNote,
    listQuickNotes,
    listQuickNoteTags,
    QuickNoteTagError,
    renameQuickNoteTag,
    restoreQuickNote,
    reorderQuickNote,
    setQuickNotePinned,
    trashQuickNote,
    unarchiveQuickNote,
    updateQuickNote,
  } from "$lib/api/quick-notes";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { compareQuickNoteTags, quickNoteViewIndexForKey, quickNoteViewShortcut } from "$lib/quick-notes/tags";
  import { applyQuickNoteGroupOrder } from "$lib/quick-notes/masonry";
  import {
    cacheQuickNotesAllWindow,
    cacheQuickNotesViewWindow,
    cacheQuickNoteTags,
    preloadQuickNotesInitialSnapshot,
    readQuickNotesInitialSnapshot,
    readQuickNotesViewWindow,
  } from "$lib/quick-notes/initial-snapshot";
  import type { QuickNote, QuickNotesCollection, QuickNoteTag } from "$lib/quick-notes/types";
  import { listenForQuickNotesChanges, publishQuickNotesChanged } from "$lib/quick-notes/window-sync";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import QuickNoteEditorModal from "./QuickNoteEditorModal.svelte";
  import QuickNoteTagChip from "./QuickNoteTagChip.svelte";
  import QuickNoteTagManager from "./QuickNoteTagManager.svelte";
  import QuickNotesMasonry from "./QuickNotesMasonry.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { cn } from "$lib/utils";

  let {
    onClose,
    mobileLayout = false,
    mobilePanelStyle = "",
  }: {
    onClose: () => void;
    mobileLayout?: boolean;
    mobilePanelStyle?: string;
  } = $props();

  const { t } = getLocalization();
  const theme = getTheme();
  const mobileBackStack = getMobileBackStack();
  const initialSnapshot = readQuickNotesInitialSnapshot();
  let panel = $state<HTMLDivElement | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);
  let searchButton = $state<HTMLButtonElement | null>(null);
  let collection = $state<QuickNotesCollection>("active");
  let renderedCollection = $state<QuickNotesCollection>("active");
  let selectedTagId = $state<string | null>(null);
  let tags = $state<QuickNoteTag[]>(initialSnapshot?.tags ?? []);
  let searchText = $state("");
  let searchOpen = $state(false);
  let animateLayout = $state(false);
  let notes = $state<QuickNote[]>(initialSnapshot?.window.notes ?? []);
  let nextCursor = $state<string | null>(initialSnapshot?.window.nextCursor ?? null);
  let loading = $state(false);
  let initializing = $state(initialSnapshot === null);
  let loadingMore = $state(false);
  let loadError = $state("");
  let editorNote = $state<QuickNote | null | undefined>(undefined);
  let deleteTarget = $state<QuickNote | null>(null);
  let deleteTagTarget = $state<QuickNoteTag | null>(null);
  let confirmEmptyTrash = $state(false);
  let undoMessage = $state("");
  let reorderAnnouncement = $state("");
  let undoAction = $state<(() => Promise<void>) | null>(null);
  let undoTimer: ReturnType<typeof setTimeout> | null = null;
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  let loadGeneration = 0;
  let reorderWorkerRunning = false;
  const pendingReorders = new Map<string, {
    id: string;
    previousId: string | null;
    nextId: string | null;
    tagId: string | null;
  }>();

  const pinnedNotes = $derived(renderedCollection === "active" ? notes.filter((note) => note.pinned) : []);
  const otherNotes = $derived(renderedCollection === "active" ? notes.filter((note) => !note.pinned) : notes);
  const creationColors = $derived(getQuickNoteColor(FALLBACK_COLOR_INDEX, theme.current));
  const reorderable = $derived(
    collection === "active"
      && renderedCollection === "active"
      && searchText.trim() === ""
      && !loading,
  );
  const emptyMessage = $derived(searchText.trim()
    ? t("quickNotes.empty.search")
    : collection === "archive"
      ? t("quickNotes.empty.archive")
      : collection === "trash"
        ? t("quickNotes.empty.trash")
        : null);
  const childOverlayOpen = $derived(editorNote !== undefined || deleteTarget !== null || deleteTagTarget !== null || confirmEmptyTrash);
  const panelClass = $derived(mobileLayout
    ? "surface-dialog fixed z-50 flex flex-col overflow-hidden text-foreground outline-none"
    : "surface-dialog fixed right-2 z-50 flex w-[min(760px,calc(100vw-1rem))] flex-col overflow-hidden text-foreground outline-none");
  const panelStyle = $derived(mobileLayout
    ? `${mobilePanelStyle || "left:calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem);top:calc(var(--visual-viewport-offset-top) + var(--safe-area-top) + var(--mobile-topbar-h) + 0.25rem);width:calc(var(--visual-viewport-width) - var(--safe-area-left) - var(--safe-area-right) - 1rem);height:calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - var(--mobile-topbar-h) - 0.75rem)"};background-color:var(--cal-bg);`
    : "top: calc(var(--titlebar-h) + 4px); height: min(680px, calc(100dvh - var(--titlebar-h) - 12px)); background-color: var(--cal-bg);");
  const compactHeaderButton = $derived(mobileLayout
    ? "flex min-h-12 min-w-12 shrink-0 items-center justify-center rounded-xl text-foreground transition-colors active:bg-accent"
    : "flex size-7 items-center justify-center rounded-md text-foreground transition-colors hover:bg-accent/60");

  async function load(reset = true): Promise<void> {
    const generation = ++loadGeneration;
    const requestedCollection = collection;
    const requestedTagId = requestedCollection === "active" ? selectedTagId : null;
    const requestedSearchText = searchText.trim();
    const requestedCursor = reset ? null : nextCursor;
    if (reset) {
      loading = true;
      loadError = "";
    } else {
      loadingMore = true;
    }
    try {
      const noteWindow = await listQuickNotes(
        requestedCollection,
        requestedSearchText,
        requestedTagId,
        requestedCursor,
      );
      if (generation !== loadGeneration) return;
      notes = reset
        ? noteWindow.notes
        : [...notes, ...noteWindow.notes.filter((note) => !notes.some((current) => current.id === note.id))];
      if (reset) renderedCollection = requestedCollection;
      nextCursor = noteWindow.nextCursor;
      if (reset && requestedCollection === "active" && requestedTagId === null && requestedSearchText === "") {
        cacheQuickNotesAllWindow(noteWindow);
      } else if (reset && requestedSearchText === "") {
        cacheQuickNotesViewWindow(requestedCollection, requestedTagId, noteWindow);
      }
    } catch (error: unknown) {
      if (generation !== loadGeneration) return;
      loadError = error instanceof Error ? error.message : String(error);
    } finally {
      if (generation === loadGeneration) {
        loading = false;
        loadingMore = false;
      }
    }
  }

  function restoreCachedView(nextCollection: QuickNotesCollection, nextTagId: string | null): void {
    const cached = readQuickNotesViewWindow(nextCollection, nextTagId);
    if (!cached) return;
    notes = cached.notes;
    nextCursor = cached.nextCursor;
    renderedCollection = nextCollection;
  }

  function selectCollection(next: QuickNotesCollection): void {
    if (collection === next && selectedTagId === null) return;
    animateLayout = false;
    collection = next;
    selectedTagId = null;
    nextCursor = null;
    if (searchText.trim() === "") restoreCachedView(next, null);
    void load();
  }

  function selectTag(tagId: string | null): void {
    if (collection === "active" && selectedTagId === tagId) return;
    animateLayout = false;
    collection = "active";
    selectedTagId = tagId;
    nextCursor = null;
    if (searchText.trim() === "") restoreCachedView("active", tagId);
    void load();
  }

  async function loadTags(): Promise<void> {
    tags = await listQuickNoteTags();
    cacheQuickNoteTags(tags);
    if (selectedTagId && !tags.some((tag) => tag.id === selectedTagId)) {
      selectedTagId = null;
    }
  }

  async function createTag(name: string): Promise<void> {
    const created = await createQuickNoteTag(crypto.randomUUID(), name);
    tags = [...tags, created].sort(compareQuickNoteTags);
    publishQuickNotesChanged();
  }

  function tagErrorMessage(error: unknown): string {
    if (error instanceof QuickNoteTagError && error.code === "duplicate_name") return t("quickNotes.tag.duplicateName");
    if (error instanceof QuickNoteTagError && error.code === "not_found") return t("quickNotes.tag.notFound");
    return t("quickNotes.tag.renameFailed");
  }

  async function renameTag(tag: QuickNoteTag, name: string): Promise<void> {
    try {
      const renamed = await renameQuickNoteTag(tag.id, name);
      tags = tags.map((current) => current.id === renamed.id ? renamed : current).sort(compareQuickNoteTags);
      cacheQuickNoteTags(tags);
      publishQuickNotesChanged();
    } catch (error: unknown) {
      if (error instanceof QuickNoteTagError && error.code === "not_found") void loadTags();
      throw new Error(tagErrorMessage(error), { cause: error });
    }
  }

  async function confirmDeleteTag(): Promise<void> {
    const target = deleteTagTarget;
    if (!target) return;
    deleteTagTarget = null;
    if (selectedTagId === target.id) selectTag(null);
    await runMutation(async () => {
      await deleteQuickNoteTag(target.id);
      await loadTags();
    });
  }

  function tagTitle(tag: QuickNoteTag, index: number): string {
    return `${tag.name} (${t("calendar.toolbar.shortcutKey", quickNoteViewShortcut(index + 1) ?? "")})`;
  }

  function updateSearch(value: string): void {
    animateLayout = false;
    searchText = value;
    if (value.trim() === "") {
      restoreCachedView(collection, collection === "active" ? selectedTagId : null);
    }
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void load(), 200);
  }

  async function openSearch(): Promise<void> {
    searchOpen = true;
    await tick();
    searchInput?.focus();
  }

  function closeSearch(): void {
    searchOpen = false;
    if (searchText) updateSearch("");
    void tick().then(() => searchButton?.focus());
  }

  function revisionRequest(note: QuickNote): { id: string; expectedRevision: number } {
    return { id: note.id, expectedRevision: note.revision };
  }

  function showUndo(message: string, action: () => Promise<void>): void {
    if (undoTimer) clearTimeout(undoTimer);
    undoMessage = message;
    undoAction = action;
    undoTimer = setTimeout(() => {
      undoMessage = "";
      undoAction = null;
      undoTimer = null;
    }, 5_000);
  }

  async function runMutation(action: () => Promise<unknown>): Promise<void> {
    animateLayout = true;
    loadError = "";
    try {
      await action();
      publishQuickNotesChanged();
      await load();
    } catch (error: unknown) {
      loadError = error instanceof Error ? error.message : String(error);
    }
  }

  function pinNote(note: QuickNote, pinned: boolean): void {
    void runMutation(() => setQuickNotePinned(revisionRequest(note), pinned));
  }

  async function flushPendingReorders(): Promise<void> {
    if (reorderWorkerRunning) return;
    reorderWorkerRunning = true;
    try {
      while (pendingReorders.size > 0) {
        const requests = [...pendingReorders.values()];
        pendingReorders.clear();
        for (const request of requests) await reorderQuickNote(request);
        publishQuickNotesChanged();
      }
    } catch (error: unknown) {
      pendingReorders.clear();
      reorderAnnouncement = t("quickNotes.reorderFailed");
      loadError = error instanceof Error ? error.message : String(error);
      await load();
    } finally {
      reorderWorkerRunning = false;
      if (pendingReorders.size > 0) void flushPendingReorders();
    }
  }

  function reorderNoteGroup(
    orderedIds: readonly string[],
    movedId: string,
    position: number,
    pinned: boolean,
  ): void {
    if (!reorderable) return;
    notes = applyQuickNoteGroupOrder(notes, orderedIds);
    animateLayout = true;
    reorderAnnouncement = t("quickNotes.reordered", position + 1, orderedIds.length);
    pendingReorders.set(pinned ? "pinned" : "other", {
      id: movedId,
      previousId: orderedIds[position - 1] ?? null,
      nextId: orderedIds[position + 1] ?? null,
      tagId: selectedTagId,
    });
    void flushPendingReorders();
  }

  function colorNote(note: QuickNote, color: QuickNote["color"]): void {
    void runMutation(async () => {
      const full = await getQuickNote(note.id);
      await updateQuickNote({
        id: full.id,
        expectedRevision: full.revision,
        title: full.title,
        runs: full.runs,
        color,
        tagId: full.tagId,
        pinned: full.pinned,
      });
    });
  }

  function tagNote(note: QuickNote, tagId: string | null): void {
    void runMutation(async () => {
      const full = await getQuickNote(note.id);
      await updateQuickNote({
        id: full.id,
        expectedRevision: full.revision,
        title: full.title,
        runs: full.runs,
        color: full.color,
        tagId,
        pinned: full.pinned,
      });
    });
  }

  function archiveNote(note: QuickNote): void {
    void runMutation(async () => {
      const archived = await archiveQuickNote(revisionRequest(note));
      showUndo(t("quickNotes.movedToArchive"), async () => {
        await unarchiveQuickNote(revisionRequest(archived));
        publishQuickNotesChanged();
        await load();
      });
    });
  }

  function unarchiveNote(note: QuickNote): void {
    void runMutation(() => unarchiveQuickNote(revisionRequest(note)));
  }

  function trashNote(note: QuickNote): void {
    void runMutation(async () => {
      const trashed = await trashQuickNote(revisionRequest(note));
      showUndo(t("quickNotes.movedToTrash"), async () => {
        await restoreQuickNote(revisionRequest(trashed));
        publishQuickNotesChanged();
        await load();
      });
    });
  }

  function restoreNote(note: QuickNote): void {
    void runMutation(async () => {
      const restored = await restoreQuickNote(revisionRequest(note));
      showUndo(t("quickNotes.restored"), async () => {
        await trashQuickNote(revisionRequest(restored));
        publishQuickNotesChanged();
        await load();
      });
    });
  }

  async function openNote(note: QuickNote): Promise<void> {
    loadError = "";
    try {
      editorNote = await getQuickNote(note.id);
    } catch (error: unknown) {
      loadError = error instanceof Error ? error.message : String(error);
    }
  }

  function noteSaved(saved: QuickNote): void {
    animateLayout = true;
    if (selectedTagId && saved.tagId !== selectedTagId) {
      notes = notes.filter((note) => note.id !== saved.id);
      return;
    }
    const index = notes.findIndex((note) => note.id === saved.id);
    if (index === -1) notes = [saved, ...notes];
    else notes = notes.map((note) => note.id === saved.id ? saved : note);
  }

  function closeEditorAnd(action: (note: QuickNote) => void, note: QuickNote): void {
    editorNote = undefined;
    action(note);
  }

  async function confirmDelete(): Promise<void> {
    const target = deleteTarget;
    if (!target) return;
    deleteTarget = null;
    editorNote = undefined;
    await runMutation(() => deleteQuickNotePermanently(target.id));
  }

  async function emptyTrash(): Promise<void> {
    confirmEmptyTrash = false;
    await runMutation(() => emptyQuickNotesTrash());
  }

  function handlePanelKeydown(event: KeyboardEvent): void {
    if (editorNote !== undefined || deleteTarget || deleteTagTarget || confirmEmptyTrash) return;
    const target = event.target instanceof Element ? event.target : null;
    const targetDialog = target?.closest("[role='dialog']") ?? null;
    if (event.key === "Escape") {
      if (document.documentElement.dataset.quickNoteDragging === "true") return;
      if (target?.closest("[data-quick-note-tag-creator]") !== null) return;
      if (target?.closest("[role='menu']") !== null) return;
      if (targetDialog !== null && targetDialog !== panel) return;
      event.preventDefault();
      event.stopPropagation();
      if (searchOpen) {
        closeSearch();
        return;
      }
      onClose();
      return;
    }
    const shortcutIndex = quickNoteViewIndexForKey(event.key);
    const shortcutBlocked = event.ctrlKey || event.metaKey || event.altKey || event.shiftKey
      || target?.closest("input, textarea, [contenteditable='true']") !== null
      || target?.closest("[role='menu']") !== null
      || (targetDialog !== null && targetDialog !== panel);
    if (shortcutIndex !== null && !shortcutBlocked) {
      if (shortcutIndex === 0) {
        event.preventDefault();
        selectTag(null);
      } else {
        const tag = tags[shortcutIndex - 1];
        if (tag) {
          event.preventDefault();
          selectTag(tag.id);
        }
      }
      return;
    }
    if (event.key !== "Tab" || !panel) return;
    const focusable = [...panel.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])",
    )].filter((element) => element.offsetParent !== null);
    const first = focusable[0];
    const last = focusable.at(-1);
    const active = document.activeElement;
    if (first && last && (active === panel || !panel.contains(active))) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    } else if (first && last && event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (first && last && !event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function observeMore(node: HTMLElement): { destroy: () => void } {
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting) && nextCursor && !loadingMore) {
        animateLayout = true;
        void load(false);
      }
    }, { root: panel, rootMargin: "160px" });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  $effect(() => {
    if (!mobileLayout || !searchOpen) return;
    return mobileBackStack.activate({ handle: closeSearch });
  });

  onMount(() => {
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const fallbackReturnFocus = document.querySelector<HTMLElement>("[data-mobile-quick-notes-trigger]");
    window.addEventListener("keydown", handlePanelKeydown, true);
    if (initialSnapshot === null) {
      void preloadQuickNotesInitialSnapshot().then((snapshot) => {
        if (loadGeneration !== 0) return;
        tags = snapshot.tags;
        notes = snapshot.window.notes;
        nextCursor = snapshot.window.nextCursor;
      }).catch((error: unknown) => {
        loadError = error instanceof Error ? error.message : String(error);
      }).finally(() => {
        initializing = false;
      });
    }
    void tick().then(() => {
      const firstControl = panel?.querySelector<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])",
      );
      (firstControl ?? panel)?.focus();
    });
    let stopSync: (() => void) | null = null;
    let disposed = false;
    void listenForQuickNotesChanges(() => {
      if (disposed) return;
      animateLayout = true;
      void loadTags();
      void load();
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
        return;
      }
      stopSync = unlisten;
    });
    return () => {
      disposed = true;
      window.removeEventListener("keydown", handlePanelKeydown, true);
      stopSync?.();
      if (searchTimer) clearTimeout(searchTimer);
      if (undoTimer) clearTimeout(undoTimer);
      queueMicrotask(() => {
        if (returnFocus?.isConnected) returnFocus.focus();
        else fallbackReturnFocus?.focus();
      });
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="fixed inset-0 z-40" onclick={(event) => { if (event.target === event.currentTarget) onClose(); }}></div>
<div
  bind:this={panel}
  class={panelClass}
  style={panelStyle}
  role="dialog"
  aria-modal="true"
  aria-label={t("quickNotes.title")}
  aria-hidden={childOverlayOpen ? "true" : undefined}
  inert={childOverlayOpen}
  tabindex="-1"
  data-app-shortcuts="ignore"
>
  <header class={mobileLayout ? "shrink-0 px-2 pb-2 pt-2" : "shrink-0 px-3 pb-1.5 pt-3 sm:px-4"}>
    <div class={mobileLayout ? "flex min-w-0 flex-col gap-1" : "flex min-w-0 items-center gap-1"}>
      <div class={mobileLayout ? "flex min-h-12 w-full min-w-0 items-center gap-1 overflow-x-auto" : "flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"} role="group" aria-label={t("quickNotes.title")}>
        <button
          type="button"
          aria-pressed={collection === "active" && selectedTagId === null}
          class={`flex shrink-0 items-center text-foreground transition-colors ${mobileLayout ? "min-h-12 rounded-xl px-4 text-sm active:bg-accent" : "h-7 rounded-md px-2.5 text-xs hover:bg-accent/60"} ${collection === "active" && selectedTagId === null ? "bg-accent/60" : ""}`}
          title={`${t("quickNotes.collection.active")} (${t("calendar.toolbar.shortcutKey", "1")})`}
          onclick={() => selectTag(null)}
        >{t("quickNotes.collection.active")}</button>
        {#each tags as tag, index (tag.id)}
          <QuickNoteTagChip
            {tag}
            selected={collection === "active" && selectedTagId === tag.id}
            title={tagTitle(tag, index)}
            {mobileLayout}
            onSelect={() => selectTag(tag.id)}
            onRename={(name) => renameTag(tag, name)}
            onDelete={() => { deleteTagTarget = tag; }}
          />
        {/each}
        <QuickNoteTagManager tagCount={tags.length} onCreate={createTag} {mobileLayout} />
      </div>
      <div class={mobileLayout ? "flex min-h-12 w-full min-w-0 items-center gap-1" : "ml-auto flex shrink-0 items-center gap-1"} role="group" aria-label={t("quickNotes.title")}>
        {#if mobileLayout && !searchOpen}<h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">{t("quickNotes.title")}</h2>{/if}
        {#if searchOpen}
          <div class={mobileLayout ? "relative min-w-0 flex-1" : "relative w-24 sm:w-48"}>
            <Search class={mobileLayout ? "pointer-events-none absolute left-3 top-1/2 size-5 -translate-y-1/2 text-muted-foreground" : "pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"} strokeWidth={1.5} />
            <input
              bind:this={searchInput}
              type="search"
              value={searchText}
              class={mobileLayout ? "field min-h-12 w-full pl-10 pr-12 text-base" : "field w-full pl-8 pr-7 text-xs"}
              placeholder={t("quickNotes.search")}
              aria-label={t("quickNotes.search")}
              oninput={(event) => updateSearch(event.currentTarget.value)}
            />
            <button type="button" class={mobileLayout ? "absolute right-0 top-1/2 flex size-12 -translate-y-1/2 items-center justify-center rounded-xl active:bg-accent" : "absolute right-1 top-1/2 flex size-5 -translate-y-1/2 items-center justify-center rounded hover:bg-accent"} aria-label={t("quickNotes.clearSearch")} onclick={closeSearch}><X class={mobileLayout ? "size-5" : "size-3"} strokeWidth={1.5} /></button>
          </div>
        {:else}
          <button
            bind:this={searchButton}
            type="button"
            aria-label={t("quickNotes.search")}
            title={t("quickNotes.search")}
            class={compactHeaderButton}
            onclick={() => void openSearch()}
          ><Search class={mobileLayout ? "size-5" : "size-3.5"} strokeWidth={1.5} /></button>
        {/if}
        <button
          type="button"
          aria-pressed={collection === "archive"}
          aria-label={t("quickNotes.collection.archive")}
          title={t("quickNotes.collection.archive")}
          class={`${compactHeaderButton} ${collection === "archive" ? "bg-accent" : ""}`}
          onclick={() => selectCollection("archive")}
        ><Archive class={mobileLayout ? "size-5" : "size-3.5"} strokeWidth={1.5} /></button>
        <button
          type="button"
          aria-pressed={collection === "trash"}
          aria-label={t("quickNotes.collection.trash")}
          title={t("quickNotes.collection.trash")}
          class={`${compactHeaderButton} ${collection === "trash" ? "bg-accent" : ""}`}
          onclick={() => selectCollection("trash")}
        ><Trash2 class={mobileLayout ? "size-5" : "size-3.5"} strokeWidth={1.5} /></button>
        <button
          type="button"
          class={compactHeaderButton}
          aria-label={t("common.close")}
          title={t("common.close")}
          onclick={onClose}
        ><X class={mobileLayout ? "size-5" : "size-4"} strokeWidth={1.5} /></button>
      </div>
    </div>
  </header>

  <div data-quick-notes-scroll use:scrollEdgeFadeAction class={mobileLayout ? "min-h-0 flex-1 overscroll-contain overflow-y-auto px-3 pb-4 pt-2" : "min-h-0 flex-1 overflow-y-auto px-3 pb-3 pt-1.5 sm:px-4"} aria-busy={loading}>
    {#if renderedCollection === "active"}
      <button
        type="button"
        class={`mx-auto mb-4 flex min-h-12 items-center rounded-xl px-4 text-left text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${mobileLayout ? "w-full" : "w-2/3"}`}
        style="background-color: {creationColors.bg}; color: {creationColors.text};"
        onclick={() => { editorNote = null; }}
      >{t("quickNotes.takeNote")}</button>
    {:else if renderedCollection === "trash"}
      <div class="mb-3 flex items-center gap-2 rounded-lg bg-muted/45 px-3 py-2">
        <p class="min-w-0 flex-1 text-xs text-muted-foreground">{t("quickNotes.trashRetention")}</p>
        {#if notes.length > 0}<button type="button" class={mobileLayout ? "min-h-12 shrink-0 rounded-xl px-3 text-xs text-destructive active:bg-destructive/10" : "shrink-0 rounded-md px-2 py-1 text-xs text-destructive hover:bg-destructive/10"} onclick={() => { confirmEmptyTrash = true; }}>{t("quickNotes.action.emptyTrash")}</button>{/if}
      </div>
    {/if}

    {#if loadError && notes.length === 0}
      <div class="flex min-h-40">
        <Notice layout="fill" class="text-sm" message={t("quickNotes.loadFailed")} copy={loadError} retryLabel={t("quickNotes.retry")} onRetry={() => load()} />
      </div>
    {:else if !initializing && notes.length === 0}
      {#if emptyMessage}<div class="flex min-h-40 items-center justify-center text-center text-sm text-muted-foreground">{emptyMessage}</div>{/if}
    {:else}
      {#if pinnedNotes.length > 0}
        <h3 class="mb-2 px-1 text-[0.7rem] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{t("quickNotes.pinned")}</h3>
        <QuickNotesMasonry notes={pinnedNotes} collection={renderedCollection} {animateLayout} {reorderable} theme={theme.current} {tags} {mobileLayout} onOpen={(note) => void openNote(note)} onReorder={(ids, id, position) => reorderNoteGroup(ids, id, position, true)} onPin={pinNote} onColor={colorNote} onTag={tagNote} onArchive={archiveNote} onUnarchive={unarchiveNote} onTrash={trashNote} onRestore={restoreNote} onDelete={(note) => { deleteTarget = note; }} />
      {/if}
      {#if otherNotes.length > 0}
        {#if pinnedNotes.length > 0}<h3 class="mb-2 mt-5 px-1 text-[0.7rem] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{t("quickNotes.others")}</h3>{/if}
        <QuickNotesMasonry notes={otherNotes} collection={renderedCollection} {animateLayout} {reorderable} theme={theme.current} {tags} {mobileLayout} onOpen={(note) => void openNote(note)} onReorder={(ids, id, position) => reorderNoteGroup(ids, id, position, false)} onPin={pinNote} onColor={colorNote} onTag={tagNote} onArchive={archiveNote} onUnarchive={unarchiveNote} onTrash={trashNote} onRestore={restoreNote} onDelete={(note) => { deleteTarget = note; }} />
      {/if}
      {#if nextCursor}
        <button use:observeMore type="button" class={cn("mt-4 flex w-full items-center justify-center gap-1.5 text-xs text-muted-foreground", mobileLayout ? "min-h-12 rounded-xl px-3 active:bg-accent" : "rounded-md py-2 hover:bg-accent")} disabled={loadingMore} aria-busy={loadingMore || undefined} onclick={() => { animateLayout = true; void load(false); }}>{#if loadingMore}<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />{/if}{t("quickNotes.action.loadMore")}</button>
      {/if}
    {/if}
    {#if loadError && notes.length > 0}<Notice class="mt-3 justify-center text-xs" message={loadError} />{/if}
    <p class="sr-only" aria-live="polite">{reorderAnnouncement}</p>
  </div>

  {#if undoAction}
    <div class={`surface-floating absolute bottom-3 left-1/2 z-10 flex -translate-x-1/2 items-center gap-3 px-3 py-2 ${mobileLayout ? "max-w-[calc(100%-1.5rem)]" : ""}`}>
      <span>{undoMessage}</span>
      <button type="button" class={mobileLayout ? "min-h-12 shrink-0 px-2 font-semibold underline" : "font-semibold underline"} onclick={() => { const action = undoAction; undoAction = null; undoMessage = ""; if (action) void action(); }}>{t("quickNotes.action.undo")}</button>
    </div>
  {/if}
</div>

{#if editorNote !== undefined}
  <QuickNoteEditorModal
    note={editorNote}
    {tags}
    defaultTagId={selectedTagId}
    theme={theme.current}
    {mobileLayout}
    obscured={deleteTarget !== null}
    onClose={() => { editorNote = undefined; void load(); }}
    onSaved={noteSaved}
    onArchive={(note) => closeEditorAnd(archiveNote, note)}
    onUnarchive={(note) => closeEditorAnd(unarchiveNote, note)}
    onTrash={(note) => closeEditorAnd(trashNote, note)}
    onRestore={(note) => closeEditorAnd(restoreNote, note)}
    onDelete={(note) => { deleteTarget = note; }}
  />
{/if}

{#if deleteTarget}
  <ConfirmDialog
    title={t("quickNotes.deleteConfirmTitle")}
    message={t("quickNotes.deleteConfirmMessage")}
    confirmLabel={t("quickNotes.deleteConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void confirmDelete()}
    onCancel={() => { deleteTarget = null; }}
  />
{/if}

{#if deleteTagTarget}
  <ConfirmDialog
    title={t("quickNotes.tag.deleteConfirmTitle", deleteTagTarget.name)}
    message={t("quickNotes.tag.deleteConfirmMessage")}
    confirmLabel={t("quickNotes.tag.delete")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void confirmDeleteTag()}
    onCancel={() => { deleteTagTarget = null; }}
  />
{/if}

{#if confirmEmptyTrash}
  <ConfirmDialog
    title={t("quickNotes.emptyTrashConfirmTitle")}
    message={t("quickNotes.emptyTrashConfirmMessage")}
    confirmLabel={t("quickNotes.emptyTrashConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void emptyTrash()}
    onCancel={() => { confirmEmptyTrash = false; }}
  />
{/if}
