<script lang="ts">
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { filterNotesPagesByTitle } from "$lib/notes/pages/selection";
  import { notesPageRestoresToWorkspace } from "$lib/notes/pages/recovery";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import type { NotesPage } from "$lib/notes/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Search from "@lucide/svelte/icons/search";
  import Trash2 from "@lucide/svelte/icons/trash-2";

  const notes = getNotes();
  const localization = getLocalization();
  const { t } = localization;
  let search = $state("");
  let searchInitialized = false;
  let restoringPageId = $state<string | null>(null);
  let deletingPageId = $state<string | null>(null);
  let pendingDeletePage = $state<NotesPage | null>(null);
  const filteredPages = $derived.by(() =>
    filterNotesPagesByTitle(notes.trashedPages, search, (page) =>
      notesPageTitle(page, t("notes.untitled"))
    )
  );

  $effect(() => {
    const query = search;
    if (!searchInitialized) {
      searchInitialized = true;
      return;
    }
    const timeout = window.setTimeout(() => void notes.reloadTrashedPages(query), 150);
    return () => window.clearTimeout(timeout);
  });

  function editedLabel(page: NotesPage): string {
    return t("notes.metadataEdited", new Date(page.last_edited_time).toLocaleString(localization.locale));
  }

  async function restorePage(page: NotesPage): Promise<void> {
    restoringPageId = page.id;
    try {
      await notes.restorePage(page.id);
    } finally {
      restoringPageId = null;
    }
  }

  async function permanentlyDeletePage(page: NotesPage): Promise<void> {
    deletingPageId = page.id;
    try {
      await notes.permanentlyDeletePage(page.id);
    } finally {
      deletingPageId = null;
    }
  }

  function confirmPermanentDelete(): void {
    const page = pendingDeletePage;
    pendingDeletePage = null;
    if (page) void permanentlyDeletePage(page);
  }
</script>

<section class="flex min-w-0 flex-1 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center gap-2 border-b border-border px-4 py-3 sm:px-6">
    <button
      class="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-label={t("notes.backToPages")}
      onclick={() => {
        notes.closeTrash();
      }}
    >
      <ArrowLeft class="size-4" />
    </button>
    <div class="flex min-w-0 flex-1 items-center gap-2">
      <Trash2 class="size-4 shrink-0 text-muted-foreground" />
      <h2 class="min-w-0 truncate text-[1.05rem] font-semibold text-foreground">
        {t("notes.trash")}
      </h2>
    </div>
  </div>

  <div class="shrink-0 px-4 py-3 sm:px-6">
    <label class="field flex max-w-xl items-center gap-1.5">
      <Search class="size-4 shrink-0 text-muted-foreground" />
      <input
        class="field-bare text-[0.866667rem] text-foreground"
        bind:value={search}
        placeholder={t("notes.searchTrashPlaceholder")}
        aria-label={t("notes.searchTrashLabel")}
      />
    </label>
  </div>

  <div class="min-h-0 flex-1 overflow-auto px-4 pb-5 sm:px-6">
    {#if notes.trashLoading && notes.trashedPages.length === 0}
      <Loading layout="rows" rows={3} label={t("notes.loadingTrash")} class="py-2 text-[0.866667rem]" />
    {:else if notes.trashError}
      <div class="h-full min-h-32 text-[0.866667rem]">
        <Notice layout="fill" message={t("notes.loadTrashFailed", notes.trashError)} copy={notes.trashError} onRetry={() => notes.reloadTrashedPages(search)} />
      </div>
    {:else if filteredPages.length === 0}
      <div class="py-2 text-[0.866667rem] text-muted-foreground">
        {search.trim() ? t("notes.noTrashSearchResults") : t("notes.emptyTrash")}
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
              {#if notesPageRestoresToWorkspace(page, notes.trashedPages)}
                <div class="mt-1 text-[0.733333rem] leading-snug text-muted-foreground">
                  {t("notes.restoreMovesToWorkspace")}
                </div>
              {/if}
            </div>
            <div class="flex shrink-0 flex-wrap justify-end gap-2">
              <button
                class="flex items-center gap-1.5 rounded-md border border-border bg-background px-2 py-1.5 text-[0.8rem] text-foreground hover:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
                disabled={restoringPageId === page.id || deletingPageId === page.id}
                aria-label={t("notes.restorePage", notesPageTitle(page, t("notes.untitled")))}
                onclick={() => {
                  void restorePage(page);
                }}
              >
                <RotateCcw class="size-4" />
                <span>{t("common.restore")}</span>
              </button>
              <button
                class="flex items-center gap-1.5 rounded-md border border-destructive/40 bg-background px-2 py-1.5 text-[0.8rem] text-destructive hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-60"
                disabled={restoringPageId === page.id || deletingPageId === page.id}
                aria-label={t("notes.permanentlyDeletePage", notesPageTitle(page, t("notes.untitled")))}
                onclick={() => {
                  pendingDeletePage = page;
                }}
              >
                <Trash2 class="size-4" />
                <span>{t("notes.deleteForever")}</span>
              </button>
            </div>
          </div>
        {/each}
      </div>
      {#if notes.trashHasMore}
        <button
          type="button"
          class="mt-2 rounded-md border border-border px-3 py-1.5 text-[0.8rem] text-foreground hover:bg-accent disabled:opacity-60"
          disabled={notes.trashLoading}
          onclick={() => void notes.loadMoreTrashedPages()}
        >
          {t("common.loadMore")}
        </button>
      {/if}
    {/if}
  </div>
</section>

{#if pendingDeletePage}
  <ConfirmDialog
    title={t("notes.permanentDeleteConfirmTitle", notesPageTitle(pendingDeletePage, t("notes.untitled")))}
    message={t("notes.permanentDeleteConfirmMessage")}
    confirmLabel={t("notes.permanentDeleteConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmPermanentDelete}
    onCancel={() => {
      pendingDeletePage = null;
    }}
  />
{/if}
