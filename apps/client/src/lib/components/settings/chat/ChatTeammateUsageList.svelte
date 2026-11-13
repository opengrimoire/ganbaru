<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatParticipantId, ChatTeammateUsageDayRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    formatTokenCount,
    formatWorkingDuration,
    teammateUsageRange,
  } from "$lib/chat/budgets/budget-model";
  import { formatDateLabel } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /** The last days of one teammate's usage, summed across the linked devices that ran its work. */
  let { teammateId }: { teammateId: ChatParticipantId } = $props();

  const localization = getLocalization();
  const t = localization.t;
  let days = $state<ChatTeammateUsageDayRead[] | null>(null);
  let error = $state<string | null>(null);
  let request = 0;

  async function load(id: ChatParticipantId): Promise<void> {
    const current = ++request;
    error = null;
    const { fromDate, toDate } = teammateUsageRange(Temporal.Now.plainDateISO());
    try {
      const read = await chatApi.readChatTeammateUsage(id, fromDate, toDate);
      if (current === request) days = [...read].sort((left, right) => right.localDate.localeCompare(left.localDate));
    } catch (cause) {
      if (current === request) error = chatErrorMessage(cause, t("settings.chat.teammates.usageLoadFailed"));
    }
  }

  function dayLabel(localDate: string): string {
    const date = Temporal.PlainDate.from(localDate);
    return formatDateLabel(localization.locale, Date.UTC(date.year, date.month - 1, date.day), {
      weekday: "short",
      month: "short",
      day: "numeric",
      timeZone: "UTC",
    });
  }

  $effect(() => {
    void load(teammateId);
  });
</script>

<div class="usage-list">
  {#if error}
    <Notice message={error} onRetry={() => load(teammateId)} copy={false} />
  {:else if days && days.length === 0}
    <p class="usage-empty">{t("settings.chat.teammates.usageEmpty")}</p>
  {:else if days}
    <ul>
      {#each days as day (day.localDate)}
        <li>
          <time datetime={day.localDate}>{dayLabel(day.localDate)}</time>
          <span>{t(
            "settings.chat.teammates.usageDay",
            formatTokenCount(localization.locale, day.countedTokens),
            formatWorkingDuration(localization.locale, day.workingSeconds),
            day.runCount,
          )}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .usage-list ul {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .usage-list li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.75rem;
    justify-content: space-between;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .usage-list time,
  .usage-empty {
    color: var(--muted-foreground);
  }

  .usage-empty {
    margin: 0;
    font-size: calc(0.733333rem * var(--type-scale));
  }
</style>
