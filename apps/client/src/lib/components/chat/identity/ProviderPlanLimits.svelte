<!--
  @component
  A provider's plan limits under a "Plan limits" label, one window per line, such as
  `5h 93% left (resets 20:00)`, read from the newest limits this device observed or a linked
  desktop shared. Renders nothing without an observation.
-->
<script lang="ts">
  import type { ProviderInstanceRead } from "$lib/chat/contracts";
  import { planLimitInstantLabel, planLimitWindowLine } from "$lib/chat/usage/plan-limit-labels";
  import { providerPlanLimits } from "$lib/chat/usage/provider-readiness";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let { provider }: { provider: Pick<ProviderInstanceRead, "usageLimits"> } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const nowMs = $derived(provider.usageLimits ? Date.now() : 0);
  const limits = $derived(providerPlanLimits(provider, nowMs));
</script>

{#if limits && limits.windows.length > 0}
  <div class="plan-limits" data-plan-limits>
    <span class="plan-limits-label">{t("chat.composer.planLimits")}</span>
    {#each limits.windows as window (window.id)}
      <span>{planLimitWindowLine(localization, window, nowMs)}</span>
    {/each}
    {#if limits.stale}
      <small>{t("chat.composer.planLimitsUpdated", planLimitInstantLabel(localization, limits.observedAtMs, nowMs))}</small>
    {/if}
  </div>
{/if}

<style>
  .plan-limits { display: grid; min-width: 0; gap: 0.22rem; font-size: var(--panel-font-size); }
  .plan-limits-label, .plan-limits small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
</style>
