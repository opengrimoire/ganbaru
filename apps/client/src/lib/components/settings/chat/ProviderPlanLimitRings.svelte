<!--
  @component
  One small ring per plan limit window showing the share left, sized like the header's account
  icons. Hovering or focusing shows each window with its reset time in a tooltip. Renders
  nothing without an observation.
-->
<script lang="ts">
  import type { ProviderInstanceRead } from "$lib/chat/contracts";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { planLimitInstantLabel, planLimitWindowLine } from "$lib/chat/usage/plan-limit-labels";
  import { providerPlanLimits } from "$lib/chat/usage/provider-readiness";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let { provider }: { provider: ProviderInstanceRead } = $props();

  /** Ring diameter in pixels, matching the 12 px account icons beside it. */
  const RING_SIZE = 12;
  /** Stroke in ring view box units, close to the account icons' line weight at this size. */
  const RING_STROKE = 1.8;
  const PERCENT_TOTAL = 100;

  const localization = getLocalization();
  const { t } = localization;
  const nowMs = $derived(provider.usageLimits ? Date.now() : 0);
  const limits = $derived(providerPlanLimits(provider, nowMs));
  const summary = $derived.by(() => {
    if (!limits) return "";
    const lines = [
      t("chat.composer.planLimits"),
      ...limits.windows.map((window) => planLimitWindowLine(localization, window, nowMs)),
    ];
    if (limits.stale) {
      lines.push(t("chat.composer.planLimitsUpdated", planLimitInstantLabel(localization, limits.observedAtMs, nowMs)));
    }
    return lines.join("\n");
  });
</script>

{#if limits && limits.windows.length > 0}
  <button
    type="button"
    class="plan-limit-rings"
    aria-label={summary}
    data-app-tooltip={summary}
    data-app-tooltip-keep-on-click="true"
  >
    {#each limits.windows as window (window.id)}
      <PomodoroProgressRing
        active
        remainingSeconds={window.remainingPercent}
        totalSeconds={PERCENT_TOTAL}
        size={RING_SIZE}
        strokeWidth={RING_STROKE}
        trackClass="stroke-current opacity-25"
        progressClass="stroke-current"
      />
    {/each}
  </button>
{/if}

<style>
  .plan-limit-rings { display: inline-flex; height: 1rem; flex: 0 0 auto; align-items: center; gap: 0.18rem; border-radius: 0.2rem; padding-inline: 0.1rem; color: var(--muted-foreground); }
  .plan-limit-rings:hover, .plan-limit-rings:focus-visible { color: var(--foreground); }
</style>
