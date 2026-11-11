<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesDatabaseViewKind, NotesPage } from "$lib/notes/types";
  import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import { collectionGutterRoom } from "$lib/components/collections/gutter-room";
  import { NOTES_DATABASE_DEFAULT_COLUMN_WIDTH, NOTES_DATABASE_TITLE_COLUMN_WIDTH } from "$lib/notes/database/table";
  import { NOTES_TABLE_ROW_GUTTER, notesTableGutterMargin, notesTableRowTemplate } from "./database/table-layout";

  let {
    kind = "page",
    page = null,
    openMode = "full",
    ready = false,
    children,
  }: {
    /** `simple-table` is a Notes table block, which has no row gutter; `table` and the other view kinds are database views. */
    kind?: "page" | "cover" | "block" | "simple-table" | "database" | NotesDatabaseViewKind;
    page?: NotesPage | null;
    openMode?: NotesPageOpenMode;
    ready?: boolean;
    children?: Snippet;
  } = $props();

  const { t } = getLocalization();
  const PAGE_REVEAL_DELAY_MS = 500;
  const EMBEDDED_REVEAL_DELAY_MS = 120;
  const HANDOFF_DURATION_MS = 100;
  let revealed = $state(false);
  let shouldRetainPlaceholder = $state(untrack(() => !ready));

  $effect(() => {
    if (!ready) {
      shouldRetainPlaceholder = true;
      revealed = false;
      const revealDelayMs = kind === "page" ? PAGE_REVEAL_DELAY_MS : EMBEDDED_REVEAL_DELAY_MS;
      const timer = setTimeout(() => { revealed = true; }, revealDelayMs);
      return () => clearTimeout(timer);
    }
    if (!untrack(() => revealed)) {
      shouldRetainPlaceholder = false;
      return;
    }
    // Retire only the decorative overlay. Ready content is already mounted and interactive.
    const timer = setTimeout(() => {
      shouldRetainPlaceholder = false;
      revealed = false;
    }, HANDOFF_DURATION_MS);
    return () => clearTimeout(timer);
  });

  /** Columns of a fresh database table: the title and two properties at their default widths. */
  const TABLE_COLUMN_WIDTHS = [NOTES_DATABASE_TITLE_COLUMN_WIDTH, NOTES_DATABASE_DEFAULT_COLUMN_WIDTH, NOTES_DATABASE_DEFAULT_COLUMN_WIDTH];
  const TABLE_TEMPLATE = notesTableRowTemplate(TABLE_COLUMN_WIDTHS);
  /** Text bar widths per row and column, as percentages of the cell, varied so rows read as content. */
  const TABLE_HEADER_BARS = [45, 40, 35];
  const TABLE_ROW_BARS = [[60, 45, 30], [75, 30, 50], [50, 55, 40]];
  const LIST_ROW_BARS = [55, 40, 65];

  let root: HTMLDivElement | null = $state(null);
  /** Room left of the skeleton for the table gutter, measured as the loaded table measures it so columns line up. */
  let gutterRoom = $state(0);
  const showsDatabaseTable = $derived(kind === "database" || kind === "table");

  $effect(() => {
    const element = root;
    if (!element || !showsDatabaseTable || !shouldRetainPlaceholder) return;
    const measure = () => { gutterRoom = collectionGutterRoom(element).room; };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    const clip = collectionGutterRoom(element).clip;
    if (clip) observer.observe(clip);
    return () => observer.disconnect();
  });

  /** Placeholder spacing that mirrors the loaded content: the database block margin, or the inline block padding. */
  function placeholderClass(): string {
    if (kind === "page") return "h-full min-h-0 overflow-x-hidden overflow-y-scroll";
    if (kind === "cover") return "size-full";
    if (kind === "database") return "my-5";
    if (kind === "block" || kind === "simple-table") return "overflow-hidden py-2";
    return "overflow-hidden";
  }
</script>

