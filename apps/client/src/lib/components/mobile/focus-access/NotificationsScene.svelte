<script lang="ts">
  import { fly } from "svelte/transition";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { prefersReducedMotion } from "$lib/utils/smooth-swap";
  import GuideAppIcon from "./GuideAppIcon.svelte";
  import GuideLine from "./GuideLine.svelte";

  /** The notification shade once notifications are allowed: the running timer, then a break alert. */
  let { phase, appLabel }: { phase: number; appLabel: string } = $props();

  const { t } = getLocalization();
  const duration = prefersReducedMotion() ? 0 : 400;
</script>

{#snippet card(title: string, body: string, progress: boolean)}
  <div class="flex flex-col gap-1.5 rounded-2xl bg-card px-3 py-2.5" in:fly|global={{ y: -16, duration }}>
    <div class="flex items-center gap-1.5">
      <GuideAppIcon class="size-3.5" />
      <span class="min-w-0 truncate text-[0.625rem] text-muted-foreground">{appLabel}</span>
    </div>
    <span class="text-xs font-semibold leading-tight text-foreground">{title}</span>
    <span class="text-xs leading-tight text-muted-foreground">{body}</span>
    {#if progress}
      <span class="mt-1 block h-1 overflow-hidden rounded-full bg-foreground/10">
        <span class="guide-progress block h-full rounded-full bg-primary"></span>
      </span>
    {/if}
  </div>
{/snippet}

<div class="flex h-full flex-col gap-2 bg-foreground/4 px-2.5 pt-4">
  <div class="flex items-center justify-between px-1 pb-1">
    <GuideLine class="w-10" />
    <GuideLine class="w-8" />
  </div>
  {#if phase >= 2}
    {@render card(t("mobile.focusOnboarding.guideBreak"), t("mobile.focusOnboarding.guideBreakBody"), false)}
  {/if}
  {#if phase >= 1}
    {@render card(t("mobile.focusOnboarding.guideFocus"), t("mobile.focusOnboarding.guideFocusLeft"), true)}
  {/if}
</div>

<style>
  .guide-progress {
    width: 28%;
    animation: guide-progress 4.6s linear both;
  }

  @keyframes guide-progress {
    to { width: 40%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .guide-progress { animation: none; }
  }
</style>
