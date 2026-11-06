<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import GuideLine from "./GuideLine.svelte";
  import GuideTap from "./GuideTap.svelte";

  /** The app's battery saver choices: move the selection to No restrictions. */
  let { phase, appLabel }: { phase: number; appLabel: string } = $props();

  const { t } = getLocalization();
  const chosen = $derived(phase >= 2);
</script>

{#snippet option(selected: boolean, label: string | null, widths: readonly string[], tapping = false)}
  <div
    class={cn(
      "relative flex items-start gap-1.5 rounded-xl px-2.5 py-2 transition-colors duration-300 motion-reduce:transition-none",
      selected ? "bg-primary/15 text-primary" : "bg-foreground/6 text-foreground",
    )}
  >
    <Check class={cn("mt-0.5 size-3 shrink-0 transition-opacity duration-300", selected ? "opacity-100" : "opacity-0")} aria-hidden="true" />
    <div class="flex min-w-0 flex-1 flex-col gap-1.5 py-0.5">
      {#if label}<span class="text-xs font-semibold leading-tight">{label}</span>{/if}
      {#each widths as width (width)}
        <GuideLine class={cn(width, selected && "bg-primary/35")} />
      {/each}
    </div>
    <GuideTap active={tapping} />
  </div>
{/snippet}

<div class="flex flex-col px-3 pt-3">
  <ArrowLeft class="size-3.5 text-muted-foreground" aria-hidden="true" />
  <div class="mt-2.5 truncate text-lg font-light leading-tight text-foreground">{appLabel}</div>
  <div class="mb-2 mt-3 truncate text-[0.625rem] uppercase tracking-wide text-muted-foreground">
    {t("mobile.focusOnboarding.guideBackgroundSettings")}
  </div>
  <div class="flex flex-col gap-1.5">
    {@render option(chosen, t("mobile.focusOnboarding.guideNoRestrictions"), ["w-28"], phase === 1)}
    {@render option(!chosen, null, ["w-24", "w-30", "w-20"])}
    {@render option(false, null, ["w-26", "w-20"])}
    {@render option(false, null, ["w-28", "w-16"])}
  </div>
</div>
