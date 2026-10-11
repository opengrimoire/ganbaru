<script module lang="ts">
  import type { NotesDatabaseMentionRichText, NotesPageMentionRichText, NotesRichText } from "$lib/notes/types";

  /** Narrow rich text to the local references that support navigation and metadata previews. */
  export function isNotesLocalReference(item: NotesRichText): item is NotesPageMentionRichText | NotesDatabaseMentionRichText {
    return item.type === "mention" && (item.mention.type === "page" || item.mention.type === "database");
  }
</script>

<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { isNotesUuid } from "$lib/notes/links/block-link";
  import { notesFloatingPanelContentHeight, notesFloatingPanelPlacement, type NotesFloatingPanelPlacement } from "$lib/notes/editor/floating-panel";
  import { notesLinkTarget, openNotesTextLink } from "$lib/notes/links/navigation";
  import type { NotesDatabaseReference, NotesPageBreadcrumbItem } from "$lib/notes/types";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import Database from "@lucide/svelte/icons/database";
  import FileText from "@lucide/svelte/icons/file-text";

  interface ReferenceAttributes {
    class?: string;
    style?: string;
    [name: `data-${string}`]: string | undefined;
  }

  const PREVIEW_OPEN_DELAY_MS = 300;
  const PREVIEW_CLOSE_DELAY_MS = 120;
  const PREVIEW_WIDTH_PX = 300;
  const POINTER_DRAG_DISTANCE_PX = 5;
  const THUMBNAIL_ROWS = [0, 1, 2] as const;
  const THUMBNAIL_COLUMNS = [0, 1, 2] as const;

  let { reference, text, showIcon = true, attributes = {} }: {
    reference: NotesPageMentionRichText | NotesDatabaseMentionRichText;
    text: string;
    showIcon?: boolean;
    attributes?: ReferenceAttributes;
  } = $props();

  const { t } = getLocalization();
  const componentId = $props.id();
  const previewId = `${componentId}-reference-preview`;
  const referenceType = $derived(reference.mention.type);
  const referenceId = $derived(reference.mention.type === "page" ? reference.mention.page.id : reference.mention.database.id);
  const referenceKey = $derived(`${referenceType}:${referenceId}`);
  const isValidReference = $derived(isNotesUuid(referenceId));
  let anchor = $state<HTMLAnchorElement | null>(null);
  let card = $state<HTMLDivElement | null>(null);
  let cardBody = $state<HTMLDivElement | null>(null);
  let previewOpen = $state(false);
  let previewStatus = $state<"idle" | "loading" | "ready" | "failed">("idle");
  let breadcrumbs = $state<readonly NotesPageBreadcrumbItem[]>([]);
  let database = $state<NotesDatabaseReference | null>(null);
  let hasNavigationError = $state(false);
  let navigating = $state(false);
  let placement = $state<NotesFloatingPanelPlacement>({ left: 0, top: 0, width: PREVIEW_WIDTH_PX, maxHeight: 0 });
  let openTimer: ReturnType<typeof setTimeout> | null = null;
  let closeTimer: ReturnType<typeof setTimeout> | null = null;
  let databaseRequest: Promise<NotesDatabaseReference> | null = null;
  let pointerStart: { x: number; y: number } | null = null;
  let revision = 0;
  let disposed = false;

  const providedTarget = $derived(reference.href
    ? notesLinkTarget(reference.href, typeof window === "undefined" ? undefined : window.location.href) : null);
  const destination = $derived(!isValidReference ? null : referenceType === "page"
    ? `#notes?page=${referenceId}`
    : database ? `#notes?page=${database.page_id}&block=${database.block_id}`
      : providedTarget?.blockId === referenceId ? `#notes?page=${providedTarget.pageId}&block=${referenceId}` : null);
  const href = $derived(destination ?? (isValidReference ? `#notes?block=${referenceId}` : undefined));
  const typeLabel = $derived(referenceType === "database" ? t("notes.blockType.childDatabase") : t("notes.blockType.childPage"));
  const previewTitle = $derived((referenceType === "database" ? database?.title
    : breadcrumbs.find((item) => item.current)?.title)?.trim() || reference.plain_text.trim() || t("notes.untitled"));
  const containingPath = $derived(breadcrumbs
    .filter((item) => referenceType === "database" || (!item.current && item.id !== referenceId))
    .map((item) => item.title.trim() || t("notes.untitled")).join(" / "));

  /** Close the preview without changing native text selection or the active editor. */
  function closePreview(): void {
    if (openTimer !== null) clearTimeout(openTimer);
    if (closeTimer !== null) clearTimeout(closeTimer);
    openTimer = null;
    closeTimer = null;
    previewOpen = false;
  }

  $effect(() => {
    void referenceKey;
    revision += 1;
    closePreview();
    databaseRequest = null;
    database = null;
    breadcrumbs = [];
    previewStatus = "idle";
    hasNavigationError = false;
    navigating = false;
    pointerStart = null;
    return () => { revision += 1; closePreview(); };
  });

  onDestroy(() => { disposed = true; revision += 1; closePreview(); });

  /** Share an in-flight metadata request between hovering and destination resolution. */
  async function resolveDatabase(): Promise<NotesDatabaseReference> {
    if (database) return database;
    if (databaseRequest) return databaseRequest;
    const requestRevision = revision;
    const blockId = referenceId;
    const request = import("$lib/api/notes").then((api) => api.getNotesDatabaseReference(blockId));
    databaseRequest = request;
    try {
      const result = await request;
      if (!disposed && requestRevision === revision) database = result;
      return result;
    } finally {
      if (databaseRequest === request) databaseRequest = null;
    }
  }

  /** Read titles and ancestry only, leaving database rows and page content unloaded. */
  async function loadPreview(): Promise<void> {
    if (previewStatus === "loading" || previewStatus === "ready") return;
    const requestRevision = revision;
    const pageId = referenceId;
    const isDatabase = referenceType === "database";
    previewStatus = "loading";
    try {
      const ownerId = isDatabase ? (await resolveDatabase()).page_id : pageId;
      if (disposed || requestRevision !== revision) return;
      const api = await import("$lib/api/notes");
      const path = await api.getNotesPageBreadcrumb(ownerId);
      if (disposed || requestRevision !== revision) return;
      breadcrumbs = path;
      previewStatus = "ready";
    } catch (error: unknown) {
      if (disposed || requestRevision !== revision) return;
      previewStatus = "failed";
      console.warn("Notes reference preview failed", { referenceId: pageId, error });
    }
  }

  function keepPreviewOpen(): void {
    if (closeTimer !== null) clearTimeout(closeTimer);
    closeTimer = null;
  }

  function schedulePreview(event: PointerEvent): void {
    if (!isValidReference || event.pointerType === "touch" || event.buttons) return;
    keepPreviewOpen();
    if (previewOpen || openTimer !== null) return;
    openTimer = setTimeout(() => {
      openTimer = null;
      const selection = anchor?.ownerDocument.getSelection();
      if (disposed || (selection && !selection.isCollapsed)) return;
      previewOpen = true;
      void loadPreview();
    }, PREVIEW_OPEN_DELAY_MS);
  }

  function scheduleClose(): void {
    if (openTimer !== null) clearTimeout(openTimer);
    openTimer = null;
    keepPreviewOpen();
    closeTimer = setTimeout(closePreview, PREVIEW_CLOSE_DELAY_MS);
  }

  /** Resolve missing ownership on demand and use the normal Notes navigation path. */
  async function navigate(): Promise<void> {
    if (!isValidReference || navigating) return;
    const requestRevision = revision;
    navigating = true;
    hasNavigationError = false;
    closePreview();
    try {
      let url = destination;
      if (!url) {
        const target = await resolveDatabase();
        url = `#notes?page=${target.page_id}&block=${target.block_id}`;
      }
      if (disposed || requestRevision !== revision) return;
      await openNotesTextLink(url);
    } catch (error: unknown) {
      if (disposed || requestRevision !== revision) return;
      hasNavigationError = true;
      previewOpen = true;
      console.warn("Notes reference navigation failed", { referenceId, error });
    } finally {
      if (!disposed && requestRevision === revision) navigating = false;
    }
  }

  function handleClick(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    const moved = pointerStart !== null
      && Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y) >= POINTER_DRAG_DISTANCE_PX;
    pointerStart = null;
    const selection = anchor?.ownerDocument.getSelection();
    if (moved || (selection && !selection.isCollapsed && !event.ctrlKey && !event.metaKey)) return;
    void navigate();
  }

  /** Track mouse and pen drags while allowing touch taps recognized by the browser. */
  function handlePointerDown(event: PointerEvent): void {
    pointerStart = event.pointerType === "touch" ? null : { x: event.clientX, y: event.clientY };
    closePreview();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      void navigate();
    } else if (event.key === "Escape" && (previewOpen || openTimer !== null)) {
      event.preventDefault();
      event.stopPropagation();
      closePreview();
    }
  }

  /** Handle native parent clicks and focused editor keys before they can open a second action. */
  function referenceEvents(node: HTMLAnchorElement) {
    const owner = node.ownerDocument;
    const capture = (event: KeyboardEvent): void => {
      if (owner.activeElement === node && event.target instanceof Node && node.contains(event.target)) handleKeydown(event);
    };
    const focus = (): void => owner.addEventListener("keydown", capture, true);
    const blur = (): void => owner.removeEventListener("keydown", capture, true);
    node.addEventListener("click", handleClick);
    node.addEventListener("keydown", handleKeydown);
    node.addEventListener("focus", focus);
    node.addEventListener("blur", blur);
    return { destroy() {
      blur();
      node.removeEventListener("click", handleClick);
      node.removeEventListener("keydown", handleKeydown);
      node.removeEventListener("focus", focus);
      node.removeEventListener("blur", blur);
    } };
  }

  function reposition(): void {
    if (!anchor || !card || disposed) return;
    const rect = anchor.getBoundingClientRect();
    const viewport = window.visualViewport;
    const offsetLeft = viewport?.offsetLeft ?? 0;
    const offsetTop = viewport?.offsetTop ?? 0;
    const viewportWidth = viewport?.width ?? window.innerWidth;
    const viewportHeight = viewport?.height ?? window.innerHeight;
    const position = notesFloatingPanelPlacement({
      left: rect.left - offsetLeft, right: rect.right - offsetLeft,
      top: Math.min(viewportHeight, Math.max(0, rect.top - offsetTop)),
      bottom: Math.min(viewportHeight, Math.max(0, rect.bottom - offsetTop)),
    }, { width: viewportWidth, height: viewportHeight }, {
      width: PREVIEW_WIDTH_PX, height: notesFloatingPanelContentHeight({
        scrollHeight: (cardBody ?? card).scrollHeight, clientHeight: card.clientHeight, offsetHeight: card.offsetHeight,
      }), align: "start",
    });
    placement = { ...position, left: position.left + offsetLeft, top: position.top + offsetTop };
  }

  /** Keep the preview outside editable content and release all observers when it closes. */
  function floatingCard(node: HTMLDivElement) {
    const floatingRoot = anchor?.closest<HTMLElement>("[data-floating-root]");
    const portaled = portal(node, floatingRoot && !floatingRoot.closest("[contenteditable='true']") ? floatingRoot : node.ownerDocument.body);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(reposition);
    observer?.observe(node);
    document.addEventListener("scroll", reposition, true);
    window.addEventListener("resize", reposition);
    window.visualViewport?.addEventListener("resize", reposition);
    window.visualViewport?.addEventListener("scroll", reposition);
    reposition();
    return { destroy() {
      observer?.disconnect();
      document.removeEventListener("scroll", reposition, true);
      window.removeEventListener("resize", reposition);
      window.visualViewport?.removeEventListener("resize", reposition);
      window.visualViewport?.removeEventListener("scroll", reposition);
      portaled.destroy();
    } };
  }

  $effect(() => {
    if (!previewOpen) return;
    void previewStatus;
    void previewTitle;
    void hasNavigationError;
    void tick().then(reposition);
  });
