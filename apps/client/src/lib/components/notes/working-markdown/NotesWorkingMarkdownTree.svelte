<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesWorkingMarkdownTreeProps } from "./tree-contracts";
  import { buildWorkingMarkdownTreeItems } from "$lib/notes/working-markdown/tree";
  import { cn } from "$lib/utils";

  let {
    tree,
    query,
    selectedFile,
    loading,
    error,
    onSelect,
    onRefresh,
  }: NotesWorkingMarkdownTreeProps = $props();

  const { t } = getLocalization();
  let collapsedKeys = $state<Set<string>>(new Set());
  const rows = $derived(buildWorkingMarkdownTreeItems(tree.roots, collapsedKeys, query));

  function toggle(key: string): void {
    const next = new Set(collapsedKeys);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsedKeys = next;
  }
</script>

{#if tree.roots.length > 0 || tree.unavailableWorkingFolderIds.length > 0 || error}
  <section class="mt-2 border-t border-border/70 pt-2" aria-label={t("notes.workingMarkdown.title")}>
    <div class="explorer-section-title mb-1 flex min-h-8 items-center gap-2 px-1.5 text-muted-foreground">
      <span class="min-w-0 flex-1 truncate">{t("notes.workingMarkdown.title")}</span>
      <button
        type="button"
        class="flex size-6 items-center justify-center rounded hover:bg-accent hover:text-foreground"
        aria-label={t("notes.workingMarkdown.refreshTree")}
        title={t("notes.workingMarkdown.refreshTree")}
        disabled={loading}
        onclick={onRefresh}
      >
        <RefreshCw class={cn("size-3.5", loading && "animate-spin")} />
      </button>
    </div>
    {#if error}
      <div class="px-2 py-1.5 text-[0.72rem] text-destructive" role="alert">{error}</div>
    {/if}
    {#if tree.unavailableWorkingFolderIds.length > 0}
      <div class="px-2 py-1.5 text-[0.72rem] text-warning" role="status">
        {t("notes.workingMarkdown.unavailableFolders", tree.unavailableWorkingFolderIds.length)}
      </div>
    {/if}
    {#each rows as row (row.key)}
      {@const collapsed = collapsedKeys.has(row.key)}
      <button
        type="button"
        class={cn(
          "explorer-row explorer-row-label flex w-full min-w-0 items-center gap-1.5 rounded-md py-1.5 pr-2 text-left",
          row.kind === "file"
            && selectedFile?.workingFolderId === row.workingFolderId
            && selectedFile.relativePath === row.relativePath
            && "explorer-selected text-foreground",
          row.kind !== "file" && "text-muted-foreground",
        )}
        style={`padding-left: ${6 + row.depth * 14}px`}
        title={row.kind === "root" ? `${row.name}: ${row.sourceKind}` : row.relativePath}
        onclick={() => {
          if (row.kind === "file") {
            onSelect({
              workingFolderId: row.workingFolderId,
              workingFolderName: row.workingFolderName,
              relativePath: row.relativePath,
            });
          } else if (row.expandable) {
            toggle(row.key);
          }
        }}
      >
        <span class="flex size-3.5 shrink-0 items-center justify-center">
          {#if row.expandable}
            <ChevronRight class={cn("size-3.5 transition-transform", !collapsed && "rotate-90")} />
          {/if}
        </span>
        {#if row.kind === "root"}
          {#if row.sourceKind === "managed"}
            <HardDrive class="size-3.5 shrink-0" />
          {:else}
            <FolderGit2 class="size-3.5 shrink-0" />
          {/if}
        {:else if row.kind === "directory"}
          <Folder class="size-3.5 shrink-0" />
        {:else}
          <FileText class="size-3.5 shrink-0" />
        {/if}
        <span class="min-w-0 flex-1 truncate">{row.name}</span>
        {#if row.kind === "root" && row.truncated}
          <span class="shrink-0 text-[0.65rem]" title={t("notes.workingMarkdown.truncated")}>…</span>
        {/if}
      </button>
    {/each}
  </section>
{/if}
