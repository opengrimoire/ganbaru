<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import type { Snippet } from "svelte";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import GuideAppIcon from "./GuideAppIcon.svelte";
  import GuideHeader from "./GuideHeader.svelte";
  import GuideLine from "./GuideLine.svelte";
  import GuideTap from "./GuideTap.svelte";
  import GuideToggle from "./GuideToggle.svelte";

  /**
   * Android's Accessibility settings: open Downloaded services, choose the app's service, turn it on, and allow it in
   * the confirmation sheet.
   */
  let { phase }: { phase: number } = $props();

  const { t } = getLocalization();
  const screen = $derived(phase < 2 ? 0 : phase < 4 ? 1 : 2);
  const confirming = $derived(phase === 6 || phase === 7);
  const service = $derived(t("mobile.focusOnboarding.guideServiceLabel"));

  function screenPosition(index: number): string {
    if (index === screen) return "translate-x-0";
    return index < screen ? "-translate-x-1/3 opacity-0" : "translate-x-full";
  }
</script>

{#snippet layer(index: number, content: Snippet)}
  <div
    class={cn(
      "absolute inset-0 bg-background transition-[translate,opacity] duration-400 ease-out motion-reduce:transition-none",
      screenPosition(index),
    )}
  >
    {@render content()}
  </div>
{/snippet}

{#snippet accessibility()}
  <GuideHeader title={t("mobile.focusOnboarding.guideAccessibility")} />
  <div class="flex flex-col">
    <div class="flex flex-col gap-1.5 px-3 py-2.5"><GuideLine class="w-24" /><GuideLine class="w-32" /></div>
    <div class="flex flex-col gap-1.5 px-3 py-2.5"><GuideLine class="w-28" /><GuideLine class="w-20" /></div>
    <div class="relative flex items-center gap-2 px-3 py-2.5">
      <span class="min-w-0 flex-1 truncate text-xs font-medium text-foreground">{t("mobile.focusOnboarding.guideDownloadedServices")}</span>
      <ChevronRight class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
      <GuideTap active={phase === 1} />
    </div>
  </div>
{/snippet}

{#snippet services()}
  <GuideHeader title={t("mobile.focusOnboarding.guideDownloadedServices")} />
  <div class="flex flex-col">
    <div class="flex items-center gap-2 px-3 py-2">
      <span class="size-6 shrink-0 rounded-full bg-foreground/12"></span>
      <GuideLine class="w-20" />
    </div>
    <div class="relative flex items-center gap-2 px-3 py-2">
      <GuideAppIcon />
      <span class="min-w-0 flex-1 truncate text-xs font-medium text-foreground">{service}</span>
      <span class="shrink-0 text-[0.625rem] text-muted-foreground">{t("mobile.focusOnboarding.guideOff")}</span>
      <GuideTap active={phase === 3} />
    </div>
  </div>
{/snippet}

{#snippet detail()}
  <GuideHeader title={service} />
  <div class="mx-3 flex items-center gap-2 py-1">
    <GuideLine class="w-24" />
    <span class="ml-auto"><GuideToggle on={phase >= 8} tapping={phase === 5} /></span>
  </div>
  <div class="flex flex-col gap-1.5 px-3 pt-3">
    <GuideLine class="w-36" />
    <GuideLine class="w-28" />
  </div>
{/snippet}

{@render layer(0, accessibility)}
{@render layer(1, services)}
{@render layer(2, detail)}

<div
  class={cn(
    "absolute inset-x-0 bottom-0 flex flex-col items-center gap-3 rounded-t-2xl border-t border-border bg-card px-3 pb-3 pt-4 transition-transform duration-400 ease-out motion-reduce:transition-none",
    confirming ? "translate-y-0" : "translate-y-full",
  )}
>
  <GuideAppIcon class="size-7" />
  <div class="flex w-full flex-col gap-1.5">
    <GuideLine class="w-full" />
    <GuideLine class="w-32" />
    <GuideLine class="w-full" />
    <GuideLine class="w-24" />
  </div>
  <div class="grid w-full grid-cols-2 gap-2 text-xs font-medium">
    <span class="flex h-7 items-center justify-center rounded-full bg-foreground/8 text-foreground">{t("mobile.focusOnboarding.guideDeny")}</span>
    <span class="relative flex h-7 items-center justify-center rounded-full bg-foreground/8 text-primary">
      {t("mobile.focusOnboarding.guideAllow")}
      <GuideTap active={phase === 7} />
    </span>
  </div>
</div>
