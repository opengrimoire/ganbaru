<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Database from "@lucide/svelte/icons/database";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import type { NotesHistoricalPage } from "$lib/notes/types";
  import { notesNumberedListOrdinals } from "$lib/notes/blocks/editor-ui";
  import NotesHistoricalBlock from "./NotesHistoricalBlock.svelte";
  import NotesPageCover from "$lib/components/notes/pages/NotesPageCover.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";

  let {
    page,
    onBack,
    showBack = true,
  }: {
    page: NotesHistoricalPage;
    onBack: () => void;
    showBack?: boolean;
  } = $props();

  const { t } = getLocalization();

  const listOrdinals = $derived(notesNumberedListOrdinals(page.blocks.map((block, index) => ({
    id: String(block.id ?? index),
    type: typeof block.type === "string" ? block.type : "paragraph",
    parentId: parentBlockId(block),
    indent: blockIndent(block),
  }))));

  function blockIndent(block: Record<string, unknown>): number {
    const payload = block.payload ?? (typeof block.type === "string" ? block[block.type] : null);
    if (!payload || typeof payload !== "object" || !("ganbaru_indent" in payload)) return 0;
    const indent = payload.ganbaru_indent;
    return typeof indent === "number" && Number.isSafeInteger(indent) && indent >= 0 ? indent : 0;
  }

  function blockDepth(block: Record<string, unknown>): number {
    const blocksById = new Map(
      page.blocks
        .filter((item) => typeof item.id === "string")
        .map((item) => [item.id as string, item]),
    );
    let parentId = parentBlockId(block);
    let depth = blockIndent(block);
    const visited = new Set<string>();
    while (parentId && !visited.has(parentId)) {
      visited.add(parentId);
      const parent = blocksById.get(parentId);
      if (!parent) break;
      depth += 1 + blockIndent(parent);
      parentId = parentBlockId(parent);
    }
    return depth;
  }

  function parentBlockId(block: Record<string, unknown>): string | null {
    if (typeof block.parent_block_id === "string") return block.parent_block_id;
    if (typeof block.parent !== "object" || block.parent === null || Array.isArray(block.parent)) {
      return null;
    }
    const parent = block.parent as Record<string, unknown>;
    return parent.type === "block_id" && typeof parent.block_id === "string"
      ? parent.block_id
      : null;
  }
</script>

<section class="flex h-full min-h-0 flex-col overflow-hidden">
  {#if showBack}
    <div class="flex shrink-0 items-center px-3" style="height: var(--cal-header-row-h);">
      <button
        type="button"
        class="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground"
        onclick={onBack}
      >
        <ArrowLeft class="size-3.5" />
        <span>{t("notes.projectHistoryBackHome")}</span>
      </button>
    </div>
  {/if}

  <div class="min-h-0 flex-1 overflow-auto" style="container-type: inline-size;">
    {#if page.cover}
      <div class="notes-page-banner overflow-hidden bg-muted">
        <NotesPageCover cover={page.cover} unavailableLabel={t("notes.pageCoverUnavailable")} />
      </div>
    {/if}

    <article class="mx-auto flex w-full max-w-208 flex-col px-4 pb-12 pt-8 sm:px-8">
      {#if page.icon}
        <div class="mb-3 flex size-16 items-center justify-center rounded-md text-foreground">
          <NotesPageIcon
            icon={page.icon}
            size={48}
            emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
            class="shrink-0"
          />
        </div>
      {/if}

      <h1 class="mb-5 wrap-break-word text-[2.5rem] font-bold leading-[1.2] text-foreground">
        {page.title || t("notes.untitled")}
      </h1>

      {#if page.blocks.length === 0}
        <p class="text-[0.866667rem] text-muted-foreground">{t("notes.projectHistoryEmptyPage")}</p>
      {:else}
        <div class="flex flex-col gap-2">
          {#each page.blocks as block, index (`${String(block.id ?? index)}`)}
            <NotesHistoricalBlock {block} depth={blockDepth(block)} listOrdinal={listOrdinals.get(String(block.id ?? index))} />
          {/each}
        </div>
      {/if}

      {#if page.databases.length > 0}
        <div class="mt-8 rounded-lg border border-border p-4">
          <div class="flex items-center gap-2 text-[0.866667rem] font-medium text-foreground">
            <Database class="size-4" />
            <span>{t("notes.projectHistoryDatabaseCount", page.databases.length)}</span>
          </div>
        </div>
      {/if}
    </article>
  </div>
</section>
