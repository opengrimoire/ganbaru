<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Check from "@lucide/svelte/icons/check";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Search from "@lucide/svelte/icons/search";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import X from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicItemListEntry } from "$lib/music/library/contracts";
  import { createMusicReviewTreeViewState, type MusicReviewTreeViewState } from "$lib/music/builder/view-state";
  import {
    buildMusicReviewTree,
    flattenMusicReviewTree,
    musicReviewTreeAncestorFolderIds,
    musicReviewTreeFolderIds,
    musicReviewTreeRevealScrollTop,
    searchMusicReviewTree,
    type MusicReviewTreeNode,
  } from "$lib/music/review/tree";
  import { cn } from "$lib/utils";

  let {
    items,
    totalCount,
    activeItemId,
    onActivate,
    issueCount = 0,
    issueItemIds = new Set<string>(),
    onOpenIssues = () => undefined,
    selectionClearRequest = 0,
    selectionDisabled = false,
    canRefresh = true,
    refreshing = false,
    onRefresh = () => undefined,
    showIgnored = false,
    onShowIgnoredChange = () => undefined,
    viewState = createMusicReviewTreeViewState(),
    onViewStateChange = () => undefined,
  }: {
    items: MusicItemListEntry[];
    totalCount: number;
    activeItemId: string | null;
    onActivate: (itemId: string) => void;
    issueCount?: number;
    issueItemIds?: ReadonlySet<string>;
    onOpenIssues?: () => void;
    selectionClearRequest?: number;
    selectionDisabled?: boolean;
    canRefresh?: boolean;
    refreshing?: boolean;
    onRefresh?: () => void;
    showIgnored?: boolean;
    onShowIgnoredChange?: (showIgnored: boolean) => void;
    viewState?: MusicReviewTreeViewState;
    onViewStateChange?: (state: MusicReviewTreeViewState) => void;
  } = $props();

  const { t } = getLocalization();
  let explicitlyCollapsedIds = $state<Set<string>>(new Set(untrack(() => viewState.collapsedFolderIds)));
  let lastExpandedActiveItemId = $state<string | null>(null);
  let selectedIds = $state<Set<string>>(new Set(untrack(() => viewState.selectedItemIds)));
  let selectedFolderIds = $state<Set<string>>(new Set(untrack(() => viewState.selectedFolderIds)));
  let handledSelectionClearRequest = $state(untrack(() => selectionClearRequest));
  let search = $state(untrack(() => viewState.search));
  let scrollTop = $state(untrack(() => viewState.scrollTop));
  let scrollNode = $state<HTMLElement | null>(null);
  let revealGeneration = 0;
  const tree = $derived(buildMusicReviewTree(items));
  const expandedIds = $derived(new Set([...musicReviewTreeFolderIds(tree)]
    .filter((folderId) => !explicitlyCollapsedIds.has(folderId))));
  const searching = $derived(search.trim().length > 0);
  const searchResult = $derived(searchMusicReviewTree(tree, search));
  const rows = $derived(searching ? searchResult.rows : flattenMusicReviewTree(tree, expandedIds));
  const folderSelectionReady = $derived(items.length >= totalCount);

  $effect(() => {
    const validIds = new Set(items.map((item) => item.id));
    const retained = [...selectedIds].filter((itemId) => validIds.has(itemId));
    if (retained.length !== selectedIds.size) selectedIds = new Set(retained);
  });

  $effect(() => {
    const validFolderIds = musicReviewTreeFolderIds(tree);
    const retainedSelected = [...selectedFolderIds].filter((folderId) => validFolderIds.has(folderId));
    if (retainedSelected.length !== selectedFolderIds.size) selectedFolderIds = new Set(retainedSelected);
    const retainedCollapsed = new Set([...explicitlyCollapsedIds]
      .filter((folderId) => validFolderIds.has(folderId)));
    if (retainedCollapsed.size !== explicitlyCollapsedIds.size) {
      explicitlyCollapsedIds = retainedCollapsed;
    }
  });

  $effect(() => {
    if (selectionClearRequest === handledSelectionClearRequest) return;
    handledSelectionClearRequest = selectionClearRequest;
    selectedIds = new Set();
    selectedFolderIds = new Set();
  });

  $effect(() => {
    const itemId = activeItemId;
    if (!itemId || itemId === lastExpandedActiveItemId) return;
    lastExpandedActiveItemId = itemId;
    const ancestorIds = musicReviewTreeAncestorFolderIds(tree, itemId);
    if (ancestorIds.length > 0) {
      const nextCollapsed = new Set(explicitlyCollapsedIds);
      for (const folderId of ancestorIds) {
        nextCollapsed.delete(folderId);
      }
      explicitlyCollapsedIds = nextCollapsed;
    }
    void revealActiveItem(itemId);
  });

  async function revealActiveItem(itemId: string): Promise<void> {
    const generation = ++revealGeneration;
    await tick();
    if (generation !== revealGeneration || !scrollNode || activeItemId !== itemId) return;
    const row = [...scrollNode.querySelectorAll<HTMLElement>("[data-review-item-id]")]
      .find((candidate) => candidate.dataset.reviewItemId === itemId);
    if (!row) return;
    const viewport = scrollNode.getBoundingClientRect();
    const rowBounds = row.getBoundingClientRect();
    const target = musicReviewTreeRevealScrollTop(
      scrollNode.scrollTop,
      scrollNode.clientHeight,
      rowBounds.top - viewport.top,
      rowBounds.height,
    );
    if (target === null) return;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    scrollNode.scrollTo({ top: target, behavior: reducedMotion ? "auto" : "smooth" });
  }

  function toggleExpanded(nodeId: string): void {
    if (searching) return;
    const nextCollapsed = new Set(explicitlyCollapsedIds);
    if (expandedIds.has(nodeId)) nextCollapsed.add(nodeId);
    else nextCollapsed.delete(nodeId);
    explicitlyCollapsedIds = nextCollapsed;
  }

  function toggleFolder(node: MusicReviewTreeNode): void {
    const folderIds = musicReviewTreeFolderIds([node]);
    const nextItems = new Set(selectedIds);
    const nextFolders = new Set(selectedFolderIds);
    const selecting = !nextFolders.has(node.id);
    for (const itemId of node.itemIds) {
      if (selecting) nextItems.add(itemId);
      else nextItems.delete(itemId);
    }
    for (const folderId of folderIds) {
      if (selecting) nextFolders.add(folderId);
      else nextFolders.delete(folderId);
    }
    selectedIds = nextItems;
    selectedFolderIds = nextFolders;
  }

  function toggleItem(itemId: string): void {
    const next = new Set(selectedIds);
    if (next.has(itemId)) {
      next.delete(itemId);
      const nextFolders = new Set(selectedFolderIds);
      const removeContainingFolders = (nodes: readonly MusicReviewTreeNode[]): void => {
        for (const node of nodes) {
          if (node.itemIds.includes(itemId)) nextFolders.delete(node.id);
          removeContainingFolders(node.children);
        }
      };
      removeContainingFolders(tree);
      selectedFolderIds = nextFolders;
    } else next.add(itemId);
    selectedIds = next;
  }

  function handleSearchKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || !searching) return;
    event.preventDefault();
    search = "";
  }

  $effect(() => {
    if (scrollNode && Math.abs(scrollNode.scrollTop - scrollTop) > 1) scrollNode.scrollTop = scrollTop;
  });

  $effect(() => {
    onViewStateChange({
      search,
      collapsedFolderIds: [...explicitlyCollapsedIds],
      selectedItemIds: [...selectedIds],
      selectedFolderIds: [...selectedFolderIds],
      scrollTop,
    });
  });
