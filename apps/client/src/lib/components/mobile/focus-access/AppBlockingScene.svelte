<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
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
  <div class="flex flex-col gap-3 px-3 pb-2 pt-3">
    <ArrowLeft class="size-3.5 text-muted-foreground" aria-hidden="true" />
    <div class="flex items-center gap-2.5">
      <GuideLine class="h-2 w-11 bg-foreground/30" />
      {#each ["w-9", "w-11", "w-12"] as tabWidth (tabWidth)}
        <GuideLine class={cn("h-2", tabWidth)} />
      {/each}
    </div>
  </div>
  <div class="mx-3 flex flex-col divide-y divide-border">
    <div class="flex items-center gap-2 py-2.5">
      <GuideLine class="w-20 bg-foreground/25" />
      <GuideLine class="ml-auto w-14" />
      <ChevronRight class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
    </div>
    <div class="flex items-center gap-2 py-2.5">
      <div class="flex flex-1 flex-col gap-1.5">
        <GuideLine class="w-24 bg-foreground/25" />
        <GuideLine class="w-full bg-foreground/8" />
        <GuideLine class="w-12 bg-foreground/8" />
      </div>
      <GuideLine class="w-5" />
      <ChevronRight class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
    </div>
    <div class="relative flex items-center gap-2 py-2.5">
      <span class="min-w-0 flex-1 truncate text-xs font-medium text-foreground">{t("mobile.focusOnboarding.guideDownloadedServices")}</span>
      <ChevronRight class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
      <GuideTap active={phase === 1} />
    </div>
  </div>
{/snippet}

{#snippet services()}
  <GuideHeader title={t("mobile.focusOnboarding.guideDownloadedServices")} />
  <div class="relative flex items-center gap-2 px-3 py-2.5">
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <span class="truncate text-xs font-medium leading-tight text-foreground">{service}</span>
      <GuideLine class="w-20 bg-foreground/8" />
    </div>
    <span class="shrink-0 text-[0.625rem] text-muted-foreground">{t("mobile.focusOnboarding.guideOff")}</span>
    <ChevronRight class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
    <GuideTap active={phase === 3} />
  </div>
{/snippet}

{#snippet detail()}
  <GuideHeader title={service} />
  <div class="mx-3 flex items-center gap-2 py-1">
    <span class="min-w-0 flex-1 truncate text-xs font-medium text-foreground">{t("mobile.focusOnboarding.guideAccessibility")}</span>
    <GuideToggle on={phase >= 8} tapping={phase === 5} />
  </div>
  <div class="flex flex-col gap-1.5 px-3 pt-3">
    <GuideLine class="w-full bg-foreground/8" />
    <GuideLine class="w-28 bg-foreground/8" />
  </div>
{/snippet}

{@render layer(0, accessibility)}
{@render layer(1, services)}
{@render layer(2, detail)}

<div
  class={cn(
    "absolute inset-0 bg-black/50 transition-opacity duration-400 ease-out motion-reduce:transition-none",
    confirming ? "opacity-100" : "opacity-0",
  )}
></div>
<!-- Android's confirmation sheet: the service's icon, a title, a paragraph, the two kinds of access it grants, then the
     buttons. -->
<div
  class={cn(
    "absolute inset-x-0 bottom-0 top-10 flex flex-col rounded-t-2xl bg-card px-3 pb-3 pt-4 transition-transform duration-400 ease-out motion-reduce:transition-none",
    confirming ? "translate-y-0" : "translate-y-full",
  )}
>
  <div class="flex flex-col items-center gap-2">
    <GuideAppIcon class="size-6" />
    <GuideLine class="w-32 bg-foreground/25" />
    <GuideLine class="w-12 bg-foreground/25" />
  </div>
  <div class="mt-3 flex flex-col gap-1">
    <GuideLine class="w-full" />
    <GuideLine class="w-full" />
    <GuideLine class="w-16" />
  </div>
  <div class="mt-3 flex flex-col gap-2.5">
    {#each [["w-24", ["w-full", "w-20"]], ["w-28", ["w-full", "w-full", "w-14"]]] as const as [titleWidth, lineWidths] (titleWidth)}
      <div class="flex items-start gap-2">
        <span class="size-4 shrink-0 rounded-md bg-foreground/10"></span>
        <div class="flex flex-1 flex-col gap-1 pt-0.5">
          <GuideLine class={cn("bg-foreground/25", titleWidth)} />
          {#each lineWidths as lineWidth, index (index)}
            <GuideLine class={cn("bg-foreground/8", lineWidth)} />
          {/each}
        </div>
      </div>
    {/each}
  </div>
  <div class="mt-auto grid w-full grid-cols-2 gap-2 text-xs font-medium">
    <span class="flex h-7 items-center justify-center rounded-full bg-foreground/8 text-foreground">{t("mobile.focusOnboarding.guideDeny")}</span>
    <span class="relative flex h-7 items-center justify-center rounded-full bg-foreground/8 text-primary">
      {t("mobile.focusOnboarding.guideAllow")}
      <GuideTap active={phase === 7} />
    </span>
  </div>
</div>
