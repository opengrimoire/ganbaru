<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Folder from "@lucide/svelte/icons/folder";
  import { buildChangedFileTree, type ChatChangedFileTreeNode } from "$lib/chat/workspace/inspector-model";
  import type { ChatChangedFileRead } from "$lib/chat/contracts";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatFileIcon from "$lib/components/chat/workspace/ChatFileIcon.svelte";

  let {
    files,
    selectedFile,
    onSelect,
  }: {
    files: ChatChangedFileRead[];
    selectedFile: string | null;
    onSelect: (file: ChatChangedFileRead) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const INITIAL_VISIBLE_FILES = 400;
  const FILE_BATCH_SIZE = 400;
  let collapsed = $state<string[]>([]);
  let visibleFileLimit = $state(INITIAL_VISIBLE_FILES);
  const visibleFiles = $derived.by(() => {
    const initial = files.slice(0, visibleFileLimit);
    const selected = selectedFile
      ? files.find((file) => file.relativePath === selectedFile)
      : null;
    return selected && !initial.some((file) => file.relativePath === selected.relativePath)
      ? [...initial, selected]
      : initial;
  });
  const hiddenFileCount = $derived(Math.max(0, files.length - visibleFileLimit));
  const tree = $derived(buildChangedFileTree(visibleFiles));

  function toggle(path: string): void {
    collapsed = collapsed.includes(path)
      ? collapsed.filter((entry) => entry !== path)
      : [...collapsed, path];
  }

  function statusLabel(file: ChatChangedFileRead): string {
    return file.status.slice(0, 1).toUpperCase();
  }

  function sourceLabel(file: ChatChangedFileRead): string {
    return [
      file.providerReported ? t("chat.inspector.providerSource") : null,
      file.gitObserved ? t("chat.inspector.gitSource") : null,
    ].filter((entry): entry is string => entry !== null).join(", ");
  }
</script>

{#snippet nodes(entries: ChatChangedFileTreeNode[], depth: number)}
  {#each entries as node (node.kind + node.relativePath)}
    {#if node.kind === "directory"}
      <button type="button" class="tree-row" style={`padding-left:${depth * 0.5 + 0.25}rem`} onclick={() => toggle(node.relativePath)}>
        {#if collapsed.includes(node.relativePath)}<ChevronRight size={12} />{:else}<ChevronDown size={12} />{/if}
        <Folder size={12} /><span class="truncate">{node.name}</span>
      </button>
      {#if !collapsed.includes(node.relativePath)}{@render nodes(node.children, depth + 1)}{/if}
    {:else if node.file}
      <button type="button" class="tree-row" class:selected={selectedFile === node.relativePath} style={`padding-left:${depth * 0.5 + 1.5}rem`} onclick={() => node.file && onSelect(node.file)} title={`${node.relativePath} · ${sourceLabel(node.file)}`}>
        <ChatFileIcon path={node.relativePath} size={13} />
        <span class="min-w-0 flex-1 truncate text-left">{node.name}</span>
        {#if node.file.previousRelativePath}<span class="max-w-20 truncate text-[0.583333rem] text-muted-foreground" title={node.file.previousRelativePath}>← {node.file.previousRelativePath}</span>{/if}
        {#if node.file.binary}<span class="text-[0.583333rem]">B</span>{/if}
        {#if node.file.additions !== null}<span class="text-[0.583333rem] text-action-confirm">+{formatNumber(localization.locale, node.file.additions)}</span>{/if}
        {#if node.file.deletions !== null}<span class="text-[0.583333rem] text-destructive">−{formatNumber(localization.locale, node.file.deletions)}</span>{/if}
        <span class="file-status">{statusLabel(node.file)}</span>
      </button>
    {/if}
  {/each}
{/snippet}

{@render nodes(tree, 0)}
{#if hiddenFileCount > 0}
  <button type="button" class="load-more-files" onclick={() => { visibleFileLimit += FILE_BATCH_SIZE; }}>
    {t("chat.review.showMoreFiles", hiddenFileCount)}
  </button>
{/if}

<style>
  .tree-row { display: flex; width: 100%; min-height: 1.75rem; align-items: center; gap: 0.25rem; border-radius: 0.25rem; padding-block: 0.25rem; padding-right: 0.3rem; font-size: calc(0.75rem * var(--type-scale)); }
  .tree-row:hover, .tree-row.selected { background: var(--accent); }
  .file-status { display: inline-grid; min-width: 1rem; height: 1rem; place-items: center; border-radius: 0.25rem; background: var(--muted); color: var(--muted-foreground); font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.533333rem * var(--type-scale)); font-weight: 600; }
  .load-more-files { position: sticky; bottom: 0; width: 100%; min-height: 1.9rem; border-top: 1px solid var(--border); background: var(--background); color: var(--primary); font-size: calc(0.7rem * var(--type-scale)); }
  .load-more-files:hover { background: var(--accent); }
</style>
