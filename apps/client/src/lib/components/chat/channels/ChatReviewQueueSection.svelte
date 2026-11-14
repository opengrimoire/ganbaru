<script lang="ts">
  import { tick } from "svelte";
  import FileDiff from "@lucide/svelte/icons/file-diff";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { requestConfirmation } from "$lib/components/ui/confirmation.svelte";
  import type { ChatReviewQueueItemRead, ChatWorkAssignmentId } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { useChatPaneEvents } from "$lib/chat/pane-events";
  import {
    batchAcceptCandidates,
    reviewQueueCount,
    reviewQueueSections,
    sidebarReviewQueueItems,
    type ChatBatchAcceptSummary,
  } from "$lib/chat/review-queue/model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { useChat } from "$lib/stores/chat.svelte";

  /**
   * The rows of the rail's list of work that waits for the person across every project: answers, approvals, and
   * reviews. The rail owns the section heading. Failures stay out of the list: the channel row and the reply thread
   * mark them. Recording outcomes needs local execution, so other builds and the phone panel list the work without
   * selection or Accept.
   */
  let {
    panel = false,
    onOpened,
  }: {
    /** The phone shell's floating rail, which leaves out batch selection. */
    panel?: boolean;
    /** Called after an item's reply thread opened, so a floating rail can close. */
    onOpened?: () => void;
  } = $props();

  const localExecutionAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "chat.local-execution");
  const chat = useChat();
  const paneEvents = useChatPaneEvents();
  const { t } = getLocalization();

  let selectedIds = $state<ChatWorkAssignmentId[]>([]);
  let accepting = $state(false);
  let batchResult = $state<ChatBatchAcceptSummary | null>(null);
  let actionError = $state<string | null>(null);

  const queue = $derived(chat.reviewQueue);
  const total = $derived(reviewQueueCount(queue));
  /** Listed items; failures stay out, marked on their channel and reply thread instead. */
  const items = $derived(sidebarReviewQueueItems(queue?.items ?? []));
  const sections = $derived(reviewQueueSections(items));
  const hiddenCount = $derived(queue?.truncated ? Math.max(0, total - items.length) : 0);
  const canSelect = $derived(localExecutionAvailable && !panel);
  const hasReviews = $derived(items.some((item) => item.kind === "review"));
  /** Selected reviews still in the queue; ids of work that left the queue drop out. */
  const candidates = $derived(batchAcceptCandidates(items, new Set(selectedIds)));

  function setSelected(assignmentId: ChatWorkAssignmentId, selected: boolean): void {
    selectedIds = selected
      ? [...selectedIds.filter((id) => id !== assignmentId), assignmentId]
      : selectedIds.filter((id) => id !== assignmentId);
  }

  async function open(item: ChatReviewQueueItemRead): Promise<void> {
    actionError = null;
    try {
      await chat.openReviewItem(item);
      onOpened?.();
    } catch (error: unknown) {
      actionError = chatErrorMessage(error, t("chatErrors.actions.reviewQueueAction"));
    }
  }

  /** Opens the reply thread of a review, then the whole change set of its work in the review panel. */
  async function reviewChanges(item: ChatReviewQueueItemRead): Promise<void> {
    actionError = null;
    try {
      await chat.openReviewItem(item);
      const detail = await chat.assignmentReviewDetail(item.assignmentId);
      await tick();
      paneEvents.dispatchEvent(new CustomEvent("ganbaru:chat-open-review", { detail }));
      onOpened?.();
    } catch (error: unknown) {
      actionError = chatErrorMessage(error, t("chatErrors.actions.reviewQueueAction"));
    }
  }

  async function acceptSelected(): Promise<void> {
    const count = candidates.length;
    if (count === 0 || accepting) return;
    const confirmed = await requestConfirmation({
      title: t("chat.reviewQueue.confirmTitle", count),
      message: t("chat.reviewQueue.confirmMessage"),
      confirmLabel: t("chat.reviewQueue.confirm"),
    });
    if (!confirmed) return;
    accepting = true;
    batchResult = null;
    try {
      batchResult = await chat.acceptReviewItems(candidates.map((item) => item.assignmentId));
      selectedIds = [];
    } catch (error: unknown) {
      actionError = chatErrorMessage(error, t("chatErrors.actions.reviewQueueAction"));
    } finally {
      accepting = false;
    }
  }
</script>

