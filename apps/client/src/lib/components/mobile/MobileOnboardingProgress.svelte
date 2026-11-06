<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    MOBILE_ONBOARDING_STAGES,
    onboardingSegmentFills,
    onboardingStageNumber,
    type MobileOnboardingStage,
  } from "$lib/mobile/onboarding-progress";
  import { cn } from "$lib/utils";

  /**
   * The segmented progress bar at the top of every Android onboarding screen, one segment per stage. It owns its
   * column and spacing so the bar stays in the same place as the screens change underneath it.
   */
  let {
    stage,
    fraction = 1,
    class: className = "",
  }: { stage: MobileOnboardingStage; fraction?: number; class?: string } = $props();

  const { t } = getLocalization();
  const fills = $derived(onboardingSegmentFills(stage, fraction));
</script>

<div class={cn("w-full shrink-0 px-4 pt-4 min-[560px]:px-8", className)}>
  <div class="mx-auto flex w-full max-w-md gap-1.5" aria-hidden="true">
    {#each fills as fill, index (MOBILE_ONBOARDING_STAGES[index])}
      <span class="h-1 flex-1 overflow-hidden rounded-full bg-foreground/10">
        <span
          class="block h-full rounded-full bg-primary transition-[width] duration-300 ease-out motion-reduce:transition-none"
          style:width={`${fill * 100}%`}
        ></span>
      </span>
    {/each}
  </div>
  <p class="sr-only">
    {t("mobile.onboarding.stepProgress", onboardingStageNumber(stage), MOBILE_ONBOARDING_STAGES.length)}
  </p>
</div>
