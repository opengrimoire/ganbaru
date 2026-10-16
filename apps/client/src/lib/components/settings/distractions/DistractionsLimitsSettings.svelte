<script lang="ts">
  import { onMount } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Power from "@lucide/svelte/icons/power";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { getEventColor } from "$lib/calendar/utils";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    distractionsLimitEntryKey,
    type DistractionsLimitEntry,
    type DistractionsLimitPeriod,
    type DistractionsLimitTotal,
    type DistractionsUsageLimit,
  } from "$lib/distractions";
  import { getDistractions } from "$lib/stores/distractions.svelte";
  import { DISTRACTIONS_USAGE_REFRESH_INTERVAL_MS, getDistractionsUsage } from "$lib/stores/distractions-usage.svelte";
  import { createLifecycleScheduler } from "$lib/scheduling/lifecycle-scheduler";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import DistractionsBrowserConnectionStatus from "$lib/components/settings/distractions/DistractionsBrowserConnectionStatus.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import type { DistractionsLimitEditorTarget } from "$lib/settings/types";

  interface PendingLimitAction {
    type: "disable" | "delete" | "global-disable";
    limitId: string | null;
  }

  let {
    onOpenLimitEditor = () => {},
  }: {
    onOpenLimitEditor?: (target: DistractionsLimitEditorTarget) => void;
  } = $props();

  const distractions = getDistractions();
  const usage = getDistractionsUsage();
  const theme = getTheme();
  const { t } = getLocalization();

  onMount(() => {
    if (__GANBARU_BUILD_PLATFORM__ !== "android") {
      void usage.refresh();
      return;
    }
    const reader = createLifecycleScheduler({
      run: async (context) => {
        await usage.refresh(context);
        return context.isCurrent() ? context.now() + DISTRACTIONS_USAGE_REFRESH_INTERVAL_MS : null;
      },
    });
    reader.setEnabled(true);
    return () => reader.dispose();
  });

  let pendingAction = $state<PendingLimitAction | null>(null);

  /** Native observation only runs for enabled limits that link a desktop app, so other states have nothing to report. */
  const tracksDesktopApps = $derived(
    distractions.limitsEnabled
      && distractions.usageLimits.some((limit) => limit.enabled && limit.entries.some((entry) => entry.desktopAppName)),
  );
  /** Wayland sessions without foreground tracking fall back to open-app matching, so they are not reported. */
  const desktopTrackingUnavailable = $derived(
    __GANBARU_BUILD_PLATFORM__ !== "android"
      && tracksDesktopApps
      && !usage.foregroundStatus.available
      && !usage.foregroundStatus.reason?.toLowerCase().includes("wayland"),
  );
  const desktopUnavailableMessage = $derived(t("settings.distractions.limits.desktopUnavailable"));

  function formatMinutes(totalSeconds: number): string {
    const minutes = Math.floor(totalSeconds / 60);
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.floor(minutes / 60);
    const remainder = minutes % 60;
    return remainder > 0 ? `${hours}h ${remainder}m` : `${hours}h`;
  }

  interface LimitProgressSegment {
    entry: DistractionsLimitEntry;
    usedSeconds: number;
    widthPercent: number;
    color: string;
  }

  interface LimitBudgetView {
    total: DistractionsLimitTotal;
    period: DistractionsLimitPeriod;
    segments: LimitProgressSegment[];
  }

  function entryColor(entry: DistractionsLimitEntry): string {
    return getEventColor(entry.color ?? undefined, theme.current).bg;
  }

  function totalPeriod(total: DistractionsLimitTotal): DistractionsLimitPeriod {
    return total.period ?? "day";
  }

  function periodUsageLabel(period: DistractionsLimitPeriod): string {
    return period === "week"
      ? t("settings.distractions.limits.thisWeek")
      : t("settings.distractions.limits.today");
  }

  function formatLimitSeconds(seconds: number): string {
    return formatMinutes(seconds);
  }

  function limitProgressSegments(
    limit: DistractionsUsageLimit,
    total: DistractionsLimitTotal,
  ): LimitProgressSegment[] {
    const entryTotals = usage.entryTotalsFor(limit.id, totalPeriod(total));
    const usedSecondsByEntryId = new Map(
      entryTotals.map((entryTotal) => [entryTotal.entryId, entryTotal.usedSeconds] as const),
    );
    const totalUsedSeconds = entryTotals.reduce(
      (total, entryTotal) => total + entryTotal.usedSeconds,
      0,
    );
    const widthBaseSeconds = Math.max(total.limitSeconds, totalUsedSeconds, 1);
    return limit.entries
      .map((entry) => {
        const usedSeconds = usedSecondsByEntryId.get(entry.id) ?? 0;
        return {
          entry,
          usedSeconds,
          widthPercent: Math.min(100, Math.max(0, (usedSeconds / widthBaseSeconds) * 100)),
          color: entryColor(entry),
        };
      })
      .filter((segment) => segment.usedSeconds > 0 && segment.widthPercent > 0);
  }

  function limitBudgetViews(limit: DistractionsUsageLimit): LimitBudgetView[] {
    return usage.totalsFor(limit.id)
      .filter((total) => totalPeriod(total) === "day"
        ? limit.minutesPerDay !== null
        : limit.minutesPerWeek !== null && limit.minutesPerWeek !== undefined)
      .map((total) => ({
        total,
        period: totalPeriod(total),
        segments: limitProgressSegments(limit, total),
      }))
      .sort((a, b) => {
        const order = { day: 0, week: 1 } satisfies Record<DistractionsLimitPeriod, number>;
        return order[a.period] - order[b.period];
      });
  }

  function websiteDisplayName(host: string): string {
    const firstLabel = host.replace(/^www\./, "").split(".", 1)[0] ?? host;
    return `${firstLabel.charAt(0).toUpperCase()}${firstLabel.slice(1)}`;
  }

  function entryLabel(entry: DistractionsLimitEntry): string {
    return entry.name
      ?? entry.mobileAppName
      ?? entry.desktopAppName
      ?? (entry.websiteHost ? websiteDisplayName(entry.websiteHost) : t("settings.distractions.limits.linkedSource"));
  }

  function requestLimitEnabledChange(limit: DistractionsUsageLimit, enabled: boolean): void {
    if (enabled) {
      distractions.setUsageLimitEnabled(limit.id, true);
      void usage.refresh();
      return;
    }
    pendingAction = { type: "disable", limitId: limit.id };
  }

  function requestLimitDelete(limit: DistractionsUsageLimit): void {
    pendingAction = { type: "delete", limitId: limit.id };
  }

  function requestGlobalEnabledChange(enabled: boolean): void {
    if (enabled) {
      distractions.setLimitsEnabled(true);
      void usage.refresh();
      return;
    }
    pendingAction = { type: "global-disable", limitId: null };
  }

  function confirmPendingAction(): void {
    if (!pendingAction) return;
    if (pendingAction.type === "global-disable") {
      distractions.setLimitsEnabled(false);
    } else if (pendingAction.limitId) {
      if (pendingAction.type === "disable") {
        distractions.setUsageLimitEnabled(pendingAction.limitId, false);
      } else {
        distractions.removeUsageLimit(pendingAction.limitId);
      }
    }
    pendingAction = null;
    void usage.refresh();
  }

  function cancelPendingAction(): void {
    pendingAction = null;
  }

  function pendingTitle(action: PendingLimitAction): string {
    if (action.type === "global-disable") return t("settings.distractions.limits.turnOffTitle");
    const limit = distractions.usageLimits.find((item) => item.id === action.limitId);
    const name = limit?.name ?? t("settings.distractions.limits.thisLimit");
    return action.type === "disable"
      ? t("settings.distractions.limits.disableTitle", name)
      : t("settings.distractions.limits.deleteTitle", name);
  }

  function pendingMessage(action: PendingLimitAction): string {
    if (action.type === "global-disable") {
      return t("settings.distractions.limits.turnOffMessage");
    }
    return action.type === "disable"
      ? t("settings.distractions.limits.disableMessage")
      : t("settings.distractions.limits.deleteMessage");
  }

  function pendingConfirmLabel(action: PendingLimitAction): string {
    if (action.type === "global-disable") return t("settings.distractions.shared.turnOffAction");
    return action.type === "disable"
      ? t("settings.distractions.shared.disableAction")
      : t("settings.distractions.shared.deleteAction");
  }
