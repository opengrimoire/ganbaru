<script lang="ts">
  import { onMount } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import type { ProjectWorkingFolderFileEntry } from "$lib/chat/contracts";
  import type { ChatFileTreeRow } from "$lib/chat/workspace/file-tree-model";
  import { chatVirtualRange } from "$lib/chat/workspace/file-tree-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatFileIcon from "./ChatFileIcon.svelte";

  let {
    rows,
    selectedPath,
    changedPaths,
    loadingPaths,
    refreshing = false,
    onToggle,
    onSelect,
  }: {
    rows: readonly ChatFileTreeRow[];
    selectedPath: string | null;
    changedPaths: ReadonlySet<string>;
    loadingPaths: readonly string[];
    refreshing?: boolean;
    onToggle: (entry: ProjectWorkingFolderFileEntry) => void;
    onSelect: (entry: ProjectWorkingFolderFileEntry) => void;
  } = $props();

  const { t } = getLocalization();
  const ROW_HEIGHT = 26;
  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(360);
  let scrollFrame: number | null = null;
  const range = $derived(chatVirtualRange(rows.length, scrollTop, viewportHeight, ROW_HEIGHT, 10));
  const visibleRows = $derived(rows.slice(range.start, range.end));

  onMount(() => {
    const observer = new ResizeObserver(([entry]) => {
      if (entry) viewportHeight = entry.contentRect.height;
    });
    if (scroller) observer.observe(scroller);
    return () => {
      observer.disconnect();
      if (scrollFrame !== null) window.cancelAnimationFrame(scrollFrame);
    };
  });

  function handleScroll(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLDivElement) || scrollFrame !== null) return;
    scrollFrame = window.requestAnimationFrame(() => {
      scrollTop = target.scrollTop;
      scrollFrame = null;
    });
  }
</script>

<div bind:this={scroller} class="file-tree-scroll" role="tree" aria-busy={refreshing || undefined} onscroll={handleScroll}>
  <div class="file-tree-canvas" style={`height:${range.totalSize}px;`}>
    <div class="file-tree-window" style={`transform:translateY(${range.offset}px);`}>
      {#each visibleRows as row (row.entry.relativePath)}
        {@const entry = row.entry}
        {@const loading = loadingPaths.includes(entry.relativePath)}
        <div
          role="treeitem"
          aria-level={row.depth + 1}
          aria-expanded={entry.kind === "directory" ? row.expanded : undefined}
          aria-selected={entry.kind === "file" ? selectedPath === entry.relativePath : undefined}
          class="file-tree-row"
          class:selected={selectedPath === entry.relativePath}
          class:ignored={entry.ignored}
          style={`padding-left:${row.depth * 0.8 + 0.2}rem;`}
        >
          {#if entry.kind === "directory"}
            <button type="button" class="tree-disclosure" aria-label={row.expanded ? t("chat.inspector.collapseFolder", entry.displayName) : t("chat.inspector.expandFolder", entry.displayName)} onclick={() => onToggle(entry)}>
              {#if loading}<LoaderCircle size={12} class="animate-spin" />{:else if row.expanded}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
            </button>
            <button type="button" class="tree-label" onclick={() => onToggle(entry)}>
              {#if row.expanded}<FolderOpen size={14} class="folder-icon" />{:else}<Folder size={14} class="folder-icon" />{/if}
              <span class="truncate">{entry.displayName}</span>
            </button>
          {:else}
            <span class="tree-disclosure" aria-hidden="true"></span>
            <button type="button" class="tree-label" onclick={() => onSelect(entry)}>
              <ChatFileIcon path={entry.relativePath} size={14} />
              <span class="truncate">{entry.displayName}</span>
              {#if changedPaths.has(entry.relativePath)}<span class="changed-dot" aria-label={t("chat.inspector.changed")}></span>{/if}
            </button>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .file-tree-scroll { min-width: 0; min-height: 0; flex: 1; overflow: auto; overscroll-behavior: contain; contain: strict; }
  .file-tree-canvas { position: relative; min-width: max-content; }
  .file-tree-window { position: absolute; top: 0; min-width: 100%; }
  .file-tree-row { display: flex; height: 26px; min-width: 100%; align-items: center; padding-right: 0.25rem; color: var(--foreground); contain: layout paint style; }
  .file-tree-row:hover { background: color-mix(in srgb, var(--accent) 62%, transparent); }
  .file-tree-row.selected { background: var(--accent); }
  .file-tree-row.ignored { opacity: 0.58; }
  .tree-disclosure { display: inline-grid; width: 1.4rem; height: 1.4rem; flex: 0 0 auto; place-items: center; border-radius: 0.25rem; color: var(--muted-foreground); }
  button.tree-disclosure:hover { background: color-mix(in srgb, var(--background) 72%, transparent); color: var(--foreground); }
  .tree-label { display: flex; min-width: 0; height: 100%; flex: 1; align-items: center; gap: 0.35rem; padding-right: 0.35rem; text-align: left; font-size: calc(0.733333rem * var(--type-scale)); }
  .tree-label :global(.folder-icon) { flex: 0 0 auto; color: color-mix(in srgb, var(--muted-foreground) 72%, var(--status-tentative)); }
  .changed-dot { width: 0.38rem; height: 0.38rem; flex: 0 0 auto; border-radius: 9999px; background: var(--primary); }
</style>
