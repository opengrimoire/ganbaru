<!--
  @component
  One small ring per plan limit window showing the share left, each its own control sized like
  the header's account icons. Hovering or focusing a ring shows that window with its reset time
  in a tooltip. Renders nothing without an observation.
-->
<script lang="ts">
  import type { ProviderInstanceRead } from "$lib/chat/contracts";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { planLimitInstantLabel, planLimitWindowLine } from "$lib/chat/usage/plan-limit-labels";
  import { providerPlanLimits, type ProviderPlanLimitWindow } from "$lib/chat/usage/provider-readiness";
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
  const staleLine = $derived(limits?.stale
    ? t("chat.composer.planLimitsUpdated", planLimitInstantLabel(localization, limits.observedAtMs, nowMs))
    : null);

  /** The tooltip of one ring: its window, then when the reading was taken if it is out of date. */
  function tooltip(window: ProviderPlanLimitWindow): string {
    const line = planLimitWindowLine(localization, window, nowMs);
    return staleLine ? `${line}\n${staleLine}` : line;
  }
</script>

{#if limits}
  {#each limits.windows as window (window.id)}
    <button
      type="button"
      class="plan-limit-ring"
      aria-label={`${t("chat.composer.planLimits")}: ${tooltip(window)}`}
      data-app-tooltip={tooltip(window)}
      data-app-tooltip-keep-on-click="true"
    >
      <PomodoroProgressRing
        active
        remainingSeconds={window.remainingPercent}
        totalSeconds={PERCENT_TOTAL}
        size={RING_SIZE}
        strokeWidth={RING_STROKE}
        trackClass="stroke-current opacity-25"
        progressClass="stroke-current"
      />
    </button>
  {/each}
{/if}

<style>
  .plan-limit-ring { display: inline-grid; width: 1rem; height: 1rem; flex: 0 0 auto; place-items: center; border-radius: 0.2rem; color: var(--muted-foreground); }
  .plan-limit-ring:hover, .plan-limit-ring:focus-visible { color: var(--foreground); }
</style>
