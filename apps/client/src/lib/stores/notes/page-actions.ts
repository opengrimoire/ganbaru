import {
  archiveNotesPage,
  createNotesChildPageFromBlock,
  createNotesPage,
  duplicateNotesPage,
  hydrateNotesBlocks,
  isNotesPageActive,
  moveNotesPage,
  permanentlyDeleteNotesPage,
  trashNotesPage,
  updateNotesPage,
} from "$lib/api/notes";
import { invalidateNotesPageCoverAssetUrl } from "$lib/api/notes/page-covers";
import { invalidateNotesPageIconAssetUrl } from "$lib/api/notes/page-icons";
import { blockPlainText, createBlockUpdate } from "$lib/notes/blocks/factory";
import { planNotesInsertedBlockFocus, planNotesPageLoadFocus } from "$lib/notes/editor/focus";
import { nextSelectedNotesPageId } from "$lib/notes/pages/selection";
import { notesPageCoverAssetPath } from "$lib/notes/pages/cover";
import { notesPageIconAssetPath } from "$lib/notes/pages/icon";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
import {
  normalizeNotesProjectId,
  notesPageProjectId,
  notesPageProjectProperties,
  type NotesCreatePageOptions,
} from "$lib/notes/project-membership";
import type { NotesPostMutationResult, NotesSidebarMetadataImpact } from "$lib/notes/post-mutation";
import type {
  NotesBlock,
  NotesBlockUpdate,
  NotesFolder,
  NotesLoadedPage,
  NotesPage,
  NotesPageCover,
  NotesPageIcon,
  NotesParent,
} from "$lib/notes/types";
import type { NotesTextSelection } from "$lib/notes/editor/selection";

const START_OF_NOTES_BLOCK_SELECTION: NotesTextSelection = { start: 0, end: 0 };

export interface NotesMovePageOptions {
  preserveSelection?: boolean;
}

interface NotesPageActionsContext {
  readSelectedPageId: () => string | null;
  readPages: () => NotesPage[];
  readAllPages: () => NotesPage[];
  readLoadedPage: () => NotesPage | null;
  readFolders: () => NotesFolder[];
  readBlocksById: () => Record<string, NotesBlock>;
  defaultOpenMode: (projectId: string | null) => NotesPageOpenMode;
  activateReturnedPage: (
    loaded: NotesLoadedPage,
    impact: Exclude<NotesSidebarMetadataImpact, "none">,
    openMode?: NotesPageOpenMode,
  ) => Promise<void>;
  activateProvisionalPage: (loaded: NotesLoadedPage, openMode: NotesPageOpenMode) => void | Promise<void>;
  beginPageCreation: (request: Parameters<typeof createNotesPage>[0], provisional: NotesLoadedPage) => void;
  awaitPageReady: (pageId: string | null) => Promise<void>;
  awaitPageCreationAttempt: (pageId: string) => Promise<"ready" | "failed">;
  discardFailedPageCreation: (pageId: string) => void;
  activateRestoredPage: (page: NotesPage) => Promise<void>;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  selectPage: (pageId: string | null) => Promise<void>;
  selectPageAfterRemoval: (pageId: string | null) => Promise<void>;
  discardRemovedPageWrites: () => void;
  reloadPageBreadcrumb: (pageId: string) => Promise<void>;
  flushBlockSave: (blockId: string) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  flushPendingWrites: () => Promise<void>;
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  resetUndoHistory: (pageId: string) => void;
  requestBlockFocus: (blockId: string | null, selection?: NotesTextSelection | null) => void;
  requestTitleFocus: (pageId: string) => void;
  requestPageLoadFocus: () => void;
  queueDescendantHydration: () => void;
  setFolderCollapsed: (folderId: string, collapsed: boolean) => void;
  prependArchivedPage: (page: NotesPage) => void;
  prependTrashedPage: (page: NotesPage) => void;
  removeArchivedPages: (pageIds: ReadonlySet<string>) => void;
  removeTrashedPages: (pageIds: ReadonlySet<string>) => void;
  removePagesFromActiveCollections: (pageIds: ReadonlySet<string>) => void;
  removeSidebarPageIds: (pageIds: ReadonlySet<string>) => void;
  scheduleHierarchyRefresh: () => void;
  beginPageRemoval: (pageId: string) => ReadonlySet<string> | null;
  endPageRemoval: (pageId: string) => void;
  setTrashActionError: (message: string | null) => void;
}

