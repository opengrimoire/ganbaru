<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Info from "@lucide/svelte/icons/info";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Youtube from "@lucide/svelte/icons/youtube";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { groupMusicIssues, type MusicIssueGroup } from "$lib/music/library/issue-presentation";
  import type { MusicIssue, MusicItemListEntry, MusicSourceSummary } from "$lib/music/library/contracts";
  import { cn } from "$lib/utils";

  let {
    issues,
    items,
    sources,
    selectedGroup,
    activeItemId,
    refreshing = false,
    onSelectGroup,
    onBack,
    onSelectIssue,
    onRepair,
    canRepairIssue = () => true,
    onRefresh,
  }: {
    issues: MusicIssue[];
    items: MusicItemListEntry[];
    sources: MusicSourceSummary[];
    selectedGroup: MusicIssueGroup | null;
    activeItemId: string | null;
    refreshing?: boolean;
    onSelectGroup: (group: MusicIssueGroup | null) => void;
    onBack: () => void;
    onSelectIssue: (issue: MusicIssue) => void;
    onRepair: (issue: MusicIssue) => void;
    canRepairIssue?: (issue: MusicIssue) => boolean;
    onRefresh: () => void;
  } = $props();

  const { t } = getLocalization();
  const groups = $derived(groupMusicIssues(issues));
  const visibleIssues = $derived(selectedGroup ? groups.get(selectedGroup) ?? [] : []);
  const itemTitles = $derived(new Map(items.map((item) => [item.id, item.title])));
  const sourceNames = $derived(new Map(sources.map((source) => [source.id, source.name])));

  function groupLabel(group: MusicIssueGroup): string {
    if (group === "missing-local-file") return t("music.builder.missingFiles");
    if (group === "root-unavailable") return t("music.builder.unavailableFolders");
    if (group === "ambiguous-match") return t("music.builder.ambiguousMatches");
    if (group === "youtube-unavailable") return t("music.builder.unavailableVideos");
    if (group === "embedding-blocked") return t("music.builder.blockedVideos");
    return t("music.builder.incompleteRefreshes");
  }

  function issueTitle(issue: MusicIssue): string {
    if (issue.itemId) {
      const title = itemTitles.get(issue.itemId);
      if (title) return title;
    }
    if (issue.collectionId) {
      const sourceName = sourceNames.get(issue.collectionId);
      if (sourceName) return sourceName;
    }
    return groupLabel(selectedGroup ?? "missing-local-file");
  }
</script>

<section class="flex min-h-0 flex-1 flex-col" aria-label={t("music.builder.sourceIssues")}>
  <header class="flex h-12 shrink-0 items-center gap-1.5 px-2">
    <button
      type="button"
      onclick={() => selectedGroup ? onSelectGroup(null) : onBack()}
      class="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-label={selectedGroup ? t("music.builder.backToIssueGroups") : t("music.builder.backToReviewFolders")}
      title={selectedGroup ? t("music.builder.backToIssueGroups") : t("music.builder.backToReviewFolders")}
    ><ArrowLeft size={14} /></button>
    <strong class="min-w-0 flex-1 truncate text-[0.7rem] font-semibold">{selectedGroup ? groupLabel(selectedGroup) : t("music.builder.sourceIssues")}</strong>
    <span class="shrink-0 text-[0.6rem] tabular-nums text-muted-foreground">{selectedGroup ? visibleIssues.length : issues.length}</span>
    <button type="button" onclick={onRefresh} disabled={refreshing} class="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40" aria-label={refreshing ? t("music.builder.refreshing") : t("music.builder.refresh")} title={refreshing ? t("music.builder.refreshing") : t("music.builder.refresh")}><RefreshCw class={cn(refreshing && "animate-spin motion-reduce:animate-none")} size={12} /></button>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2" data-music-scrollable="true">
    {#if selectedGroup}
      <div class="space-y-1">
        {#each visibleIssues as issue (issue.id)}
          {@const title = issueTitle(issue)}
          <article class={cn("rounded-lg px-2 py-2", issue.itemId === activeItemId ? "bg-primary/10" : "hover:bg-accent/45")}>
            <div class="flex min-w-0 items-start gap-2">
              <span class={cn("mt-0.5 shrink-0", issue.actionRequired ? "text-destructive" : "text-muted-foreground")}>{#if issue.actionRequired}<TriangleAlert size={13} />{:else}<Info size={13} />{/if}</span>
              {#if issue.itemId}
                <button type="button" onclick={() => onSelectIssue(issue)} class="min-w-0 flex-1 rounded-sm text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">
                  <strong class="block truncate text-[0.68rem] font-medium">{title}</strong>
                  <span class="mt-0.5 block text-[0.61rem] leading-relaxed text-muted-foreground">{issue.message}</span>
                </button>
              {:else}
                <span class="min-w-0 flex-1">
                  <strong class="block truncate text-[0.68rem] font-medium">{title}</strong>
                  <span class="mt-0.5 block text-[0.61rem] leading-relaxed text-muted-foreground">{issue.message}</span>
                </span>
              {/if}
              {#if issue.actionRequired && canRepairIssue(issue)}<button type="button" onclick={() => onRepair(issue)} class="h-7 shrink-0 rounded-md px-1.5 text-[0.62rem] font-semibold text-primary hover:bg-primary/10">{t("music.builder.repair")}</button>{/if}
            </div>
          </article>
        {/each}
      </div>
    {:else}
      <div class="space-y-0.5">
        {#each [...groups.entries()] as [group, entries] (group)}
          <button type="button" onclick={() => onSelectGroup(group)} class="flex min-h-10 w-full items-center gap-2 rounded-lg px-2 text-left hover:bg-accent/55">
            <span class="grid h-7 w-7 shrink-0 place-items-center text-muted-foreground">{#if group === "youtube-unavailable" || group === "embedding-blocked"}<Youtube size={14} />{:else if group === "refresh-incomplete"}<RefreshCw size={13} />{:else}<FolderSearch size={14} />{/if}</span>
            <span class="min-w-0 flex-1 truncate text-[0.68rem] font-medium">{groupLabel(group)}</span>
            <span class="text-[0.6rem] tabular-nums text-muted-foreground">{entries.length}</span>
            <ChevronRight size={12} class="shrink-0 text-muted-foreground" />
          </button>
        {/each}
      </div>
    {/if}
  </div>
</section>
