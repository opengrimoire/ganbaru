<!--
  @component
  One compact line of a provider's plan limits, such as `5h 93% left (resets 20:00) · Week 99% left`,
  read from the newest limits this device observed. Renders nothing without an observation.
-->
<script lang="ts">
  import type { ProviderInstanceRead } from "$lib/chat/contracts";
  import { planLimitWindowName, providerPlanLimits } from "$lib/chat/usage/provider-readiness";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    provider,
    showHeading = false,
  }: {
    provider: ProviderInstanceRead;
    /** Shows a "Plan limits" label above the line, for surfaces without their own heading. */
    showHeading?: boolean;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const nowMs = $derived(provider.usageLimits ? Date.now() : 0);
  const limits = $derived(providerPlanLimits(provider, nowMs));

  /** Formats an instant as a time today, or with the weekday and day otherwise. */
  function instantLabel(ms: number): string {
    const sameDay = new Date(ms).toDateString() === new Date(nowMs).toDateString();
    return formatDateTime(localization.locale, ms, sameDay
      ? { hour: "numeric", minute: "2-digit" }
      : { weekday: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  }
</script>

{#if limits && limits.windows.length > 0}
  <div class="plan-limits">
    {#if showHeading}<span class="plan-limits-heading">{t("chat.composer.planLimits")}</span>{/if}
    <span class="plan-limits-line">
      {#each limits.windows as window, index (window.id)}
        {#if index > 0}<span class="plan-limits-separator" aria-hidden="true">·</span>{/if}
        <span class="plan-limits-window" class:exhausted={window.remainingPercent <= 0}>
          <span class="plan-limits-name">{planLimitWindowName(window, t("chat.composer.rateLimitWeek"))}</span>
          {t("chat.composer.rateLimitRemaining", formatNumber(localization.locale, window.remainingPercent, { maximumFractionDigits: 0 }))}
          {#if window.resetsAtMs !== null}<small>({t("chat.composer.rateLimitResets", instantLabel(window.resetsAtMs))})</small>{/if}
        </span>
      {/each}
    </span>
    {#if limits.stale}<small class="plan-limits-updated">{t("chat.composer.planLimitsUpdated", instantLabel(limits.observedAtMs))}</small>{/if}
  </div>
{/if}

<style>
  .plan-limits { display: grid; min-width: 0; gap: 0.18rem; font-size: var(--panel-font-size); }
  .plan-limits-heading, .plan-limits small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .plan-limits-line { display: flex; min-width: 0; flex-wrap: wrap; align-items: baseline; column-gap: 0.4rem; row-gap: 0.1rem; }
  .plan-limits-window { white-space: nowrap; }
  .plan-limits-window.exhausted { color: var(--destructive); }
  .plan-limits-name { font-weight: 600; }
  .plan-limits-separator { color: var(--muted-foreground); }
</style>