/** Create Notes page lifecycle operations over the facade-owned reactive state. */
export function createNotesPageActions(context: NotesPageActionsContext) {
  function projectIdForParent(
    parent: NotesParent,
    options: NotesCreatePageOptions,
  ): string | null {
    if ("projectId" in options) return normalizeNotesProjectId(options.projectId);
    const folderId = options.folderId?.trim();
    if (parent.type === "workspace" && folderId) {
      return normalizeNotesProjectId(
        context.readFolders().find((folder) => folder.id === folderId)?.project_id,
      );
    }
    if (parent.type !== "page_id") return null;
    const parentPage = context.readAllPages().find((page) => page.id === parent.page_id)
      ?? (context.readLoadedPage()?.id === parent.page_id ? context.readLoadedPage() : null);
    return parentPage ? notesPageProjectId(parentPage) : null;
  }

  async function createPageWithParent(
    title: string,
    parent: NotesParent,
    options: NotesCreatePageOptions,
  ): Promise<void> {
    try {
      await context.flushPendingWrites();
    } catch {
      // The editor retains its draft and displays the save error.
      return;
    }
    const pageId = crypto.randomUUID();
    const firstBlockId = crypto.randomUUID();
    const projectId = projectIdForParent(parent, options);
    const folderId = parent.type === "workspace" ? options.folderId?.trim() || null : null;
    const request = {
      id: pageId,
      title,
      parent,
      folder_id: folderId,
      first_block_id: firstBlockId,
      after_block_id: null,
      properties: notesPageProjectProperties(projectId),
    };
    const provisional = createProvisionalNotesPage(request);
    if (provisional.page.folder_id) context.setFolderCollapsed(provisional.page.folder_id, false);
    await context.activateProvisionalPage(
      provisional,
      options.openMode ?? context.defaultOpenMode(projectId),
    );
    context.requestBlockFocus(null);
    context.requestTitleFocus(provisional.page.id);
    context.beginPageCreation(request, provisional);
  }

  async function createPage(title: string, options: NotesCreatePageOptions = {}): Promise<void> {
    await createPageWithParent(title, { type: "workspace", workspace: true }, options);
  }

  async function createSubpage(
    parentPageId: string,
    title: string,
    options: NotesCreatePageOptions = {},
  ): Promise<void> {
    await createPageWithParent(title, { type: "page_id", page_id: parentPageId }, options);
  }

  /** Reserve the note row immediately and serialize its lifecycle after earlier editor writes. */
  function createChildPageFromBlock(blockId: string, clearText = false): Promise<void> {
    const sourcePageId = context.readSelectedPageId();
    const block = context.readBlocksById()[blockId];
    if (!sourcePageId || !block || block.type === "child_page") return Promise.resolve();
    const firstBlockId = crypto.randomUUID();
    const page = context.readLoadedPage();
    const request = {
      first_block_id: firstBlockId,
      title: clearText ? "" : blockPlainText(block).trim(),
      properties: notesPageProjectProperties(page ? notesPageProjectId(page) : null),
    };
    context.localApplyBlockUpdate(blockId, createBlockUpdate("child_page", request.title));
    context.resetUndoHistory(sourcePageId);
    context.requestBlockFocus(blockId);
    let loaded: NotesLoadedPage | null = null;
    return context.enqueueEditorMutation(async () => {
      await context.awaitPageReady(sourcePageId);
      loaded ??= await createNotesChildPageFromBlock(blockId, request);
      await context.activateReturnedPage(loaded, "hierarchy");
      context.requestBlockFocus(
        planNotesInsertedBlockFocus([loaded.blocks.results[0]?.id, firstBlockId]),
        START_OF_NOTES_BLOCK_SELECTION,
      );
    });
  }

  async function createChildPageAfterBlock(blockId: string): Promise<void> {
    const sourcePageId = context.readSelectedPageId();
    if (!sourcePageId) return;
    await context.awaitPageReady(sourcePageId);
    const block = context.readBlocksById()[blockId];
    if (!block) return;
    await context.flushPendingWrites();
    const pageId = crypto.randomUUID();
    const firstBlockId = crypto.randomUUID();
    const page = context.readLoadedPage();
    const loaded = await createNotesPage({
      id: pageId,
      title: "",
      parent: block.parent,
      folder_id: null,
      first_block_id: firstBlockId,
      after_block_id: blockId,
      properties: notesPageProjectProperties(page ? notesPageProjectId(page) : null),
    });
    const blocks = await hydrateNotesBlocks({ page_id: sourcePageId, block_ids: [loaded.page.id] });
    context.applyPostMutation({
      blocks,
      placements: [{ blockId: loaded.page.id, parent: block.parent, after: blockId }],
      pages: [loaded.page],
      sidebarImpact: "hierarchy",
    });
    context.resetUndoHistory(sourcePageId);
    await context.activateReturnedPage(loaded, "hierarchy");
    context.requestBlockFocus(
      planNotesInsertedBlockFocus([loaded.blocks.results[0]?.id, firstBlockId]),
      START_OF_NOTES_BLOCK_SELECTION,
    );
  }

  async function renamePage(pageId: string, title: string): Promise<void> {
    await context.awaitPageReady(pageId);
    const page = await updateNotesPage(pageId, { title: title.trim() });
    context.applyPostMutation({ pages: [page] });
    if (context.readSelectedPageId() === page.id) await context.reloadPageBreadcrumb(page.id);
  }

  async function duplicatePage(pageId: string, title: string): Promise<void> {
    await context.flushPendingBlockSaves();
    const loaded = await duplicateNotesPage(pageId, { title });
    if (loaded.page.folder_id) context.setFolderCollapsed(loaded.page.folder_id, false);
    await context.activateReturnedPage(loaded, "hierarchy");
    context.requestBlockFocus(planNotesPageLoadFocus(loaded.blocks.results.map((block) => block.id)));
  }

  async function movePageWithPlacement(
    pageId: string,
    parent: NotesParent,
    folderId: string | null,
    options: NotesMovePageOptions = {},
  ): Promise<void> {
    await context.flushPendingBlockSaves();
    const loaded = await moveNotesPage(pageId, { parent, folder_id: folderId });
    if (loaded.page.folder_id) context.setFolderCollapsed(loaded.page.folder_id, false);
    if (options.preserveSelection) {
      context.applyPostMutation({ pages: [loaded.page], sidebarImpact: "hierarchy" });
      const selectedPageId = context.readSelectedPageId();
      if (selectedPageId) await context.reloadPageBreadcrumb(selectedPageId);
    } else {
      await context.activateReturnedPage(loaded, "hierarchy");
      context.requestPageLoadFocus();
    }
    context.queueDescendantHydration();
  }

  async function updatePageIcon(pageId: string, icon: NotesPageIcon | null): Promise<void> {
    const previous = context.readLoadedPage()?.id === pageId
      ? context.readLoadedPage()
      : context.readAllPages().find((page) => page.id === pageId);
    const previousPath = notesPageIconAssetPath(previous?.icon ?? null);
    const page = await updateNotesPage(pageId, { icon });
    const nextPath = notesPageIconAssetPath(page.icon);
    if (previousPath && previousPath !== nextPath) invalidateNotesPageIconAssetUrl(previousPath);
    context.applyPostMutation({ pages: [page] });
  }

  async function updatePageCover(pageId: string, cover: NotesPageCover | null): Promise<void> {
    const previous = context.readLoadedPage()?.id === pageId
      ? context.readLoadedPage()
      : context.readAllPages().find((page) => page.id === pageId);
    const previousPath = notesPageCoverAssetPath(previous?.cover ?? null);
    const page = await updateNotesPage(pageId, { cover });
    const nextPath = notesPageCoverAssetPath(page.cover);
    if (previousPath && previousPath !== nextPath) invalidateNotesPageCoverAssetUrl(previousPath);
    context.applyPostMutation({ pages: [page] });
  }

  async function trashPage(pageId: string): Promise<void> {
    const visiblePages = context.readPages();
    const removedIds = context.beginPageRemoval(pageId);
    if (!removedIds) return;
    context.setTrashActionError(null);
    let removalConfirmed = false;
    let pendingWriteFailed = false;
    try {
      const failedCreationIds = new Set<string>();
      for (const id of removedIds) {
        if (await context.awaitPageCreationAttempt(id) === "failed") failedCreationIds.add(id);
      }
      const rootIsAlreadyInactive = failedCreationIds.has(pageId)
        && !await isNotesPageActive(pageId);
      let trashed: NotesPage | null = null;
      if (!rootIsAlreadyInactive) {
        try {
          const selectedPageId = context.readSelectedPageId();
          if (selectedPageId && removedIds.has(selectedPageId) && !failedCreationIds.has(selectedPageId)) {
            try {
              await context.flushPendingWrites();
            } catch (error) {
              pendingWriteFailed = true;
              throw error;
            }
          }
          trashed = await trashNotesPage(pageId, true);
        } catch (error) {
          let stillActive: boolean | null = null;
          try {
            stillActive = await isNotesPageActive(pageId);
          } catch (probeError) {
            console.error("check Notes page after Trash failure failed", probeError);
          }
          if (stillActive !== false) throw error;
        }
      }
      removalConfirmed = true;
      const selectedPageId = context.readSelectedPageId();
      if (selectedPageId && removedIds.has(selectedPageId)
        && (pendingWriteFailed || failedCreationIds.has(selectedPageId))) {
        context.discardRemovedPageWrites();
      }
      for (const id of failedCreationIds) context.discardFailedPageCreation(id);
      const preferred = nextSelectedNotesPageId(visiblePages, pageId);
      if (trashed) {
        context.prependTrashedPage(trashed);
        context.removeArchivedPages(removedIds);
      }
      const currentPageId = context.readSelectedPageId();
      context.applyPostMutation({ removedPageIds: [...removedIds], sidebarImpact: "hierarchy" });
      context.removeSidebarPageIds(removedIds);
      if (currentPageId && removedIds.has(currentPageId)) {
        const pages = context.readPages();
        const next = preferred && pages.some((page) => page.id === preferred)
          ? preferred
          : pages[0]?.id ?? null;
        await context.selectPageAfterRemoval(next);
      }
    } catch (error) {
      if (!removalConfirmed) context.setTrashActionError(error instanceof Error ? error.message : String(error));
      throw error;
    } finally {
      context.endPageRemoval(pageId);
    }
  }

  async function archivePage(pageId: string): Promise<void> {
    const archived = await archiveNotesPage(pageId, true);
    context.prependArchivedPage(archived);
    const next = nextSelectedNotesPageId(context.readPages(), pageId);
    context.applyPostMutation({ removedPageIds: [pageId], sidebarImpact: "hierarchy" });
    await context.selectPage(next);
  }

  async function unarchivePage(pageId: string): Promise<void> {
    const page = await archiveNotesPage(pageId, false);
    context.removeArchivedPages(new Set([pageId]));
    await context.activateRestoredPage(page);
  }

  async function restorePage(pageId: string): Promise<void> {
    const page = await trashNotesPage(pageId, false);
    context.removeTrashedPages(new Set([pageId]));
    await context.activateRestoredPage(page);
  }

  async function permanentlyDeletePage(pageId: string): Promise<void> {
    await context.flushPendingBlockSaves();
    const deletedIds = new Set(await permanentlyDeleteNotesPage(pageId));
    context.removePagesFromActiveCollections(deletedIds);
    context.removeArchivedPages(deletedIds);
    context.removeTrashedPages(deletedIds);
    context.removeSidebarPageIds(deletedIds);
    context.scheduleHierarchyRefresh();
    const selectedPageId = context.readSelectedPageId();
    if (selectedPageId && deletedIds.has(selectedPageId)) {
      await context.selectPage(context.readPages()[0]?.id ?? null);
    }
  }

  return {
    createPage,
    createSubpage,
    createChildPageFromBlock,
    createChildPageAfterBlock,
    renamePage,
    duplicatePage,
    movePage: (pageId: string, parent: NotesParent, options?: NotesMovePageOptions) => (
      movePageWithPlacement(pageId, parent, null, options)
    ),
    movePageToFolder: (
      pageId: string,
      folderId: string | null,
      options?: NotesMovePageOptions,
    ) => movePageWithPlacement(
      pageId,
      { type: "workspace", workspace: true },
      folderId?.trim() || null,
      options,
    ),
    updatePageIcon,
    updatePageCover,
    trashPage,
    archivePage,
    unarchivePage,
    restorePage,
    permanentlyDeletePage,
  };
}
