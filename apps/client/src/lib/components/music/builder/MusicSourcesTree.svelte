<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSourceBrowserNode } from "$lib/music/sources/browser";
  import { cn } from "$lib/utils";

  let {
    nodes,
    selectedNodeId,
    onSelect,
  }: {
    nodes: MusicSourceBrowserNode[];
    selectedNodeId: string | null;
    onSelect: (nodeId: string | null) => void;
  } = $props();

  interface SourceTreeRow {
    node: MusicSourceBrowserNode;
    depth: number;
  }

  const { t } = getLocalization();
  let search = $state("");
  let collapsedIds = $state<Set<string>>(new Set());
  const query = $derived(search.trim().toLocaleLowerCase());

  function matches(node: MusicSourceBrowserNode): boolean {
    if (!query) return true;
    return node.name.toLocaleLowerCase().includes(query)
      || node.directItems.some((item) =>
        item.title.toLocaleLowerCase().includes(query)
        || item.artist.toLocaleLowerCase().includes(query))
      || node.children.some(matches);
  }

  function flatten(entries: readonly MusicSourceBrowserNode[], depth = 0): SourceTreeRow[] {
    const rows: SourceTreeRow[] = [];
    for (const node of entries) {
      if (!matches(node)) continue;
      rows.push({ node, depth });
      if ((query || !collapsedIds.has(node.id)) && node.children.length > 0) {
        rows.push(...flatten(node.children, depth + 1));
      }
    }
    return rows;
  }

  const rows = $derived(flatten(nodes));

  function toggle(nodeId: string): void {
    const next = new Set(collapsedIds);
    if (next.has(nodeId)) next.delete(nodeId);
    else next.add(nodeId);
    collapsedIds = next;
  }
</script>

<section class="flex min-h-0 flex-1 flex-col" aria-label={t("music.builder.sources")}>
  <div class="shrink-0 p-2">
    <div class="field flex h-8 items-center gap-2 px-2.5">
      <Search size={13} class="shrink-0 text-muted-foreground" />
      <input bind:value={search} type="search" autocomplete="off" aria-label={t("music.builder.searchSources")} placeholder={t("music.builder.searchSources")} class="field-bare text-[0.7rem]" />
      {#if search}<button type="button" onclick={() => search = ""} class="grid h-6 w-6 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground" aria-label={t("music.builder.clearSearch")}><X size={12} /></button>{/if}
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2" data-music-scrollable="true">
    <button type="button" class={cn("source-row", selectedNodeId === null && "active-row")} onclick={() => onSelect(null)}>
      <span class="w-7 shrink-0"></span>
      <span class="min-w-0 flex-1 truncate text-[0.68rem] font-medium">{t("music.builder.allSources")}</span>
      <span class="source-count">{nodes.reduce((total, node) => total + node.itemIds.length, 0)}</span>
    </button>
    {#each rows as row (row.node.id)}
      <div class={cn("source-row group", selectedNodeId === row.node.id && "active-row")} style={`padding-left: ${row.depth * 0.75 + 0.2}rem`}>
        {#if row.node.children.length > 0}
          <button type="button" onclick={() => toggle(row.node.id)} disabled={Boolean(query)} class="relative z-10 grid h-7 w-7 shrink-0 place-items-center rounded-md text-muted-foreground hover:text-foreground disabled:cursor-default" aria-label={(query || !collapsedIds.has(row.node.id)) ? t("music.builder.collapseFolder", row.node.name) : t("music.builder.expandFolder", row.node.name)} aria-expanded={Boolean(query) || !collapsedIds.has(row.node.id)}>
            <ChevronRight size={13} class={cn("transition-transform motion-reduce:transition-none", (query || !collapsedIds.has(row.node.id)) && "rotate-90")} />
          </button>
        {:else}<span class="h-7 w-7 shrink-0"></span>{/if}
        <button type="button" onclick={() => onSelect(row.node.id)} class="absolute inset-0 rounded-lg focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring" aria-current={selectedNodeId === row.node.id ? "page" : undefined}><span class="sr-only">{row.node.name}</span></button>
        <span class="pointer-events-none min-w-0 flex-1 truncate text-[0.68rem]" class:font-medium={row.node.kind !== "folder"}>{row.node.name}</span>
        {#if row.node.issueCount > 0}<span class="pointer-events-none h-1.5 w-1.5 shrink-0 rounded-full bg-destructive" aria-hidden="true"></span>{/if}
        <span class="source-count pointer-events-none">{row.node.itemIds.length}</span>
      </div>
    {/each}
  </div>
</section>

<style>
  .source-row { position: relative; display: flex; height: 2rem; width: 100%; min-width: 0; align-items: center; gap: 0.35rem; border-radius: 0.5rem; padding-right: 0.45rem; color: var(--foreground); text-align: left; }
  .source-row:hover { background: color-mix(in srgb, var(--accent) 48%, transparent); }
  .active-row { background: color-mix(in srgb, var(--primary) 10%, transparent); }
  .source-count { flex: none; color: var(--muted-foreground); font-size: calc(0.58rem * var(--type-scale)); font-variant-numeric: tabular-nums; }
</style>