</script>

<section class="review-tree flex min-h-0 flex-1 flex-col" aria-label={t("music.builder.reviewFolders")}>
  <div class="shrink-0 p-2">
    <div class="field flex h-8 items-center gap-2 px-2.5">
      <Search size={13} class="shrink-0 text-muted-foreground" />
      <input bind:value={search} onkeydown={handleSearchKeydown} type="text" inputmode="search" enterkeyhint="search" autocomplete="off" aria-label={t("music.builder.searchReviewTree")} placeholder={t("music.builder.searchReviewTree")} class="field-bare text-[0.7rem]" />
      {#if searching}
        <span class="shrink-0 text-[0.6rem] tabular-nums text-muted-foreground" aria-live="polite" aria-label={t("music.builder.reviewSearchMatchCount", searchResult.matchCount)} title={t("music.builder.reviewSearchMatchCount", searchResult.matchCount)}>{searchResult.matchCount}</span>
        <button type="button" onclick={() => search = ""} class="grid h-6 w-6 shrink-0 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground" aria-label={t("music.builder.clearReviewSearch")}><X size={12} /></button>
      {/if}
      <button type="button" onclick={() => onShowIgnoredChange(!showIgnored)} class={cn("grid h-6 w-6 shrink-0 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground", showIgnored && "bg-background/60 text-foreground")} aria-label={showIgnored ? t("music.builder.hideIgnored") : t("music.builder.showIgnored")} title={showIgnored ? t("music.builder.hideIgnored") : t("music.builder.showIgnored")} aria-pressed={showIgnored}>{#if showIgnored}<EyeOff size={12} />{:else}<Eye size={12} />{/if}</button>
      <button type="button" onclick={onRefresh} disabled={!canRefresh || refreshing} class="grid h-6 w-6 shrink-0 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground disabled:opacity-40" aria-label={refreshing ? t("music.builder.refreshing") : t("music.builder.refreshLocalFolders")} title={refreshing ? t("music.builder.refreshing") : t("music.builder.refreshLocalFolders")}><RefreshCw class={cn(refreshing && "animate-spin motion-reduce:animate-none")} size={12} /></button>
    </div>
  </div>

  {#if issueCount > 0}
    <button type="button" onclick={onOpenIssues} class="mx-2 mb-1 flex h-7 shrink-0 items-center gap-2 rounded-lg px-2 text-left hover:bg-accent/55" aria-label={t("music.builder.reviewIssuesCount", issueCount)}>
      <TriangleAlert size={12} class="shrink-0 text-destructive" />
      <span class="min-w-0 flex-1 truncate text-[0.66rem] font-medium">{t("music.builder.sourceIssues")}</span>
      <span class="text-[0.6rem] tabular-nums text-destructive">{issueCount}</span>
      <ChevronRight size={11} class="shrink-0 text-muted-foreground" />
    </button>
  {/if}

  <div bind:this={scrollNode} onscroll={(event) => scrollTop = event.currentTarget.scrollTop} class="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2" data-music-scrollable="true">
    {#if searching && rows.length === 0}
      <p class="px-3 py-6 text-center text-[0.68rem] text-muted-foreground">{t("music.builder.noReviewSearchMatches")}</p>
    {/if}
    {#each rows as row (row.kind === "folder" ? row.node.id : row.item.id)}
      {#if row.kind === "folder"}
        {@const checked = selectedFolderIds.has(row.node.id)}
        <div class={cn("group relative flex h-8 min-w-0 items-center rounded-lg", checked ? "bg-primary/10" : "hover:bg-accent/60")} style={`padding-left: ${row.depth * 0.75}rem`}>
          <button type="button" onclick={() => toggleExpanded(row.node.id)} disabled={searching} class="absolute inset-0 rounded-lg focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring disabled:cursor-default" aria-label={(searching || expandedIds.has(row.node.id)) ? t("music.builder.collapseFolder", row.node.name) : t("music.builder.expandFolder", row.node.name)} aria-expanded={searching || expandedIds.has(row.node.id)}></button>
          <span class="pointer-events-none grid h-8 w-7 shrink-0 place-items-center text-muted-foreground">
            <ChevronRight size={14} class={cn("transition-transform motion-reduce:transition-none", (searching || expandedIds.has(row.node.id)) && "rotate-90")} />
          </span>
          <label class="relative z-10 grid h-full w-7 shrink-0 cursor-pointer place-items-center">
            <Checkbox {checked} disabled={!folderSelectionReady || selectionDisabled} onChange={() => toggleFolder(row.node)} label={t("music.builder.selectFolder", row.node.name, row.node.itemIds.length)} />
          </label>
          <span class="pointer-events-none flex min-w-0 flex-1 items-center pr-2 text-left">
            <span class="min-w-0 flex-1 truncate text-[0.7rem] font-medium">{row.node.name}</span>
            <span class="text-[0.6rem] tabular-nums text-muted-foreground">{row.node.itemIds.length}</span>
          </span>
        </div>
      {:else}
        <div data-review-item-id={row.item.id} class={cn("relative flex h-8 min-w-0 items-center rounded-lg", selectedIds.has(row.item.id) || activeItemId === row.item.id ? "bg-primary/10 text-foreground" : "hover:bg-accent/50")} style={`padding-left: ${row.depth * 0.75 + 1.75}rem`}>
          <button type="button" onclick={() => onActivate(row.item.id)} class="absolute inset-0 rounded-lg focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring" aria-label={row.item.reviewState === "reviewed" ? `${row.item.title}, ${t("music.builder.markReviewed")}` : row.item.reviewState === "ignored" ? `${row.item.title}, ${t("music.builder.ignored")}` : row.item.title} aria-current={activeItemId === row.item.id ? "true" : undefined}></button>
          <label class="relative z-10 grid h-full w-7 shrink-0 cursor-pointer place-items-center">
            <Checkbox checked={selectedIds.has(row.item.id)} disabled={selectionDisabled} onChange={() => toggleItem(row.item.id)} label={t("music.builder.selectTrack", row.item.title)} />
          </label>
          <span class="pointer-events-none flex min-w-0 flex-1 items-center pr-2 text-left">
            <span class="min-w-0 flex-1 truncate text-[0.68rem]">{row.item.title}</span>
            {#if row.item.reviewState === "ignored"}
              <span class="ml-2 grid h-5 w-5 shrink-0 place-items-center text-muted-foreground" data-review-state="ignored" aria-hidden="true"><Eye size={12} /></span>
            {:else if issueItemIds.has(row.item.id)}
              <span class="ml-2 grid h-5 w-5 shrink-0 place-items-center text-destructive" aria-label={t("music.builder.trackNeedsAttention", row.item.title)}><TriangleAlert size={12} /></span>
            {:else if row.item.reviewState === "reviewed"}
              <span class="ml-2 grid h-5 w-5 shrink-0 place-items-center text-primary" aria-hidden="true"><Check size={12} strokeWidth={2.5} /></span>
            {/if}
          </span>
        </div>
      {/if}
    {/each}
  </div>

</section>

<style>
  .review-tree {
    grid-column: 1;
    min-height: 0;
  }

  @container (width < 620px) {
    .review-tree {
      min-height: 12rem;
      flex: 0 0 42%;
      border-bottom: 1px solid color-mix(in srgb, var(--border) 46%, transparent);
    }
  }
</style>
