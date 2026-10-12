<script lang="ts">
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import type { NotesBacklink } from "$lib/notes/types";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Link2 from "@lucide/svelte/icons/link-2";

  const notes = getNotesEditor();
  const { t } = getLocalization();
  let { embedded = false }: { embedded?: boolean } = $props();
  let open = $state(false);
  const panelOpen = $derived(embedded || open);

  /** Loads the backlinks again, rejecting while they are still unavailable. */
  async function retryBacklinks(): Promise<void> {
    await notes.reloadBacklinks();
    if (notes.backlinksError) throw new Error(notes.backlinksError);
  }

  function backlinkTypeLabel(backlink: NotesBacklink): string {
    switch (backlink.reference_type) {
      case "child_page":
        return t("notes.backlinkTypeChildPage");
      case "page_mention":
        return t("notes.backlinkTypePageMention");
      case "link":
        return t("notes.backlinkTypeLink");
      case "database_relation":
        return t("notes.backlinkTypeDatabaseRelation");
      case "comment_mention":
        return t("notes.backlinkTypeCommentMention");
      case "comment_link":
        return t("notes.backlinkTypeCommentLink");
    }
  }

  function openBacklink(backlink: NotesBacklink): void {
    void notes.openNotesLink({
      pageId: backlink.source_page.id,
      blockId:
        backlink.source_block_type === "database_relation" ||
        (backlink.source_block_type === "comment" &&
          backlink.source_block_id === backlink.source_page.id)
          ? undefined
          : backlink.source_block_id,
    });
  }
</script>

<div class={embedded ? "min-w-0" : "mt-2"}>
  {#if !embedded}
    <button
      type="button"
      class="inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-expanded={open}
      onclick={() => {
        open = !open;
      }}
    >
      <Link2 class="size-3.5 shrink-0" />
      <span class="min-w-0 truncate">
        {#if notes.backlinksLoading}
          {t("notes.loadingBacklinks")}
        {:else}
          {t("notes.backlinksCount", notes.backlinks.length)}
        {/if}
      </span>
      <ChevronDown class={`size-3.5 shrink-0 transition-transform ${open ? "rotate-180" : ""}`} />
    </button>
  {/if}

  {#if panelOpen}
    <div class={embedded ? "min-w-0 rounded-md bg-background/70 p-1" : "mt-1 max-w-2xl rounded-md border border-border bg-background/70 p-1"}>
      {#if notes.backlinksError}
        <Notice class="px-2 py-1.5 text-[0.8rem]" message={t("notes.loadBacklinksFailed", notes.backlinksError)} copy={notes.backlinksError} onRetry={retryBacklinks} />
      {:else if notes.backlinks.length === 0}
        <div class="px-2 py-1.5 text-[0.8rem] text-muted-foreground">
          {t("notes.noBacklinks")}
        </div>
      {:else}
        <div class="flex flex-col">
          {#each notes.backlinks as backlink (backlink.id)}
            {@const sourceTitle = notesPageTitle(backlink.source_page, t("notes.untitled"))}
            <button
              type="button"
              class="min-w-0 rounded px-2 py-1.5 text-left hover:bg-accent"
              aria-label={t("notes.openBacklinkSource", sourceTitle)}
              onclick={() => {
                openBacklink(backlink);
              }}
            >
              <div class="flex min-w-0 items-center gap-2">
                <span class="min-w-0 flex-1 truncate text-[0.866667rem] font-medium text-foreground">
                  {sourceTitle}
                </span>
                <span class="shrink-0 text-[0.7rem] text-muted-foreground">
                  {backlinkTypeLabel(backlink)}
                </span>
              </div>
              <div class="mt-0.5 truncate text-[0.733333rem] text-muted-foreground">
                {backlink.snippet}
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