{#if total > 0}
  <div class="review-queue">
    {#each sections as section (section.kind)}
      {#each section.items as item (item.assignmentId)}
        <div class="queue-row explorer-row" role="group" aria-label={t("chat.reviewQueue.open", item.teammateDisplayName, item.channelName)} data-review-kind={item.kind}>
          {#if canSelect && item.kind === "review"}
            <Checkbox
              class="queue-check"
              checked={selectedIds.includes(item.assignmentId)}
              disabled={accepting}
              label={t("chat.reviewQueue.select", item.teammateDisplayName, item.channelName)}
              onChange={(selected) => setSelected(item.assignmentId, selected)}
            />
          {/if}
          <button type="button" class="queue-open explorer-row-label" onclick={() => void open(item)}>
            <span class="queue-primary"><strong>{item.teammateDisplayName}</strong><span>#{item.channelName}</span></span>
            <span class="queue-secondary">
              <span class="queue-kind" data-kind={item.kind}>{t(`chat.reviewQueue.kinds.${item.kind}`)}</span>
              {#if item.kind === "review"}<span>{t("chat.reviewQueue.changedFiles", item.changedFileCount)}</span>{/if}
              {#if item.projectId !== chat.selectedChannel?.projectId}<span class="truncate">{item.projectName}</span>{/if}
            </span>
          </button>
          {#if localExecutionAvailable && item.kind === "review"}
            <button type="button" class="explorer-icon" aria-label={t("chat.reviewQueue.reviewChanges")} data-app-tooltip={t("chat.reviewQueue.reviewChanges")} onclick={() => void reviewChanges(item)}><FileDiff size={14} /></button>
          {/if}
        </div>
      {/each}
    {/each}
    {#if canSelect && hasReviews}
      <button type="button" class="queue-accept explorer-row-label" disabled={candidates.length === 0 || accepting} onclick={() => void acceptSelected()}>
        {t("chat.reviewQueue.acceptSelected", candidates.length)}
      </button>
    {/if}
    {#if batchResult}
      <Notice class="queue-notice" role="status" copy={false} message={t("chat.reviewQueue.batchResult", batchResult.accepted, batchResult.stale, batchResult.failed)} onDismiss={() => { batchResult = null; }} />
    {/if}
    {#if hiddenCount > 0}
      <Notice class="queue-notice" role="status" copy={false} message={t("chat.reviewQueue.truncated", hiddenCount)} />
    {/if}
    {#if chat.reviewQueueError}
      <Notice class="queue-notice" copy={false} message={t("chat.reviewQueue.refreshFailed")} onRetry={() => chat.reloadReviewQueue()} retryLabel={t("chat.reviewQueue.retry")} />
    {/if}
    {#if actionError}
      <Notice class="queue-notice" message={actionError} onDismiss={() => { actionError = null; }} />
    {/if}
  </div>
{/if}

<style>
  .review-queue { display: grid; gap: 0.1rem; }
  .queue-row { display: flex; min-width: 0; align-items: center; gap: 0.25rem; padding-inline-start: 0.5rem; }
  .queue-row :global(.queue-check) { flex: 0 0 auto; }
  .queue-open { display: grid; min-width: 0; flex: 1; gap: 0.1rem; border-radius: 0.35rem; padding: var(--explorer-row-padding) 0.25rem; color: var(--foreground); text-align: left; }
  .queue-row > .queue-open:first-child { margin-inline-start: -0.25rem; }
  .queue-primary, .queue-secondary { display: flex; min-width: 0; align-items: baseline; gap: 0.35rem; overflow: hidden; white-space: nowrap; }
  .queue-primary > strong { flex: 0 1 auto; overflow: hidden; text-overflow: ellipsis; }
  .queue-primary > span { min-width: 0; overflow: hidden; color: var(--muted-foreground); text-overflow: ellipsis; }
  .queue-secondary { color: var(--muted-foreground); font-size: calc(0.62rem * var(--type-scale)); }
  .queue-accept { margin-inline: 0.5rem; justify-self: start; border-radius: 0.35rem; padding: 0.2rem 0.4rem; color: var(--primary); }
  .queue-accept:disabled { color: var(--muted-foreground); }
  @media (hover: hover) {
    .queue-accept:not(:disabled):hover { background: var(--explorer-icon-hover, var(--accent)); }
  }
  .review-queue :global(.queue-notice) { margin-inline: 0.5rem; font-size: calc(0.68rem * var(--type-scale)); }
</style>
