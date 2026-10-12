<script module lang="ts">
  import type { NotesDatabaseMentionRichText, NotesPageMentionRichText, NotesRichText } from "$lib/notes/types";

  /** Narrow rich text to the local references that support navigation and metadata previews. */
  export function isNotesLocalReference(item: NotesRichText): item is NotesPageMentionRichText | NotesDatabaseMentionRichText {
    return item.type === "mention" && (item.mention.type === "page" || item.mention.type === "database");
  }
</script>

<script lang="ts">
  import { onDestroy } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { isNotesUuid } from "$lib/notes/links/block-link";
  import { notesLinkTarget, openNotesTextLink } from "$lib/notes/links/navigation";
  import { notesLabeledRoute, notesPageLocationHeading } from "$lib/notes/rich-text/mention-preview";
  import type { NotesDatabaseReference } from "$lib/notes/types";
  import Database from "@lucide/svelte/icons/database";
  import FileText from "@lucide/svelte/icons/file-text";
  import { NotesMentionPreviewHover } from "./mention-preview-hover.svelte";
  import NotesMentionPreviewCard from "./NotesMentionPreviewCard.svelte";

  interface ReferenceAttributes {
    class?: string;
    style?: string;
    [name: `data-${string}`]: string | undefined;
  }

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
  let previewStatus = $state<"idle" | "loading" | "ready" | "failed">("idle");
  let location = $state<readonly string[]>([]);
  let database = $state<NotesDatabaseReference | null>(null);
  let hasNavigationError = $state(false);
  let navigating = $state(false);
  let databaseRequest: Promise<NotesDatabaseReference> | null = null;
  let pointerStart: { x: number; y: number } | null = null;
  let revision = 0;
  let disposed = false;
  const preview = new NotesMentionPreviewHover({
    anchor: () => anchor,
    canOpen: () => isValidReference,
    onOpen: () => void loadPreview(),
  });

  const providedTarget = $derived(reference.href
    ? notesLinkTarget(reference.href, typeof window === "undefined" ? undefined : window.location.href) : null);
  const destination = $derived(!isValidReference ? null : referenceType === "page"
    ? `#notes?page=${referenceId}`
    : database ? `#notes?page=${database.page_id}&block=${database.block_id}`
      : providedTarget?.blockId === referenceId ? `#notes?page=${providedTarget.pageId}&block=${referenceId}` : null);
  const href = $derived(destination ?? (isValidReference ? `#notes?block=${referenceId}` : undefined));
  const fallbackTitle = $derived(reference.plain_text.trim() || t("notes.untitled"));
  /** A page's location ends with its own title; a database's location ends with the page that holds it. */
  const heading = $derived(referenceType === "database"
    ? { route: notesLabeledRoute(location, t("notes.untitled")), title: database?.title.trim() || fallbackTitle }
    : notesPageLocationHeading(location, t("notes.untitled")));
  const previewStatusText = $derived(hasNavigationError ? t("notes.linkOpenFailed")
    : previewStatus === "failed" ? t("notes.databaseGalleryPreviewUnavailable") : null);

  $effect(() => {
    void referenceKey;
    revision += 1;
    preview.close();
    databaseRequest = null;
    database = null;
    location = [];
    previewStatus = "idle";
    hasNavigationError = false;
    navigating = false;
    pointerStart = null;
    return () => { revision += 1; preview.close(); };
  });

  onDestroy(() => { disposed = true; revision += 1; preview.dispose(); });

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

  /** Read titles and location only, leaving database rows and page content unloaded. */
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
      const segments = await api.getNotesPageLocation(ownerId);
      if (disposed || requestRevision !== revision) return;
      location = segments;
      previewStatus = "ready";
    } catch (error: unknown) {
      if (disposed || requestRevision !== revision) return;
      previewStatus = "failed";
      console.warn("Notes reference preview failed", { referenceId: pageId, error });
    }
  }

  /** Resolve missing ownership on demand and use the normal Notes navigation path. */
  async function navigate(): Promise<void> {
    if (!isValidReference || navigating) return;
    const requestRevision = revision;
    navigating = true;
    hasNavigationError = false;
    preview.close();
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
      preview.show();
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
    preview.close();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      void navigate();
    } else if (event.key === "Escape" && preview.pending) {
      event.preventDefault();
      event.stopPropagation();
      preview.close();
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
</script>{#snippet referenceIcon(size: number | string, strokeWidth: number)}{#if referenceType === "database"}<Database {size} {strokeWidth} />{:else}<FileText {size} {strokeWidth} />{/if}{/snippet}{#snippet previewIcon()}{@render referenceIcon(20, 1.5)}{/snippet}<a
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
  aria-describedby={preview.open ? previewId : undefined}
  onpointerenter={(event) => preview.schedule(event)}
  onpointerleave={() => preview.scheduleClose()}
  onpointerdown={handlePointerDown}
  onpointercancel={() => { pointerStart = null; preview.close(); }}
  onpointermove={(event) => { if (event.buttons) preview.close(); }}
>{#if showIcon}<span class="notes-mention-chip-icon" data-notes-editor-sentinel="decoration" aria-hidden="true">{@render referenceIcon("1em", 1.75)}</span>{/if}{text}</a>{#if preview.open && anchor}<NotesMentionPreviewCard
  {anchor}
  id={previewId}
  route={heading.route}
  title={heading.title ?? fallbackTitle}
  status={previewStatusText}
  loading={previewStatus === "loading"}
  icon={previewIcon}
  onKeepOpen={() => preview.keepOpen()}
  onLeave={() => preview.scheduleClose()}
  onDismiss={() => preview.close()}
>{#if referenceType === "database"}
    <div class="notes-reference-database-thumbnail mx-3.5 mb-3.5 overflow-hidden rounded-md border border-border/60 bg-muted/20" aria-hidden="true">
      <div class="notes-reference-thumbnail-row notes-reference-thumbnail-header">{#each THUMBNAIL_COLUMNS as column}<span class="notes-reference-thumbnail-cell"><i class:notes-reference-thumbnail-short={column > 0}></i></span>{/each}</div>
      {#each THUMBNAIL_ROWS as row}<div class="notes-reference-thumbnail-row">{#each THUMBNAIL_COLUMNS as column}<span class="notes-reference-thumbnail-cell"><i class:notes-reference-thumbnail-short={(row + column) % 2 === 1}></i></span>{/each}</div>{/each}
    </div>
  {/if}</NotesMentionPreviewCard>{/if}<style>
  .notes-reference-thumbnail-row { display: grid; grid-template-columns: 1.6fr 1fr 1fr; }
  .notes-reference-thumbnail-row + .notes-reference-thumbnail-row { border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent); }
  .notes-reference-thumbnail-header { background: color-mix(in srgb, var(--muted) 65%, transparent); }
  .notes-reference-thumbnail-cell { display: flex; align-items: center; height: 1.45em; padding-inline: 0.65em; }
  .notes-reference-thumbnail-cell + .notes-reference-thumbnail-cell { border-inline-start: 1px solid color-mix(in srgb, var(--border) 60%, transparent); }
  .notes-reference-thumbnail-cell i { width: 75%; height: 0.22em; border-radius: 1em; background: color-mix(in srgb, var(--muted-foreground) 16%, transparent); }
  .notes-reference-thumbnail-cell i.notes-reference-thumbnail-short { width: 45%; }
</style>
