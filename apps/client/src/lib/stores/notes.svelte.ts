import { untrack } from "svelte";
import { createNotesEditorStore, type NotesEditorStore, type NotesSelectPageOptions } from "$lib/stores/notes/editor-session.svelte";
import { saveNotesSelectedPageId } from "$lib/stores/notes/page-state";
import { getProjects } from "./projects.svelte";
import { notesPageProjectId } from "$lib/notes/project-membership";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
import type { NotesLoadedPage } from "$lib/notes/types";
import type { NotesSidebarMetadataImpact } from "$lib/notes/post-mutation";

export interface NotesEditorPane {
  id: string;
  store: NotesEditorStore;
}

/** Coordinate live panes without replacing the main editor when a preview opens. */
function createNotesWorkspace() {
  const projects = getProjects();
  let nextPaneId = 0;
  let main = $state.raw<NotesEditorPane>(createPane(true));
  let preview = $state.raw<NotesEditorPane | null>(null);
  let activePaneId = $state(untrack(() => main.id));
  const active = $derived(preview?.id === activePaneId ? preview : main);
  const panes = $derived(preview ? [main, preview] : [main]);

  function createPane(restoreSelection = false): NotesEditorPane {
    const id = `notes-pane-${++nextPaneId}`;
    const store = createNotesEditorStore({
      select: (pageId, options) => selectFrom(id, pageId, options),
      openLoaded: (loaded, impact, mode) => openLoadedFrom(id, loaded, impact, mode),
      openProvisional: (loaded, mode) => openProvisionalFrom(id, loaded, mode),
      close: () => closePane(id),
      showAs: (mode) => { void showPaneAs(id, mode).catch(reportNavigationError); },
      focusBlock: (blockId, selection, preventScroll) => {
        const target = panes.find((pane) => pane.id !== id && pane.store.blockById(blockId));
        if (!target) return false;
        target.store.focusBlock(blockId, selection, preventScroll);
        return true;
      },
      focusTitle: (pageId) => {
        const target = panes.find((pane) => pane.id !== id && pane.store.selectedPageId === pageId);
        if (!target) return false;
        target.store.requestTitleFocus(pageId);
        return true;
      },
      removePages: (pageIds) => {
        const removed = new Set(pageIds);
        if (preview?.store.selectedPageId && removed.has(preview.store.selectedPageId)) {
          preview.store.clearRemovedPage();
          preview.store.dispose();
          preview = null;
        }
        if (main.store.selectedPageId && removed.has(main.store.selectedPageId)) main.store.clearRemovedPage();
        if (!panes.some((pane) => pane.id === activePaneId)) activatePane(main.id);
      },
      afterRemoval: async (replacementId) => {
        if (!main.store.selectedPageId) await main.store.selectPageLocally(replacementId, { openMode: "full" });
        activatePane(main.id);
      },
    }, restoreSelection);
    return { id, store };
  }

  function reportNavigationError(error: unknown): void {
    console.warn("Notes pane navigation failed", error);
  }

  function paneById(id: string): NotesEditorPane {
    const pane = panes.find((candidate) => candidate.id === id);
    if (!pane) throw new Error("Notes editor is no longer open");
    return pane;
  }

  function activatePane(id: string, restoreProjectContext = true): void {
    if (!panes.some((pane) => pane.id === id)) return;
    const pane = paneById(id);
    if (activePaneId !== id) {
      activePaneId = id;
      saveNotesSelectedPageId(pane.store.selectedPageId);
    }
    if (!restoreProjectContext) return;
    void synchronizeProjectContext(pane).catch((error: unknown) => {
      console.warn("Refresh Notes project context failed", error);
    });
  }

  /** Follow the active document's owning project without reopening its content. */
  async function synchronizeProjectContext(pane: NotesEditorPane): Promise<void> {
    const page = pane.store.loadedPage;
    if (!page || page.id !== pane.store.selectedPageId) return;
    const projectId = notesPageProjectId(page);
    if (!projectId) return;
    if (!projects.loaded) await projects.ensureLoaded();
    if (activePaneId !== pane.id
      || pane.store.selectedPageId !== page.id
      || !pane.store.loadedPage
      || notesPageProjectId(pane.store.loadedPage) !== projectId) return;
    if (!projects.projectById(projectId) || projects.selectedProjectId === projectId) return;
    projects.selectedProjectId = projectId;
    await pane.store.refreshProjectNavigation();
  }

  async function closePreview(restoreProjectContext = true): Promise<void> {
    const closing = preview;
    if (!closing) return;
    const pageId = closing.store.selectedPageId;
    await closing.store.prepareToClose();
    if (preview !== closing || closing.store.selectedPageId !== pageId) return;
    preview = null;
    closing.store.dispose();
    activatePane(main.id, restoreProjectContext);
  }

  async function closePane(id: string): Promise<void> {
    if (preview?.id === id) await closePreview();
    else if (main.id === id) {
      await closePreview(false);
      await main.store.selectPageLocally(null);
      activatePane(main.id);
    }
  }

  async function showPaneAs(id: string, mode: NotesPageOpenMode): Promise<void> {
    const pane = paneById(id);
    if (mode === "full" && pane === preview) {
      const previousMain = main;
      await previousMain.store.prepareToClose();
      if (main !== previousMain || preview !== pane) return;
      main = pane;
      preview = null;
      previousMain.store.dispose();
    }
    pane.store.setOpenMode(mode);
    activatePane(pane.id);
  }

  async function destination(sourceId: string, pageId: string, mode: NotesPageOpenMode): Promise<NotesEditorPane> {
    const source = paneById(sourceId);
    const existing = panes.find((pane) => pane.store.selectedPageId === pageId);
    if (existing) {
      if (existing === main && preview && (mode === "full" || preview.store.pageOpenMode === "center")) await closePreview(false);
      if (mode === "full") await showPaneAs(existing.id, mode);
      activatePane(existing.id);
      return existing;
    }
    if (mode === "full") {
      await closePreview(false);
      activatePane(main.id);
      return main;
    }
    if (!main.store.selectedPageId) {
      activatePane(main.id);
      return main;
    }
    if (source === preview) return source;
    if (preview) {
      const replacing = preview;
      await replacing.store.flushPendingWrites();
      if (preview !== replacing) return destination(sourceId, pageId, mode);
    } else preview = createPane();
    main.store.setOpenMode("full");
    activatePane(preview.id);
    return preview;
  }

  async function selectFrom(sourceId: string, pageId: string | null, options: NotesSelectPageOptions = {}): Promise<void> {
    if (!pageId) {
      await closePreview(false);
      await main.store.selectPageLocally(null, options);
      activatePane(main.id);
      return;
    }
    const source = paneById(sourceId);
    if (options.databaseBlockId) {
      const existing = panes.find((pane) => pane.store.selectedPageId === pageId);
      const target = existing ?? await destination(sourceId, pageId, "side");
      await target.store.selectPageLocally(pageId, {
        ...options,
        openMode: target.store.pageOpenMode,
      });
      if (target.store.selectedDatabaseBlockId === options.databaseBlockId) activatePane(target.id);
      return;
    }
    const mode = options.openMode ?? (source.store.selectedPageId ? source.store.pageOpenMode : source.store.defaultOpenMode());
    const target = await destination(sourceId, pageId, mode);
    await target.store.selectPageLocally(pageId, { ...options, openMode: target === main && preview ? "full" : mode });
    activatePane(target.id);
  }

  async function openLoadedFrom(sourceId: string, loaded: NotesLoadedPage, impact: Exclude<NotesSidebarMetadataImpact, "none">, mode: NotesPageOpenMode): Promise<void> {
    const target = await destination(sourceId, loaded.page.id, mode);
    await target.store.acceptLoadedPage(loaded, impact, mode);
    activatePane(target.id);
  }

  async function openProvisionalFrom(sourceId: string, loaded: NotesLoadedPage, mode: NotesPageOpenMode): Promise<void> {
    const target = await destination(sourceId, loaded.page.id, mode);
    target.store.acceptProvisionalPage(loaded, mode);
    activatePane(target.id);
  }

  const workspace = {
    get editorPanes(): readonly NotesEditorPane[] { return panes; },
    get mainPaneId(): string { return main.id; },
    get previewPane(): NotesEditorPane | null { return preview; },
    get activePaneId(): string { return activePaneId; },
    activatePane,
    closePane,
    showPaneAs,
    closeContextualPage: () => preview ? closePreview() : closePane(main.id),
    // Workspace navigation always acts on the pane currently receiving user input.
    selectPage: (pageId: string | null, options?: NotesSelectPageOptions) => selectFrom(active.id, pageId, options),
    load: () => preview ? closePreview(false).then(() => main.store.load()) : main.store.load(),
    ensureLoaded: () => main.store.ensureLoaded(),
    loadMoreWorkspaceWindow: () => active.store.loadMoreWorkspaceWindow(),
    flushPendingWrites: async () => {
      for (const pane of panes) await pane.store.flushPendingWrites();
    },
  };
  return new Proxy(workspace as NotesEditorStore & typeof workspace, {
    get(target, key): unknown {
      return Object.hasOwn(workspace, key) ? Reflect.get(target, key) : Reflect.get(active.store, key);
    },
    set(_target, key, value: unknown): boolean {
      return Reflect.set(active.store, key, value);
    },
  });
}

const notes = createNotesWorkspace();

/** Shared workspace controls and the live editor that currently owns input focus. */
export function getNotes() { return notes; }
