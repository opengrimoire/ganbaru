import {
  applyNotesPageTemplate,
  createNotesPageTemplateFromPage,
  deleteNotesPageTemplate,
  duplicateNotesPageTemplate,
  listNotesPageTemplates,
  updateNotesPageTemplate,
} from "$lib/api/notes";
import type {
  NotesLoadedPage,
  NotesPage,
  NotesPageTemplate,
} from "$lib/notes/types";

interface NotesPageTemplatesControllerContext {
  readLoadedPage: () => NotesPage | null;
  flushPendingBlockSaves: () => Promise<void>;
  activateReturnedPage: (loaded: NotesLoadedPage) => Promise<void>;
  queueDescendantHydration: () => void;
  requestPageLoadFocus: () => void;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Own Notes page-template loading and lifecycle mutations. */
export function createNotesPageTemplatesController(
  context: NotesPageTemplatesControllerContext,
) {
  let templates = $state<NotesPageTemplate[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let requestId = 0;

  async function reload(): Promise<void> {
    const currentRequestId = ++requestId;
    loading = true;
    error = null;
    try {
      const next = await listNotesPageTemplates();
      if (currentRequestId !== requestId) return;
      templates = [...next];
    } catch (caught) {
      if (currentRequestId !== requestId) return;
      error = errorMessage(caught);
      throw caught;
    } finally {
      if (currentRequestId === requestId) loading = false;
    }
  }

  async function apply(templateId: string, title?: string): Promise<void> {
    const loaded = await applyNotesPageTemplate(templateId, {
      parent: { type: "workspace", workspace: true },
      title: title?.trim() || null,
    });
    await context.activateReturnedPage(loaded);
    context.queueDescendantHydration();
    context.requestPageLoadFocus();
  }

  async function createFromCurrentPage(name: string): Promise<void> {
    const page = context.readLoadedPage();
    if (!page) return;
    await context.flushPendingBlockSaves();
    const template = await createNotesPageTemplateFromPage({
      id: crypto.randomUUID(),
      source_page_id: page.id,
      name,
    });
    templates = [template, ...templates.filter((item) => item.id !== template.id)];
  }

  async function updateFromCurrentPage(templateId: string): Promise<void> {
    const page = context.readLoadedPage();
    if (!page) return;
    await context.flushPendingBlockSaves();
    const template = await updateNotesPageTemplate(templateId, { source_page_id: page.id });
    templates = templates.map((item) => item.id === template.id ? template : item);
  }

  async function rename(templateId: string, name: string): Promise<void> {
    const template = await updateNotesPageTemplate(templateId, { name });
    templates = templates.map((item) => item.id === template.id ? template : item);
  }

  async function duplicate(templateId: string, name: string): Promise<void> {
    const template = await duplicateNotesPageTemplate(templateId, {
      id: crypto.randomUUID(),
      name,
    });
    templates = [template, ...templates];
  }

  async function remove(templateId: string): Promise<void> {
    const deletedId = await deleteNotesPageTemplate(templateId);
    templates = templates.filter((template) => template.id !== deletedId);
  }

  return {
    get templates(): NotesPageTemplate[] { return templates; },
    get loading(): boolean { return loading; },
    get error(): string | null { return error; },
    reloadPageTemplates: reload,
    applyPageTemplate: apply,
    createPageTemplateFromCurrentPage: createFromCurrentPage,
    updatePageTemplateFromCurrentPage: updateFromCurrentPage,
    renamePageTemplate: rename,
    duplicatePageTemplate: duplicate,
    deletePageTemplate: remove,
  };
}
