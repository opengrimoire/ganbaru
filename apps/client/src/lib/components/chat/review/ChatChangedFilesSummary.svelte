<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileDiff from "@lucide/svelte/icons/file-diff";
  import type {
    ChangedFileSummary,
    ChatThreadId,
    ChatTurnId,
    ProjectWorkingFolderId,
  } from "$lib/chat/contracts";
  import { formatNumber } from "$lib/i18n/formatters";
  import { onDestroy } from "svelte";
  import { loadChatReviewDiffRuntime } from "$lib/chat/review/diff-loader";
  import { prefetchChatReview } from "$lib/chat/review/prefetch";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import ChatFileIcon from "$lib/components/chat/workspace/ChatFileIcon.svelte";

  let {
    turnId,
    files,
    sourceThreadId,
    sourceWorkingFolderId,
  }: {
    turnId: ChatTurnId;
    files: ChangedFileSummary[];
    sourceThreadId: ChatThreadId | null;
    sourceWorkingFolderId: ProjectWorkingFolderId | null;
  } = $props();
  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const additions = $derived(files.reduce((total, file) => total + (file.additions ?? 0), 0));
  const deletions = $derived(files.reduce((total, file) => total + (file.deletions ?? 0), 0));
  let prefetchTimer: number | null = null;

  onDestroy(() => cancelPrefetch());

  function openChanges(relativePath: string | null): void {
    if (!sourceThreadId || !sourceWorkingFolderId) return;
    window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-open-review", {
      detail: {
        source: { kind: "provider_turn", turnId },
        relativePath,
        sourceThreadId,
        sourceWorkingFolderId,
      },
    }));
  }

  function prefetchChanges(relativePath: string | null): void {
    cancelPrefetch();
    prefetchTimer = window.setTimeout(() => {
      prefetchTimer = null;
      runPrefetch(relativePath);
    }, 100);
  }

  function runPrefetch(relativePath: string | null): void {
    const threadId = sourceThreadId;
    const workingFolderId = sourceWorkingFolderId;
    if (!threadId || !workingFolderId
      || threadId !== chat.selectedThreadId
      || workingFolderId !== chat.selectedWorkingFolderId) return;
    void loadChatReviewDiffRuntime().catch(() => undefined);
    prefetchChatReview({
      threadId,
      workingFolderId,
      executionEnvironmentId: chat.selectedExecutionEnvironmentId,
      source: { kind: "provider_turn", turnId },
      ignoreWhitespace: false,
      contextLines: 3,
      preferredRelativePath: relativePath,
    });
  }

  function cancelPrefetch(): void {
    if (prefetchTimer === null) return;
    window.clearTimeout(prefetchTimer);
    prefetchTimer = null;
  }
</script>

<section class="changed-files-card">
  <button type="button" class="changed-files-header" onpointerenter={() => prefetchChanges(files[0]?.relativePath ?? null)} onpointerleave={cancelPrefetch} onfocus={() => prefetchChanges(files[0]?.relativePath ?? null)} onblur={cancelPrefetch} onclick={() => openChanges(files[0]?.relativePath ?? null)}>
    <FileDiff size={14} />
    <strong>{t("chat.timeline.changedFiles", formatNumber(localization.locale, files.length))}</strong>
    {#if additions > 0}<span class="additions">+{formatNumber(localization.locale, additions)}</span>{/if}
    {#if deletions > 0}<span class="deletions">−{formatNumber(localization.locale, deletions)}</span>{/if}
    <span class="view-diff">{t("chat.timeline.viewDiff")}<ChevronRight size={13} /></span>
  </button>
  <div class="changed-files-list">
    {#each files as file (file.relativePath)}
      <button type="button" onpointerenter={() => prefetchChanges(file.relativePath)} onpointerleave={cancelPrefetch} onfocus={() => prefetchChanges(file.relativePath)} onblur={cancelPrefetch} onclick={() => openChanges(file.relativePath)}>
        <ChatFileIcon path={file.relativePath} size={13} />
        <span title={file.relativePath}>{file.relativePath}</span>
        {#if file.additions}<small class="additions">+{formatNumber(localization.locale, file.additions)}</small>{/if}
        {#if file.deletions}<small class="deletions">−{formatNumber(localization.locale, file.deletions)}</small>{/if}
      </button>
    {/each}
  </div>
</section>

<style>
  .changed-files-card { overflow:hidden; margin-top:var(--chat-conversation-block-space,0.65rem); border:1px solid var(--border); border-radius:0.8rem; background:color-mix(in srgb,var(--muted) 24%,var(--background)); }
  .changed-files-header { display:flex; width:100%; min-width:0; align-items:center; gap:0.45rem; padding:0.55rem 0.65rem; text-align:left; }
  .changed-files-header:hover { background: color-mix(in srgb, var(--accent) 45%, transparent); }
  .changed-files-header strong { font-size: calc(0.78rem * var(--type-scale)); font-weight: 600; }
  .additions { color: var(--action-confirm); }
  .deletions { color: var(--destructive); }
  .view-diff { display: inline-flex; margin-left: auto; align-items: center; gap: 0.15rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  .changed-files-list { display:grid; max-height:12rem; overflow:auto; border-top:1px solid var(--border); padding:0.25rem; }
  .changed-files-list button { display:flex; min-width:0; align-items:center; gap:0.4rem; border-radius:0.45rem; padding:0.3rem 0.4rem; text-align:left; }
  .changed-files-list button:hover { background: var(--accent); }
  .changed-files-list button > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.72rem * var(--type-scale)); }
  .changed-files-list small { font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.66rem * var(--type-scale)); }
</style>
