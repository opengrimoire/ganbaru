import {
  importNotesHtmlPage,
  importNotesNotionApi,
  importNotesNotionExportFolder,
  saveNotesAgentBridge,
  saveNotesHtmlArchive,
  saveNotesJsonGraph,
} from "$lib/api/notes";
import type {
  NotesAgentBridgeExportRequest,
  NotesAgentBridgeExportSaveResult,
  NotesHtmlArchiveSaveResult,
  NotesHtmlExportRequest,
  NotesHtmlImportRequest,
  NotesHtmlImportResult,
  NotesJsonGraphExportRequest,
  NotesJsonGraphExportSaveResult,
  NotesLoadedPage,
  NotesNotionApiImportRequest,
  NotesNotionApiImportResult,
  NotesNotionExportImportRequest,
  NotesNotionExportImportResult,
  NotesPage,
  NotesParent,
} from "$lib/notes/types";

interface NotesTransferActionsContext {
  readSelectedPageId: () => string | null;
  activateReturnedPage: (loaded: NotesLoadedPage) => Promise<void>;
  upsertPage: (page: NotesPage) => void;
  showPages: () => void;
  scheduleHierarchyRefresh: () => void;
  queueDescendantHydration: () => void;
  requestPageLoadFocus: () => void;
}

/** Create Notes import and export operations around the canonical page activation flow. */
export function createNotesTransferActions(context: NotesTransferActionsContext) {
  async function importHtmlPage(
    input: Omit<NotesHtmlImportRequest, "parent"> & { parent?: NotesParent },
  ): Promise<NotesHtmlImportResult> {
    const result = await importNotesHtmlPage({
      ...input,
      parent: input.parent ?? { type: "workspace", workspace: true },
    });
    await context.activateReturnedPage(result.page);
    context.queueDescendantHydration();
    context.requestPageLoadFocus();
    return result;
  }

  async function refreshAfterMultiPageImport(importedPages: NotesLoadedPage[]): Promise<void> {
    context.showPages();
    const firstPage = importedPages[0] ?? null;
    for (const loaded of importedPages) context.upsertPage(loaded.page);
    if (firstPage) {
      await context.activateReturnedPage(firstPage);
      context.queueDescendantHydration();
      context.requestPageLoadFocus();
    } else {
      context.scheduleHierarchyRefresh();
    }
  }

  async function importNotionApi(
    input: Omit<NotesNotionApiImportRequest, "parent"> & { parent?: NotesParent },
  ): Promise<NotesNotionApiImportResult> {
    const result = await importNotesNotionApi({
      ...input,
      parent: input.parent ?? { type: "workspace", workspace: true },
    });
    await refreshAfterMultiPageImport(result.imported_pages);
    return result;
  }

  async function importNotionExportFolder(
    input: Omit<NotesNotionExportImportRequest, "parent"> & { parent?: NotesParent },
  ): Promise<NotesNotionExportImportResult> {
    const result = await importNotesNotionExportFolder({
      ...input,
      parent: input.parent ?? { type: "workspace", workspace: true },
    });
    await refreshAfterMultiPageImport(result.imported_pages);
    return result;
  }

  async function exportHtmlArchive(
    input: Omit<NotesHtmlExportRequest, "page_id"> = {},
  ): Promise<NotesHtmlArchiveSaveResult> {
    const pageId = context.readSelectedPageId();
    if (!pageId) throw new Error("No Notes page is selected");
    return saveNotesHtmlArchive({ ...input, page_id: pageId });
  }

  async function exportJsonGraph(
    input: NotesJsonGraphExportRequest = {},
  ): Promise<NotesJsonGraphExportSaveResult> {
    return saveNotesJsonGraph(input);
  }

  async function exportAgentBridge(
    input: Omit<NotesAgentBridgeExportRequest, "page_ids"> = {},
  ): Promise<NotesAgentBridgeExportSaveResult> {
    const pageId = context.readSelectedPageId();
    if (!pageId) throw new Error("No Notes page is selected");
    return saveNotesAgentBridge({ ...input, page_ids: [pageId] });
  }

  return {
    importHtmlPage,
    importNotionApi,
    importNotionExportFolder,
    exportHtmlArchive,
    exportJsonGraph,
    exportAgentBridge,
  };
}
