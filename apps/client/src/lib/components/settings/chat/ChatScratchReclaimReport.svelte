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
  import type ChatScratchManagerComponent from "./ChatScratchManager.svelte";
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
  let report = $state<ChatScratchReclaimReportRead | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
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
    return t("settings.chat.teammates.scratchManager.bytes", formatNumber(localization.locale, bytes));
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
    if (busy) return;
    busy = true;
    error = null;
    try {
      report = await chatApi.readChatScratchReclaimReport();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
    } finally {
      busy = false;
    }
  }

  async function review(row: ChatScratchReclaimRow): Promise<void> {
    if (reviewing) return;
    error = null;
    try {
      ScratchManager ??= (await import("./ChatScratchManager.svelte")).default;
      reviewing = row;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
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

<div class="space-y-2">
  <div class="flex items-center justify-between gap-2">
    <h4 class="text-sm font-medium">{t("settings.chat.behavior.reclaimTitle")}</h4>
    <button type="button" class="reclaim-action inline-flex items-center gap-1" disabled={busy} aria-label={t("settings.chat.behavior.reclaimRefresh")} title={t("settings.chat.behavior.reclaimRefresh")} onclick={() => void refresh()}><RotateCw class="size-3.5" aria-hidden="true" /></button>
  </div>
  {#if error}<Notice class="text-sm" message={error} />{/if}
  {#if report}
    <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.reclaimDescription", formatNumber(localization.locale, report.idleDays))}</p>
    {#if report.rows.length === 0}
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.reclaimEmpty")}</p>
    {:else}
      <p class="text-sm">{report.totalTruncated
        ? t("settings.chat.behavior.reclaimTotalLimited", formatNumber(localization.locale, report.totalBytes))
        : t("settings.chat.behavior.reclaimTotal", formatNumber(localization.locale, report.totalBytes))}</p>
      <ul class="space-y-2">
        {#each report.rows as row (row.scratchGenerationId)}
          {@const generationLabel = t("settings.chat.teammates.scratchManager.generation", row.generation)}
          <li class="flex min-w-0 items-center gap-2">
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm">{row.teammateName} · {generationLabel}</p>
              <p class="truncate text-xs text-muted-foreground">{locationLabel(row)}</p>
              <p class="truncate text-xs text-muted-foreground">{[
                reasonLabel(row.reason, report.idleDays),
                row.deviceAvailability === "available" ? bytesLabel(row.byteSize) : t("settings.chat.teammates.scratchManager.unavailable"),
                row.deviceAvailability === "available" && row.sizeTruncated ? t("settings.chat.teammates.scratchManager.sizeLimited") : null,
                activityLabel(row),
              ].filter(Boolean).join(" · ")}</p>
            </div>
            <button type="button" class="reclaim-action shrink-0" disabled={reviewing !== null} aria-label={t("settings.chat.behavior.reclaimReviewLabel", row.teammateName, generationLabel)} onclick={() => void review(row)}>{t("settings.chat.behavior.reclaimReview")}</button>
          </li>
        {/each}
      </ul>
      {#if report.omitted > 0}
        <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.reclaimOmitted", formatNumber(localization.locale, report.omitted))}</p>
      {/if}
    {/if}
  {/if}
</div>

{#if reviewing && ScratchManager}
  {@const Manager = ScratchManager}
  <Manager initialReplyThreadId={reviewing.replyThreadId} initialGenerationId={reviewing.scratchGenerationId} onClose={closeReview} />
{/if}

<style>
  .reclaim-action { border: 1px solid var(--border); border-radius: 0.375rem; padding: 0.35rem 0.65rem; font-size: calc(0.733333rem * var(--type-scale)); }
  .reclaim-action:disabled { cursor: not-allowed; opacity: 0.5; }
</style>