</script><a
  {...attributes}
  bind:this={anchor}
  use:referenceEvents
  {href}
  class={`notes-local-reference notes-mention-chip notes-mention-chip-link ${attributes.class ?? ""}`}
  data-notes-mention-chip="true"
  data-notes-reference-id={referenceId}
  data-notes-reference-type={referenceType}
  aria-disabled={!isValidReference ? "true" : undefined}
  aria-busy={navigating ? "true" : undefined}
  aria-describedby={previewOpen ? previewId : undefined}
  onpointerenter={schedulePreview}
  onpointerleave={scheduleClose}
  onpointerdown={handlePointerDown}
  onpointercancel={() => { pointerStart = null; closePreview(); }}
  onpointermove={(event) => { if (event.buttons) closePreview(); }}
>{#if showIcon}<span class="notes-mention-chip-icon" data-notes-editor-sentinel="decoration" aria-hidden="true">{#if referenceType === "database"}<Database size="1em" strokeWidth={1.75} />{:else}<FileText size="1em" strokeWidth={1.75} />{/if}</span>{/if}{text}</a>{#if previewOpen}
  <div
    bind:this={card}
    use:floatingCard
    use:dismissOnOutside={{ onDismiss: (reason, event) => {
      if (reason === "escape") { event.preventDefault(); event.stopPropagation(); }
      closePreview();
    } }}
    id={previewId}
    role="tooltip"
    contenteditable="false"
    data-notes-reference-preview
    class="notes-reference-preview surface-floating fixed z-60 flex flex-col overflow-hidden"
    style:left={`${placement.left}px`}
    style:top={`${placement.top}px`}
    style:width={`${placement.width}px`}
    style:max-height={`${placement.maxHeight}px`}
    onpointerenter={keepPreviewOpen}
    onpointerleave={scheduleClose}
  >
    <div bind:this={cardBody} class="min-h-0 flex-1 overflow-auto" use:scrollEdgeFadeAction>
      <div class="flex items-start gap-3 p-3.5">
        <span class="flex size-9 shrink-0 items-center justify-center rounded-floating-item border border-border/60 bg-muted/40 text-muted-foreground" aria-hidden="true">
          {#if referenceType === "database"}<Database size={20} strokeWidth={1.5} />{:else}<FileText size={20} strokeWidth={1.5} />{/if}
        </span>
        <div class="min-w-0 flex-1">
          <div class="text-[0.85em] text-muted-foreground">{typeLabel}</div>
          <div class="wrap-anywhere leading-snug font-medium">{previewTitle}</div>
          {#if containingPath}<div class="mt-1 wrap-anywhere text-[0.85em] leading-snug text-muted-foreground">{containingPath}</div>{/if}
        </div>
      </div>
      {#if hasNavigationError}
        <div class="px-3.5 pb-3.5 text-[0.9em] text-muted-foreground" role="status">{t("notes.linkOpenFailed")}</div>
      {:else if previewStatus === "failed"}
        <div class="px-3.5 pb-3.5 text-[0.9em] text-muted-foreground" role="status">{t("notes.databaseGalleryPreviewUnavailable")}</div>
      {:else if previewStatus === "loading"}
        <div class="px-3.5 pb-3.5 text-[0.9em] text-muted-foreground" role="status">{t("common.loading")}</div>
      {/if}
      {#if referenceType === "database"}
        <div class="notes-reference-database-thumbnail mx-3.5 mb-3.5 overflow-hidden rounded-md border border-border/60 bg-muted/20" aria-hidden="true">
          <div class="notes-reference-thumbnail-row notes-reference-thumbnail-header">{#each THUMBNAIL_COLUMNS as column}<span class="notes-reference-thumbnail-cell"><i class:notes-reference-thumbnail-short={column > 0}></i></span>{/each}</div>
          {#each THUMBNAIL_ROWS as row}<div class="notes-reference-thumbnail-row">{#each THUMBNAIL_COLUMNS as column}<span class="notes-reference-thumbnail-cell"><i class:notes-reference-thumbnail-short={(row + column) % 2 === 1}></i></span>{/each}</div>{/each}
        </div>
      {/if}
    </div>
  </div>
{/if}<style>
  .notes-reference-preview { font-size: calc(0.8rem * var(--font-scale, 1)); line-height: 1.5; }
  .notes-reference-thumbnail-row { display: grid; grid-template-columns: 1.6fr 1fr 1fr; }
  .notes-reference-thumbnail-row + .notes-reference-thumbnail-row { border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent); }
  .notes-reference-thumbnail-header { background: color-mix(in srgb, var(--muted) 65%, transparent); }
  .notes-reference-thumbnail-cell { display: flex; align-items: center; height: 1.45em; padding-inline: 0.65em; }
  .notes-reference-thumbnail-cell + .notes-reference-thumbnail-cell { border-inline-start: 1px solid color-mix(in srgb, var(--border) 60%, transparent); }
  .notes-reference-thumbnail-cell i { width: 75%; height: 0.22em; border-radius: 1em; background: color-mix(in srgb, var(--muted-foreground) 16%, transparent); }
  .notes-reference-thumbnail-cell i.notes-reference-thumbnail-short { width: 45%; }
</style>
