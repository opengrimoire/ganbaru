<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    MOBILE_ONBOARDING_SCREEN_COUNT,
    onboardingScreenPosition,
    onboardingSegmentFills,
    type MobileOnboardingStage,
  } from "$lib/mobile/onboarding-progress";
  import { cn } from "$lib/utils";

  /**
   * The segmented progress bar at the top of every Android onboarding screen, one segment per screen, with `screen` the
   * current screen's position within `stage`. It owns its column and spacing so the bar stays in the same place as the
   * screens change underneath it.
   *
   * Without `children`, the bar takes its own row above the screen. With `children`, the screen fills the whole height
   * and the bar floats over its top, for screens that center their content on the full screen and already leave room
   * above it. The bar then keeps the background, so content scrolled under it stays hidden.
   */
  let {
    stage,
    screen = 0,
    class: className = "",
    children,
  }: { stage: MobileOnboardingStage; screen?: number; class?: string; children?: Snippet } = $props();

  const { t } = getLocalization();
  const fills = $derived(onboardingSegmentFills(stage, screen));
</script>

{#snippet bar(placement: string)}
  <div class={cn("w-full shrink-0 px-4 pt-4 min-[560px]:px-8", placement, className)}>
    <div class="mx-auto flex w-full max-w-md gap-1" aria-hidden="true">
      {#each fills as filled, index (index)}
        <span class="h-1 flex-1 overflow-hidden rounded-full bg-foreground/10">
          <span
            class="block h-full rounded-full bg-primary transition-[width] duration-300 ease-out motion-reduce:transition-none"
            style:width={filled ? "100%" : "0%"}
          ></span>
        </span>
      {/each}
    </div>
    <p class="sr-only">
      {t("mobile.onboarding.stepProgress", onboardingScreenPosition(stage, screen) + 1, MOBILE_ONBOARDING_SCREEN_COUNT)}
    </p>
  </div>
{/snippet}

{#if children}
  <div class="relative min-h-0 flex-1">
    {@render bar("absolute inset-x-0 top-0 z-10 bg-background")}
    {@render children()}
  </div>
{:else}
  {@render bar("")}
{/if}