</script>

{#snippet limitList()}
  <section class="flex flex-col gap-4">
    <div class="flex min-w-0 flex-wrap items-center justify-between gap-2 px-1">
      <div class="min-w-0">
        <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.limits.usageLimitsHeading")}</h2>
        <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
          {t("settings.distractions.limits.usageLimitsDescription")}
        </div>
      </div>
      <button
        type="button"
        onclick={() => onOpenLimitEditor({ mode: "create" })}
        class="flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent dark:bg-transparent"
      >
        <Plus size={13} strokeWidth={2.25} />
        <span>{t("settings.distractions.limits.addLimit")}</span>
      </button>
    </div>

    <div class="flex flex-col px-1">
      {#each distractions.usageLimits as limit (limit.id)}
        {@const budgetViews = limitBudgetViews(limit)}
        <article
          class={cn(
            "flex min-w-0 flex-col gap-1.5 border-b border-border/70 py-4 transition-opacity",
            !limit.enabled && "opacity-50",
          )}
          aria-label={limit.enabled
            ? limit.name
            : t("settings.distractions.limits.disabledLabel", limit.name)}
        >
          <div class="flex min-w-0 flex-wrap items-start justify-between gap-2">
            <div class="min-w-0 flex-1">
              <h3 class="truncate text-[0.866667rem] text-foreground">
                {limit.name}
              </h3>
              <div class="mt-0.5 flex min-w-0 flex-col gap-2">
                {#each budgetViews as view (`${limit.id}:${view.period}`)}
                  <div class="flex min-w-0 flex-col gap-2 pt-2 first:pt-0">
                    <div class="text-[0.8rem] text-muted-foreground">
                      {t(
                        "settings.distractions.limits.usedOfLimit",
                        formatMinutes(view.total.usedSeconds),
                        formatLimitSeconds(view.total.limitSeconds),
                        periodUsageLabel(view.period),
                      )}
                    </div>

                    <div class="flex h-1.5 overflow-hidden rounded-full bg-muted">
                      {#if view.segments.length > 0}
                        {#each view.segments as segment (segment.entry.id)}
                          <div
                            class="h-full first:rounded-l-full last:rounded-r-full"
                            style={`width: ${segment.widthPercent}%; background-color: ${segment.color};`}
                            title={`${entryLabel(segment.entry)}: ${formatMinutes(segment.usedSeconds)}`}
                          ></div>
                        {/each}
                      {/if}
                    </div>

                    <div class="flex min-w-0 flex-wrap gap-1.5">
                      {#each limit.entries as entry (distractionsLimitEntryKey(entry))}
                        <span class="inline-flex max-w-full items-center gap-1.5 rounded-full border border-border bg-transparent px-2 py-1 text-[0.733333rem] text-foreground">
                          <span
                            class="h-2 w-2 shrink-0 rounded-full"
                            style={`background-color: ${entryColor(entry)};`}
                            aria-hidden="true"
                          ></span>
                          <span class="truncate">{entryLabel(entry)}</span>
                        </span>
                      {/each}
                    </div>
                  </div>
                {:else}
                  <div class="text-[0.8rem] text-muted-foreground">
                    {t("settings.distractions.limits.totalsUnavailable")}
                  </div>
                {/each}
              </div>
            </div>
            <div class="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                onclick={() => requestLimitEnabledChange(limit, !limit.enabled)}
                aria-label={limit.enabled
                  ? t("settings.distractions.shared.disable", limit.name)
                  : t("settings.distractions.shared.enable", limit.name)}
                data-app-tooltip-disabled="true"
                class="flex h-7 w-24 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2 text-[0.8rem] text-foreground transition-colors hover:bg-accent dark:bg-transparent"
              >
                {#if limit.enabled}
                  <Check size={13} strokeWidth={2.25} class="shrink-0" />
                  <span>{t("settings.distractions.shared.enabled")}</span>
                {:else}
                  <Power size={13} strokeWidth={2} class="shrink-0" />
                  <span>{t("settings.distractions.shared.disabled")}</span>
                {/if}
              </button>
              <button
                type="button"
                onclick={() => onOpenLimitEditor({ mode: "edit", limitId: limit.id })}
                disabled={!limit.enabled}
                aria-label={t("settings.distractions.shared.edit", limit.name)}
                data-app-tooltip-disabled="true"
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border bg-card text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40 dark:bg-transparent"
              >
                <Pencil size={13} strokeWidth={2} />
              </button>
              <button
                type="button"
                onclick={() => requestLimitDelete(limit)}
                aria-label={t("settings.distractions.shared.delete", limit.name)}
                data-app-tooltip-disabled="true"
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border bg-card text-foreground transition-colors hover:bg-accent dark:bg-transparent"
              >
                <Trash2 size={13} strokeWidth={2} />
              </button>
            </div>
          </div>
        </article>
      {:else}
        <div class="flex h-10 items-center border-b border-border/70 text-[0.8rem] text-muted-foreground">
          {t("settings.distractions.limits.noUsageLimits")}
        </div>
      {/each}
    </div>
  </section>
{/snippet}

<div class="flex flex-col gap-6">
  <DistractionsBrowserConnectionStatus />

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.limits.usageLimitConfigurationHeading")}</h2>
    <div class="flex flex-col gap-3">
      <SwitchField
        label={t("settings.distractions.limits.enableUsageLimits")}
        description={t("settings.distractions.limits.enableUsageLimitsDescription")}
        checked={distractions.limitsEnabled}
        onChange={requestGlobalEnabledChange}
      />
    </div>
  </section>

  <fieldset
    disabled={!distractions.limitsEnabled}
    aria-disabled={!distractions.limitsEnabled}
    class={cn(
      "m-0 flex min-w-0 flex-col gap-6 border-0 p-0 transition-opacity",
      !distractions.limitsEnabled && "opacity-50",
    )}
  >
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    {#if desktopTrackingUnavailable}
      <Notice
        class="px-1 text-[0.8rem] leading-5"
        tone="warning"
        role="status"
        message={desktopUnavailableMessage}
        copy={usage.foregroundStatus.reason ? `${desktopUnavailableMessage} (${usage.foregroundStatus.reason})` : false}
      />

      <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>
    {/if}

    {@render limitList()}
  </fieldset>
</div>

{#if pendingAction}
  <ConfirmDialog
    title={pendingTitle(pendingAction)}
    message={pendingMessage(pendingAction)}
    confirmLabel={pendingConfirmLabel(pendingAction)}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmPendingAction}
    onCancel={cancelPendingAction}
  />
{/if}
