import { notesErrorDetail } from "$lib/notes/failure";
import {
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
  /** The project a workspace page created from a template joins, or null for none. */
  readProjectId: () => string | null;
  flushPendingBlockSaves: () => Promise<void>;
  activateReturnedPage: (loaded: NotesLoadedPage) => Promise<void>;
  queueDescendantHydration: () => void;
  requestPageLoadFocus: () => void;
}

/** Own Notes page-template loading and lifecycle mutations. */
export function createNotesPageTemplatesController(
  context: NotesPageTemplatesControllerContext,
) {
  let resource = $state<AsyncResource<NotesPageTemplate[]>>(idleAsyncResource([]));
  const templates = $derived(resource.data);
  const loading = $derived(isAsyncResourceLoading(resource));
  const error = $derived(asyncResourceError(resource));

  function updateTemplates(update: (current: NotesPageTemplate[]) => NotesPageTemplate[]): void {
    resource = updateAsyncResourceData(resource, update);
  }

  async function reload(): Promise<void> {
    const started = startAsyncRequest(resource);
    resource = started.resource;
    try {
      const next = await listNotesPageTemplates();
      resource = resolveAsyncRequest(resource, started.token, [...next]);
    } catch (caught) {
      if (!isCurrentAsyncRequest(resource, started.token)) return;
      resource = failAsyncRequest(resource, started.token, notesErrorDetail(caught));
      throw caught;
    }
  }

  async function apply(templateId: string, title?: string): Promise<void> {
    const loaded = await applyNotesPageTemplate(templateId, {
      parent: { type: "workspace", workspace: true },
      title: title?.trim() || null,
      project_id: context.readProjectId(),
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
    updateTemplates((current) => [template, ...current.filter((item) => item.id !== template.id)]);
  }

  async function updateFromCurrentPage(templateId: string): Promise<void> {
    const page = context.readLoadedPage();
    if (!page) return;
    await context.flushPendingBlockSaves();
    const template = await updateNotesPageTemplate(templateId, { source_page_id: page.id });
    updateTemplates((current) => current.map((item) => item.id === template.id ? template : item));
  }

  async function rename(templateId: string, name: string): Promise<void> {
    const template = await updateNotesPageTemplate(templateId, { name });
    updateTemplates((current) => current.map((item) => item.id === template.id ? template : item));
  }

  async function duplicate(templateId: string, name: string): Promise<void> {
    const template = await duplicateNotesPageTemplate(templateId, {
      id: crypto.randomUUID(),
      name,
    });
    updateTemplates((current) => [template, ...current]);
  }

  async function remove(templateId: string): Promise<void> {
    const deletedId = await deleteNotesPageTemplate(templateId);
    updateTemplates((current) => current.filter((template) => template.id !== deletedId));
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
