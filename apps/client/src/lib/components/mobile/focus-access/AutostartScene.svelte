<script lang="ts">
  import Search from "@lucide/svelte/icons/search";
  import { fade } from "svelte/transition";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import GuideAppIcon from "./GuideAppIcon.svelte";
  import GuideHeader from "./GuideHeader.svelte";
  import GuideLine from "./GuideLine.svelte";
  import GuideToggle from "./GuideToggle.svelte";

  /** The manufacturer autostart list: search for the app, then turn its switch on. */
  let { phase, appLabel }: { phase: number; appLabel: string } = $props();

  const { t } = getLocalization();
  const TYPE_MS = 80;
  const characters = $derived([...appLabel]);
  const OTHER_APPS = ["w-20", "w-14", "w-24", "w-16"];
</script>

<GuideHeader title={t("mobile.focusOnboarding.guideAutostart")} />
<div class="mx-3 flex h-7 items-center gap-1.5 rounded-full bg-foreground/8 px-2.5 text-xs">
  <Search class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
  {#if phase === 0}
    <span class="truncate text-muted-foreground">{t("mobile.focusOnboarding.guideSearchApps")}</span>
  {:else}
    <span class="truncate text-foreground">
      {#each characters as character, index (index)}
        <span class="guide-typed" style:animation-delay="{index * TYPE_MS}ms">{character === " " ? " " : character}</span>
      {/each}
    </span>
  {/if}
</div>
<div class="mt-2 flex flex-col">
  {#if phase === 0}
    {#each OTHER_APPS as width (width)}
      <div class="flex items-center gap-2 px-3 py-2">
        <span class="size-6 shrink-0 rounded-full bg-foreground/12"></span>
        <GuideLine class={width} />
        <span class="ml-auto"><GuideToggle on={false} /></span>
      </div>
    {/each}
  {:else}
    <div class="flex items-center gap-2 px-3 py-2" in:fade={{ delay: characters.length * TYPE_MS, duration: 200 }}>
      <GuideAppIcon />
      <span class="min-w-0 truncate text-xs font-medium text-foreground">{appLabel}</span>
      <span class="ml-auto"><GuideToggle on={phase >= 3} tapping={phase === 2} /></span>
    </div>
  {/if}
</div>

<style>
  .guide-typed {
    animation: guide-typed 1ms step-end both;
  }

  @keyframes guide-typed {
    from { opacity: 0; }
    to { opacity: 1; }
  }
</style>
