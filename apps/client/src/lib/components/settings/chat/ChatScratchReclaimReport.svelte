<script lang="ts">
  import { onMount } from "svelte";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatScratchReclaimReason,
    ChatScratchReclaimReportRead,
    ChatScratchReclaimRow,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    asyncResourceError,
    failAsyncRequest,
    idleAsyncResource,
    isAsyncResourceLoading,
    resolveAsyncRequest,
    startAsyncRequest,
    type AsyncResource,
  } from "$lib/utils/async-resource";
  import type ChatScratchManagerComponent from "./ChatScratchManager.svelte";
  import { SETTINGS_ICON_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * Ranks reclaimable private scratch on this device and opens the scratch manager for review.
   *
   * The report never removes anything; cleanup stays previewed and confirmed in the manager.
   */
  const localization = getLocalization();
  const { t } = localization;
  /** The report; a newer refresh, such as the one after a review closes, supersedes one in flight. */
  let reportResource = $state.raw<AsyncResource<ChatScratchReclaimReportRead | null>>(idleAsyncResource(null));
  let reviewError = $state<string | null>(null);
  const report = $derived(reportResource.data);
  const busy = $derived(isAsyncResourceLoading(reportResource));
  const error = $derived(reviewError ?? asyncResourceError(reportResource));
  let ScratchManager = $state<typeof ChatScratchManagerComponent | null>(null);
  let reviewing = $state<ChatScratchReclaimRow | null>(null);

  function reasonLabel(reason: ChatScratchReclaimReason, idleDays: number): string {
    switch (reason) {
      case "scopeArchived": return t("settings.chat.behavior.reclaimReasonScopeArchived");
      case "scopeCleanupFailed": return t("settings.chat.behavior.reclaimReasonScopeCleanupFailed");
      case "generationCleanupFailed": return t("settings.chat.behavior.reclaimReasonGenerationCleanupFailed");
      case "generationQuarantined": return t("settings.chat.behavior.reclaimReasonGenerationQuarantined");
      case "idle": return t("settings.chat.behavior.reclaimReasonIdle", formatNumber(localization.locale, idleDays));
    }
  }

  function bytesLabel(bytes: number): string {
    return t("settings.agents.scratchManager.bytes", formatNumber(localization.locale, bytes));
  }

  function locationLabel(row: ChatScratchReclaimRow): string {
    return [row.groupName, row.projectName, `#${row.channelName}`].filter(Boolean).join(" / ");
  }

  function activityLabel(row: ChatScratchReclaimRow): string {
    return t(
      "settings.chat.behavior.reclaimLastActivity",
      formatDateTime(localization.locale, new Date(row.lastActivityAt), { dateStyle: "short", timeStyle: "short" }),
    );
  }

  async function refresh(): Promise<void> {
    reviewError = null;
    const started = startAsyncRequest(reportResource);
    reportResource = started.resource;
    try {
      const next = await chatApi.readChatScratchReclaimReport();
      reportResource = resolveAsyncRequest(reportResource, started.token, next);
    } catch (cause: unknown) {
      reportResource = failAsyncRequest(reportResource, started.token, chatErrorMessage(cause));
    }
  }

  async function review(row: ChatScratchReclaimRow): Promise<void> {
    if (reviewing) return;
    reviewError = null;
    try {
      ScratchManager ??= (await import("./ChatScratchManager.svelte")).default;
      reviewing = row;
    } catch (cause: unknown) {
      reviewError = chatErrorMessage(cause);
    }
  }

  function closeReview(): void {
    reviewing = null;
    void refresh();
  }

  onMount(() => {
    void refresh();
  });
</script>

<section class="flex flex-col gap-4">
  <div class="flex items-center justify-between gap-4 px-1">
    <div class="min-w-0 flex-1">
      <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.reclaimTitle")}</h2>
      {#if report}<p class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.chat.behavior.reclaimDescription", formatNumber(localization.locale, report.idleDays))}</p>{/if}
    </div>
    <button type="button" class={SETTINGS_ICON_BUTTON_CLASS} disabled={busy} aria-label={t("settings.chat.behavior.reclaimRefresh")} title={t("settings.chat.behavior.reclaimRefresh")} onclick={() => void refresh()}><RotateCw size={13} aria-hidden="true" /></button>
  </div>
  {#if error}<Notice class="px-1 text-[0.733333rem]" message={error} />{/if}
  {#if report}
    {#if report.rows.length === 0}
      <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.chat.behavior.reclaimEmpty")}</p>
    {:else}
      <p class="px-1 text-[0.866667rem] text-foreground">{report.totalTruncated
        ? t("settings.chat.behavior.reclaimTotalLimited", formatNumber(localization.locale, report.totalBytes))
        : t("settings.chat.behavior.reclaimTotal", formatNumber(localization.locale, report.totalBytes))}</p>
      <ul class="flex flex-col gap-2 px-1">
        {#each report.rows as row (row.scratchGenerationId)}
          {@const generationLabel = t("settings.agents.scratchManager.generation", row.generation)}
          <li class="flex min-w-0 items-center gap-2">
            <div class="min-w-0 flex-1">
              <p class="truncate text-[0.866667rem] text-foreground">{row.agentName} · {generationLabel}</p>
              <p class="truncate text-[0.733333rem] text-muted-foreground">{locationLabel(row)}</p>
              <p class="truncate text-[0.733333rem] text-muted-foreground">{[
                reasonLabel(row.reason, report.idleDays),
                row.deviceAvailability === "available" ? bytesLabel(row.byteSize) : t("settings.agents.scratchManager.unavailable"),
                row.deviceAvailability === "available" && row.sizeTruncated ? t("settings.agents.scratchManager.sizeLimited") : null,
                activityLabel(row),
              ].filter(Boolean).join(" · ")}</p>
            </div>
            <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={reviewing !== null} aria-label={t("settings.chat.behavior.reclaimReviewLabel", row.agentName, generationLabel)} onclick={() => void review(row)}>{t("settings.chat.behavior.reclaimReview")}</button>
          </li>
        {/each}
      </ul>
      {#if report.omitted > 0}
        <p class="px-1 text-[0.733333rem] text-muted-foreground">{t("settings.chat.behavior.reclaimOmitted", formatNumber(localization.locale, report.omitted))}</p>
      {/if}
    {/if}
  {/if}
</section>

{#if reviewing && ScratchManager}
  {@const Manager = ScratchManager}
  <Manager initialReplyThreadId={reviewing.replyThreadId} initialGenerationId={reviewing.scratchGenerationId} onClose={closeReview} />
{/if}
