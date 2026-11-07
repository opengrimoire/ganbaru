<script lang="ts">
  import ArrowBigDown from "@lucide/svelte/icons/arrow-big-down";
  import ArrowBigUp from "@lucide/svelte/icons/arrow-big-up";
  import type { BlockerApp } from "$lib/vault/welcome-blocker";
  import WelcomeParodyIcon, { PARODY_COLORS } from "./WelcomeParodyIcon.svelte";

  /**
   * A parody app's feed in the distraction blocker's welcome scenes, drawn with placeholder lines: Y's posts, Bluedit's
   * voted threads, or Egogram's stories and photos. It scrolls by `scroll`, from 0 to 1, over `travel` pixels.
   */
  let { app, scroll, travel, topInset = 0 }: { app: BlockerApp; scroll: number; travel: number; topInset?: number } = $props();

  /** The widths of the posts' placeholder lines, as fractions of their room. */
  const LINE_WIDTHS = [0.9, 0.7, 0.82, 0.55, 0.76, 0.64];
  const POSTS = [0, 1, 2, 3, 4, 5, 6];
</script>

{#snippet line(fraction: number, tone = "bg-foreground/12")}
  <span class="block h-1.5 shrink-0 rounded-full {tone}" style:width={`${fraction * 100}%`}></span>
{/snippet}

<div class="flex h-full flex-col overflow-hidden bg-background text-foreground" style:padding-top={`${topInset}px`}>
  <div class="relative z-1 flex h-8 shrink-0 items-center justify-center border-b border-border bg-background">
    <WelcomeParodyIcon {app} size={16} />
  </div>
  <div class="flex flex-col" style:transform={`translateY(${-scroll * travel}px)`}>
    {#if app === "egogram"}
      <div class="flex shrink-0 gap-2 px-2.5 py-2">
        {#each [0, 1, 2, 3, 4] as index (index)}
          <span class="size-7 shrink-0 rounded-full border-2 bg-foreground/10" style:border-color={PARODY_COLORS.egogram}></span>
        {/each}
      </div>
      {#each [0, 1, 2] as index (index)}
        <div class="flex items-center gap-1.5 px-2.5 py-1.5">
          <span class="size-4 rounded-full bg-foreground/15"></span>
          {@render line(0.3, "bg-foreground/20")}
        </div>
        <span class="block aspect-square w-full shrink-0 bg-foreground/8"></span>
        <div class="flex flex-col gap-1.5 px-2.5 py-2">{@render line(LINE_WIDTHS[index] * 0.6, "bg-foreground/20")}{@render line(LINE_WIDTHS[index + 2])}</div>
      {/each}
    {:else if app === "bluedit"}
      {#each POSTS as index (index)}
        <div class="flex gap-2 border-b border-border/60 px-3 py-2.5">
          <span class="flex shrink-0 flex-col items-center text-muted-foreground">
            <span class="flex" style:color={index % 2 === 0 ? PARODY_COLORS.bluedit : undefined}>
              <ArrowBigUp size={12} strokeWidth={1.8} />
            </span>
            <span class="my-0.5 h-1 w-2.5 rounded-full bg-foreground/15"></span>
            <ArrowBigDown size={12} strokeWidth={1.8} />
          </span>
          <div class="flex min-w-0 flex-1 flex-col gap-1.5">
            {@render line(0.3, "bg-foreground/10")}
            {@render line(LINE_WIDTHS[index % LINE_WIDTHS.length], "bg-foreground/25")}
            {@render line(LINE_WIDTHS[(index + 3) % LINE_WIDTHS.length] * 0.8)}
            {#if index % 3 === 1}<span class="mt-0.5 block h-14 rounded-md bg-foreground/8"></span>{/if}
          </div>
        </div>
      {/each}
    {:else}
      {#each POSTS as index (index)}
        <div class="flex gap-2 border-b border-border/60 px-3 py-2.5">
          <span class="size-6 shrink-0 rounded-full bg-foreground/12"></span>
          <div class="flex min-w-0 flex-1 flex-col gap-1.5">
            <span class="flex gap-1.5">
              <span class="h-1.5 w-12 rounded-full bg-foreground/25"></span>
              <span class="h-1.5 w-8 rounded-full bg-foreground/10"></span>
            </span>
            {@render line(LINE_WIDTHS[index % LINE_WIDTHS.length])}
            {@render line(LINE_WIDTHS[(index + 3) % LINE_WIDTHS.length] * 0.7)}
            {#if index % 3 === 1}<span class="mt-0.5 block h-14 rounded-md bg-foreground/8"></span>{/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>
