<script lang="ts">
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { filterNotesPagesByTitle } from "$lib/notes/pages/selection";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import type { NotesPage } from "$lib/notes/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Search from "@lucide/svelte/icons/search";

  const notes = getNotes();
  const localization = getLocalization();
  const { t } = localization;
  let search = $state("");
  let searchInitialized = false;
  let restoringPageId = $state<string | null>(null);
  const filteredPages = $derived.by(() =>
    filterNotesPagesByTitle(notes.archivedPages, search, (page) =>
      notesPageTitle(page, t("notes.untitled"))
    )
  );

  $effect(() => {
    const query = search;
    if (!searchInitialized) {
      searchInitialized = true;
      return;
    }
    const timeout = window.setTimeout(() => void notes.reloadArchivedPages(query), 150);
    return () => window.clearTimeout(timeout);
  });

  function editedLabel(page: NotesPage): string {
    return t("notes.metadataEdited", new Date(page.last_edited_time).toLocaleString(localization.locale));
  }

  async function unarchivePage(page: NotesPage): Promise<void> {
    restoringPageId = page.id;
    try {
      await notes.unarchivePage(page.id);
    } finally {
      restoringPageId = null;
    }
  }
</script>

<section class="flex min-w-0 flex-1 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center gap-2 border-b border-border px-4 py-3 sm:px-6">
    <button
      class="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-label={t("notes.backToPages")}
      onclick={() => {
        notes.closeArchive();
      }}
    >
      <ArrowLeft class="size-4" />
    </button>
    <div class="flex min-w-0 flex-1 items-center gap-2">
      <Archive class="size-4 shrink-0 text-muted-foreground" />
      <h2 class="min-w-0 truncate text-[1.05rem] font-semibold text-foreground">
        {t("notes.archive")}
      </h2>
    </div>
  </div>

  <div class="shrink-0 px-4 py-3 sm:px-6">
    <label class="field flex max-w-xl items-center gap-1.5">
      <Search class="size-4 shrink-0 text-muted-foreground" />
      <input
        class="field-bare text-[0.866667rem] text-foreground"
        bind:value={search}
        placeholder={t("notes.searchArchivePlaceholder")}
        aria-label={t("notes.searchArchiveLabel")}
      />
    </label>
  </div>

  <div class="min-h-0 flex-1 overflow-auto px-4 pb-5 sm:px-6">
    {#if notes.archiveLoading && notes.archivedPages.length === 0}
      <Loading layout="rows" rows={3} label={t("notes.loadingArchive")} class="py-2 text-[0.866667rem]" />
    {:else if notes.archiveError}
      <div class="h-full min-h-32 text-[0.866667rem]">
        <Notice layout="fill" message={t("notes.loadArchiveFailed", notes.archiveError)} copy={notes.archiveError} onRetry={() => notes.reloadArchivedPages(search)} />
      </div>
    {:else if filteredPages.length === 0}
      <div class="py-2 text-[0.866667rem] text-muted-foreground">
        {search.trim() ? t("notes.noArchiveSearchResults") : t("notes.emptyArchive")}
      </div>
    {:else}
      <div class="flex max-w-3xl flex-col gap-1">
        {#each filteredPages as page (page.id)}
          <div class="flex min-w-0 flex-wrap items-start gap-3 rounded-md px-2 py-2 hover:bg-accent/70">
            <div class="min-w-32 flex-1">
              <div class="truncate text-[0.933333rem] font-medium text-foreground">
                {notesPageTitle(page, t("notes.untitled"))}
              </div>
              <div class="mt-0.5 truncate text-[0.733333rem] text-muted-foreground">
                {editedLabel(page)}
              </div>
            </div>
            <button
              class="flex shrink-0 items-center gap-1.5 rounded-md border border-border bg-background px-2 py-1.5 text-[0.8rem] text-foreground hover:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
              disabled={restoringPageId === page.id}
              aria-label={t("notes.unarchivePage", notesPageTitle(page, t("notes.untitled")))}
              onclick={() => {
                void unarchivePage(page);
              }}
            >
              <ArchiveRestore class="size-4" />
              <span>{t("notes.unarchive")}</span>
            </button>
          </div>
        {/each}
      </div>
      {#if notes.archiveHasMore}
        <button
          type="button"
          class="mt-2 rounded-md border border-border px-3 py-1.5 text-[0.8rem] text-foreground hover:bg-accent disabled:opacity-60"
          disabled={notes.archiveLoading}
          onclick={() => void notes.loadMoreArchivedPages()}
        >
          {t("common.loadMore")}
        </button>
      {/if}
    {/if}
  </div>
</section>