<div
  bind:this={root}
  class="notes-loading-skeleton {children ? 'notes-loading-surface' : ''} {kind === 'page' ? 'min-h-0 min-w-0 flex-1 overflow-hidden' : kind === 'cover' ? 'size-full' : 'w-full min-w-0'}"
  style={`--notes-skeleton-handoff: ${HANDOFF_DURATION_MS}ms;`}
  role={!ready ? "status" : undefined}
  aria-busy={!ready || undefined}
  aria-label={!ready ? t("common.loading") : undefined}
  data-notes-skeleton={!ready ? kind : undefined}
>
  {#if children}
    <div
      class="notes-skeleton-content {kind === 'page' ? 'flex min-h-0 min-w-0' : kind === 'cover' ? 'relative size-full' : 'min-w-0'}"
      data-ready={ready}
      data-handoff={ready && revealed}
      inert={!ready}
    >
      {@render children()}
    </div>
  {/if}
  {#if shouldRetainPlaceholder}
    <div
      class="notes-skeleton-placeholder {placeholderClass()}"
      style:container-type={kind === "page" ? "inline-size" : undefined}
      data-retiring={ready}
      aria-hidden="true"
    >
      <div class="notes-skeleton-shapes {kind === 'cover' ? 'size-full' : ''}" style:opacity={!ready && revealed ? 1 : 0}>
        {#if kind === "database"}
          <!-- Mirrors the database block: title row, view toolbar, then the view section's top padding. -->
          <div class="space-y-1">
            <div class="flex min-h-10 items-center pl-1"><div class="skeleton-fill h-6 w-2/5 max-w-64 rounded-md"></div></div>
            <div class="flex items-center gap-1 py-1">
              <div class="skeleton-fill h-8 w-24 rounded-md"></div>
              <div class="skeleton-fill ml-auto size-8 rounded-md"></div>
              <div class="skeleton-fill h-8 w-20 rounded-md"></div>
            </div>
            <div class="pt-2">{@render databaseTable()}</div>
          </div>
        {:else if kind === "table"}
          {@render databaseTable()}
        {:else if kind === "list"}
          {#each LIST_ROW_BARS as width}
            <CollectionRow template="minmax(0, 1fr)">
              <div class="flex min-w-0 items-center px-2"><div class="skeleton-fill h-3 rounded" style:width="{width}%"></div></div>
            </CollectionRow>
          {/each}
        {:else if kind === "page"}
          {#if page?.cover}<div class="notes-page-banner skeleton-fill" data-notes-skeleton-cover></div>{/if}
          <div class="mx-auto flex w-full max-w-208 flex-col pt-3 {openMode === 'side' ? 'pl-16 pr-4 sm:pr-8' : 'px-4 sm:px-8'}">
            <div class="min-h-8"></div>
            {#if page?.icon}<div class="skeleton-fill mb-2 size-14 rounded-lg"></div>{/if}
            <div class="skeleton-fill mb-6 h-10 w-3/5 rounded-md"></div>
            {@render lines()}
          </div>
        {:else if kind === "cover"}
          <div class="skeleton-fill size-full"></div>
        {:else if kind === "block"}
          {@render lines()}
        {:else if kind === "board" || kind === "gallery"}
          <div class="grid grid-cols-3 gap-3">
            {#each [0, 1, 2] as column}
              <div class="min-w-0 space-y-3">
                {#if kind === "board"}<div class="skeleton-fill h-4 w-2/3 rounded"></div>{/if}
                {#each (kind === "board" ? [0, 1] : [0]) as card}
                  <div class="space-y-3 rounded-md border border-border/50 p-3">
                    {#if kind === "gallery"}<div class="skeleton-fill h-20 rounded"></div>{/if}
                    <div class="skeleton-fill h-3 w-4/5 rounded"></div>
                    <div class="skeleton-fill h-3 w-1/2 rounded"></div>
                  </div>
                {/each}
              </div>
            {/each}
          </div>
        {:else if kind === "calendar"}
          <div class="mb-3 flex items-center justify-between"><div class="skeleton-fill h-5 w-1/3 rounded"></div><div class="skeleton-fill h-5 w-1/5 rounded"></div></div>
          <div class="grid grid-cols-7 overflow-hidden rounded-md border border-border/50">
            {#each Array(21) as cell}
              <div class="min-w-0 border-b border-r border-border/50 p-2"><div class="skeleton-fill h-10 rounded"></div></div>
            {/each}
          </div>
        {:else if kind === "timeline"}
          <div class="divide-y divide-border/50 border-y border-border/50">
            {#each [0, 1, 2, 3] as row}
              <div class="grid min-h-10 grid-cols-3 items-center gap-4 px-2">
                <div class="skeleton-fill h-3 w-4/5 rounded"></div>
                <div class="col-span-2 border-l border-border/50 pl-3"><div class="skeleton-fill h-5 rounded {row % 2 === 0 ? 'ml-auto w-1/2' : 'w-2/3'}"></div></div>
              </div>
            {/each}
          </div>
        {:else}
          <div class="overflow-hidden border-y border-border/50">
            {#each [0, 1, 2, 3] as row}
              <div class="flex min-h-10 items-center gap-6 border-b border-border/50 px-2 last:border-b-0">
                <div class="skeleton-fill h-3 w-1/3 rounded"></div>
                <div class="skeleton-fill h-3 w-1/5 rounded"></div>
                <div class="skeleton-fill h-3 w-1/6 rounded"></div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<!-- Same rows, tracks, gutter hang, and dividers as the loaded database table, so nothing shifts at the handoff. -->
{#snippet databaseTable()}
  <div class="min-w-0 overflow-hidden" style:margin-left={notesTableGutterMargin(gutterRoom)} data-notes-skeleton-table>
    <div class="min-w-max">
      <CollectionRow template={TABLE_TEMPLATE} gutter={NOTES_TABLE_ROW_GUTTER} header dividerTop>
        {@render tableCells(TABLE_HEADER_BARS, true)}
      </CollectionRow>
      {#each TABLE_ROW_BARS as bars}
        <CollectionRow template={TABLE_TEMPLATE} gutter={NOTES_TABLE_ROW_GUTTER}>
          {@render tableCells(bars, false)}
        </CollectionRow>
      {/each}
    </div>
  </div>
{/snippet}

{#snippet tableCells(bars: number[], header: boolean)}
  <div></div><div></div>
  {#each bars as width}
    <div class="flex min-w-0 items-center gap-1.5 px-2">
      {#if header}<div class="skeleton-fill size-3.5 shrink-0 rounded-sm"></div>{/if}
      <div class="skeleton-fill h-3 rounded" style:width="{width}%"></div>
    </div>
  {/each}
  <div></div>
{/snippet}

{#snippet lines()}
  <div class="space-y-3">
    <div class="skeleton-fill h-3 w-4/5 rounded"></div>
    <div class="skeleton-fill h-3 w-2/3 rounded"></div>
    <div class="skeleton-fill h-3 w-1/2 rounded"></div>
  </div>
{/snippet}

<style>
  .skeleton-fill {
    background: color-mix(in srgb, var(--muted-foreground) 12%, transparent);
  }

  .notes-loading-surface {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
  }

  .notes-loading-surface > div {
    grid-area: 1 / 1;
  }

  .notes-skeleton-placeholder {
    pointer-events: none;
  }

  .notes-skeleton-placeholder[data-retiring="true"] {
    position: absolute;
    inset-inline: 0;
    top: 0;
    overflow: hidden;
  }

  .notes-skeleton-shapes {
    transition: opacity var(--notes-skeleton-handoff) ease-out;
  }

  .notes-skeleton-content {
    visibility: hidden;
    opacity: 0;
  }

  .notes-skeleton-content[data-ready="false"] {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }

  .notes-skeleton-content[data-ready="true"] {
    visibility: visible;
    opacity: 1;
  }

  .notes-skeleton-content[data-handoff="true"] {
    transition: opacity var(--notes-skeleton-handoff) ease-out;
  }

  .notes-loading-skeleton:not(.notes-loading-surface) > .notes-skeleton-placeholder {
    height: 100%;
  }

  @media (prefers-reduced-motion: reduce) {
    .notes-skeleton-shapes,
    .notes-skeleton-content[data-handoff="true"] {
      transition: none;
    }
  }
</style>
