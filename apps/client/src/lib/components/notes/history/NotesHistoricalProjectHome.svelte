<script lang="ts">
  import Search from "@lucide/svelte/icons/search";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { buildNotesPageTree } from "$lib/notes/pages/tree";
  import type {
    NotesHistoricalPageSummary,
    NotesPage,
    NotesParent,
  } from "$lib/notes/types";
  import NotesPageRow from "$lib/components/notes/navigation/NotesPageRow.svelte";

  let {
    pages,
    selectedPageId,
    onOpenPage,
  }: {
    pages: NotesHistoricalPageSummary[];
    selectedPageId: string | null;
    onOpenPage: (pageId: string) => void;
  } = $props();

  const { t } = getLocalization();
  let search = $state("");
  let collapsedPageIds = $state<string[]>([]);

  const activeSummaries = $derived(
    pages.filter((page) => !page.inTrash && !page.archived && !page.parentDataSourceId),
  );
  const summaryById = $derived(new Map(activeSummaries.map((page) => [page.id, page])));
  const activePages = $derived(activeSummaries.map(historicalPageForTree));
  const pageIdsWithChildren = $derived(
    [...new Set(activeSummaries.flatMap((page) => page.parentPageId ? [page.parentPageId] : []))],
  );
  const treeItems = $derived.by(() =>
    buildNotesPageTree(activePages, {
      activePageId: selectedPageId,
      collapsedPageIds,
      pageIdsWithChildren,
      query: search,
      titleForPage: (page) => summaryById.get(page.id)?.title ?? t("notes.untitled"),
    })
  );

  function historicalPageForTree(page: NotesHistoricalPageSummary): NotesPage {
    return {
      object: "page",
      id: page.id,
      created_time: "",
      last_edited_time: "",
      parent: historicalParent(page),
      folder_id: null,
      in_trash: page.inTrash,
      archived: page.archived,
      icon: page.icon,
      cover: null,
      properties: {},
      url: null,
      public_url: null,
      source_provider: null,
      source_object_id: null,
      source_workspace_id: null,
      source_last_edited_time: null,
    };
  }

  function historicalParent(page: NotesHistoricalPageSummary): NotesParent {
    if (page.parentPageId) return { type: "page_id", page_id: page.parentPageId };
    if (page.parentDataSourceId) {
      return { type: "data_source_id", data_source_id: page.parentDataSourceId };
    }
    return { type: "workspace", workspace: true };
  }

  function setCollapsed(pageId: string, collapsed: boolean): void {
    const next = new Set(collapsedPageIds);
    if (collapsed) next.add(pageId);
    else next.delete(pageId);
    collapsedPageIds = [...next];
  }

  function noop(): void {}
</script>

<div class="flex h-full min-h-0 flex-col overflow-auto px-4 py-4">
  <div class="mx-auto flex w-full max-w-208 shrink-0 flex-wrap items-center gap-2">
    <label class="field flex min-w-64 flex-1 items-center gap-1.5">
      <Search class="size-4 shrink-0 text-muted-foreground" />
      <input
        class="field-bare text-[0.866667rem] text-foreground"
        bind:value={search}
        placeholder={t("notes.searchPlaceholder")}
        aria-label={t("notes.searchLabel")}
      />
    </label>
  </div>

  {#if treeItems.length === 0}
    <div class="mx-auto flex min-h-0 w-full max-w-208 flex-1 items-center justify-center text-center text-[0.866667rem] text-muted-foreground">
      {search.trim() ? t("notes.noSearchResults") : t("notes.noPages")}
    </div>
  {:else}
    <div class="mx-auto mt-4 flex w-full max-w-208 min-w-0 flex-col">
      {#each treeItems as item (item.page.id)}
        {@const summary = summaryById.get(item.page.id)}
        <NotesPageRow
          page={item.page}
          depth={item.depth}
          hasChildren={item.hasChildren}
          collapsed={item.collapsed}
          parentStatus={item.parentStatus}
          favorited={false}
          selected={item.page.id === selectedPageId}
          displayTitle={summary?.title}
          readOnly
          onSelect={() => { onOpenPage(item.page.id); }}
          onRename={noop}
          onToggleCollapsed={(collapsed) => { setCollapsed(item.page.id, collapsed); }}
          onToggleFavorite={noop}
          onCreateChild={noop}
          onDuplicate={noop}
          moveTargets={[]}
          onMove={noop}
          onArchive={noop}
          onTrash={noop}
        />
      {/each}
    </div>
  {/if}
</div>
