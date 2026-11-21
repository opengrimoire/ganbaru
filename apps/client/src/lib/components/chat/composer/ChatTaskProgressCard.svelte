<!--
  @component
  A calm summary of the agent's latest structured plan above the reply composer: how many steps
  are done, one bar segment per step, and the step in progress. The step text collapses away.
-->
<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import type { PlanProgressSummary } from "$lib/chat/composer/plan-progress";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let { summary }: { summary: PlanProgressSummary } = $props();

  const { t } = getLocalization();
  let expanded = $state(true);
  const bodyId = `chat-task-progress-${crypto.randomUUID()}`;
</script>

<section class="task-progress surface-floating" data-chat-task-progress aria-label={t("chat.composer.taskProgress.label")}>
  <button type="button" class="task-progress-header" aria-expanded={expanded} aria-controls={bodyId} onclick={() => { expanded = !expanded; }}>
    <span class="task-progress-title">{t("chat.composer.taskProgress.title", summary.completed, summary.total)}</span>
    <span class="task-progress-bar" aria-hidden="true">
      {#each summary.segments as status, index (index)}<span class="task-progress-segment" data-status={status}></span>{/each}
    </span>
    <ChevronDown size={14} class={expanded ? "task-progress-chevron expanded" : "task-progress-chevron"} aria-hidden="true" />
  </button>
  {#if expanded}
    <div id={bodyId} class="task-progress-body">
      {#if summary.activeStep}<p class="task-progress-step" data-chat-task-active-step>{summary.activeStep.text}</p>{/if}
      {#if summary.failed > 0}<p class="task-progress-failed">{t("chat.composer.taskProgress.failed", summary.failed)}</p>{/if}
    </div>
  {/if}
</section>

<style>
  .task-progress { display: grid; gap: 0.2rem; margin: 0 0.35rem 0.35rem; border-radius: 0.55rem; background: var(--cal-bg); padding: 0.3rem 0.55rem; }
  .task-progress-header { display: flex; min-width: 0; align-items: center; gap: 0.55rem; min-height: 1.75rem; text-align: start; }
  .task-progress-title { flex: 0 0 auto; color: var(--foreground); font-size: calc(0.7rem * var(--type-scale)); font-weight: 600; }
  .task-progress-bar { display: flex; min-width: 0; flex: 1 1 0%; gap: 0.15rem; }
  .task-progress-segment { height: 0.3rem; min-width: 0.25rem; flex: 1 1 0%; border-radius: 999px; background: color-mix(in srgb, var(--muted-foreground) 22%, transparent); }
  .task-progress-segment[data-status="completed"] { background: var(--primary); }
  .task-progress-segment[data-status="active"] { background: color-mix(in srgb, var(--primary) 50%, transparent); }
  .task-progress-segment[data-status="failed"],.task-progress-segment[data-status="interrupted"] { background: var(--destructive); }
  .task-progress :global(.task-progress-chevron) { flex: 0 0 auto; color: var(--muted-foreground); transform: rotate(-90deg); transition: transform 120ms ease; }
  .task-progress :global(.task-progress-chevron.expanded) { transform: none; }
  .task-progress-body { display: grid; gap: 0.1rem; padding-bottom: 0.15rem; }
  .task-progress-step { overflow: hidden; color: var(--muted-foreground); text-overflow: ellipsis; white-space: nowrap; font-size: calc(0.68rem * var(--type-scale)); }
  .task-progress-failed { color: var(--destructive); font-size: calc(0.62rem * var(--type-scale)); }
  @media (pointer: coarse) { .task-progress-header { min-height: 2.75rem; } }
</style>
