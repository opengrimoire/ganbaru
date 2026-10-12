import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
import type { NotesPageBreadcrumbItem } from "$lib/notes/types";

export interface NotesPageSessionOptions {
  initialSelectedPageId: string | null;
  persistSelectedPageId: (pageId: string | null) => void;
  recordRecentPage: (pageId: string) => void;
}

/** Owns selection identity and generation guards for asynchronous page work. */
export class NotesPageSessionController {
  selectedPageId = $state<string | null>(null);
  pageOpenMode = $state<NotesPageOpenMode>("full");
  breadcrumbs = $state<NotesPageBreadcrumbItem[]>([]);
  titleFocus = $state<{ pageId: string | null; requestId: number }>({ pageId: null, requestId: 0 });
  titleDraft = $state<{ pageId: string; title: string } | null>(null);
  generation = 0;

  constructor(private readonly options: NotesPageSessionOptions) {
    this.selectedPageId = options.initialSelectedPageId;
  }

  select(pageId: string | null, openMode?: NotesPageOpenMode): number {
    if (openMode) this.pageOpenMode = openMode;
    this.selectedPageId = pageId;
    this.options.persistSelectedPageId(pageId);
    this.generation += 1;
    return this.generation;
  }

  open(pageId: string, openMode: NotesPageOpenMode): number {
    return this.select(pageId, openMode);
  }

  invalidate(): number {
    this.generation += 1;
    return this.generation;
  }

  isCurrent(generation: number, pageId: string | null = this.selectedPageId): boolean {
    return generation === this.generation && pageId === this.selectedPageId;
  }

  recordRecentIfCurrent(generation: number, pageId: string): boolean {
    if (!this.isCurrent(generation, pageId)) return false;
    this.options.recordRecentPage(pageId);
    return true;
  }

  applyBreadcrumbsIfCurrent(
    generation: number,
    pageId: string,
    breadcrumbs: readonly NotesPageBreadcrumbItem[],
  ): boolean {
    if (!this.isCurrent(generation, pageId)) return false;
    this.breadcrumbs = [...breadcrumbs];
    return true;
  }

  requestTitleFocus(pageId: string): void {
    this.titleFocus = { pageId, requestId: this.titleFocus.requestId + 1 };
  }

  setTitleDraft(pageId: string, title: string): void {
    this.titleDraft = { pageId, title };
  }

  clearTitleDraft(pageId: string): void {
    if (this.titleDraft?.pageId === pageId) this.titleDraft = null;
  }

  titleDraftForPage(pageId: string): string | null {
    return this.titleDraft?.pageId === pageId ? this.titleDraft.title : null;
  }
}
