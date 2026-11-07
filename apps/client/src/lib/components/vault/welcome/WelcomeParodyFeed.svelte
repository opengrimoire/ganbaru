<script lang="ts">
  import ArrowBigDown from "@lucide/svelte/icons/arrow-big-down";
  import ArrowBigUp from "@lucide/svelte/icons/arrow-big-up";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Heart from "@lucide/svelte/icons/heart";
  import House from "@lucide/svelte/icons/house";
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import Send from "@lucide/svelte/icons/send";
  import SquarePlay from "@lucide/svelte/icons/square-play";
  import { BLOCKER_APP_NAMES, type BlockerApp } from "$lib/vault/welcome-blocker";
  import WelcomeParodyIcon, { EGOGRAM_GRADIENT, PARODY_COLORS } from "./WelcomeParodyIcon.svelte";
  import WelcomeParodyBlueditSite from "./WelcomeParodyBlueditSite.svelte";
  import WelcomeParodyYSite from "./WelcomeParodyYSite.svelte";

  /**
   * A parody app's feed in the distraction blocker's welcome scenes, drawn with placeholder lines: Y's and Bluedit's
   * websites in a browser, or on a phone, Y's posts, Bluedit's voted threads, or Egogram's stories and photos. It scrolls
   * by `scroll`, from 0 to 1, over `travel` pixels. On a phone, an app draws its own bars, as its mobile app does,
   * instead of a website's header.
   */
  let {
    app,
    scroll,
    travel,
    topInset = 0,
    phone = false,
  }: { app: BlockerApp; scroll: number; travel: number; topInset?: number; phone?: boolean } = $props();

  /** The widths of the posts' placeholder lines, as fractions of their room. */
  const LINE_WIDTHS = [0.9, 0.7, 0.82, 0.55, 0.76, 0.64];
  const POSTS = [0, 1, 2, 3, 4, 5, 6];
  /** Egogram's stories after your own: whether each is already seen, which greys its ring. */
  const STORIES_SEEN = [false, false, true, true, true];
  /** Egogram's posts: each author's name width, as a fraction of the row, and the photo's height against its width. */
  const EGOGRAM_POSTS: readonly { author: number; ratio: number }[] = [
    { author: 0.34, ratio: 1.25 },
    { author: 0.26, ratio: 1 },
    { author: 0.4, ratio: 1.25 },
  ];

  const phoneEgogram = $derived(phone && app === "egogram");
</script>

{#snippet line(fraction: number, tone = "bg-foreground/12")}
  <span class="block h-1.5 shrink-0 rounded-full {tone}" style:width={`${fraction * 100}%`}></span>
{/snippet}

{#snippet storyRing(seen: boolean, size: string)}
  <span class="block shrink-0 rounded-full p-0.5 {size} {seen ? 'bg-foreground/20' : ''}" style:background={seen ? undefined : EGOGRAM_GRADIENT}>
    <span class="block size-full rounded-full border-2 border-background bg-foreground/10"></span>
  </span>
{/snippet}

{#if app === "y" && !phone}
  <WelcomeParodyYSite {scroll} {travel} />
{:else if app === "bluedit" && !phone}
  <WelcomeParodyBlueditSite {scroll} {travel} />
{:else}
  <div class="flex h-full flex-col overflow-hidden bg-background text-foreground" style:padding-top={`${topInset}px`}>
    {#if phoneEgogram}
      <!-- Egogram's app bar: a new post on the left, its wordmark with the feed picker, and the activity heart. -->
      <div class="relative flex h-7 shrink-0 items-center justify-between bg-background px-2.5">
        <Plus size={14} strokeWidth={2} aria-hidden="true" />
        <span class="absolute left-1/2 flex -translate-x-1/2 items-center gap-0.5">
          <span class="text-xs leading-none tracking-tight italic">{BLOCKER_APP_NAMES.egogram}</span>
          <ChevronDown size={9} strokeWidth={2.4} aria-hidden="true" />
        </span>
        <Heart size={14} strokeWidth={2} aria-hidden="true" />
      </div>
    {:else}
      <div class="flex h-8 shrink-0 items-center justify-center border-b border-border bg-background">
        <WelcomeParodyIcon {app} size={16} />
      </div>
    {/if}
    <!-- The feed scrolls within its own clip, so it never shows above the header, in the phone's status bar. -->
    <div class="min-h-0 flex-1 overflow-hidden">
      <div class="flex flex-col" style:transform={`translateY(${-scroll * travel}px)`}>
        {#if phoneEgogram}
          <!-- Your own story with its add badge, then the followed accounts' stories, the unseen ones ringed in color. -->
          <div class="flex shrink-0 gap-2.5 overflow-hidden px-2 pt-1.5 pb-2">
            <span class="flex w-9 shrink-0 flex-col items-center gap-1">
              <span class="relative block size-9 rounded-full bg-foreground/15">
                <span class="absolute -right-0.5 -bottom-0.5 grid size-3.5 place-items-center rounded-full bg-foreground text-background ring-2 ring-background">
                  <Plus size={8} strokeWidth={3} aria-hidden="true" />
                </span>
              </span>
              <span class="h-1 w-7 rounded-full bg-foreground/25"></span>
            </span>
            {#each STORIES_SEEN as seen, index (index)}
              <span class="flex w-9 shrink-0 flex-col items-center gap-1">
                {@render storyRing(seen, "size-9")}
                <span class="h-1 rounded-full {seen ? 'bg-foreground/12' : 'bg-foreground/25'}" style:width={`${LINE_WIDTHS[index] * 100}%`}></span>
              </span>
            {/each}
          </div>
          {#each EGOGRAM_POSTS as post, index (index)}
            <div class="flex items-center gap-1.5 px-2 py-1.5">
              {@render storyRing(index === 1, "size-5")}
              {@render line(post.author, "bg-foreground/25")}
              <span class="ml-auto flex flex-col gap-0.75" aria-hidden="true">
                <span class="h-0.5 w-3 rounded-full bg-foreground/70"></span>
                <span class="h-0.5 w-2 rounded-full bg-foreground/70"></span>
              </span>
            </div>
            <span class="block w-full shrink-0 bg-foreground/8" style:aspect-ratio={`1 / ${post.ratio}`}></span>
            <div class="flex items-center gap-2.5 px-2 pt-1.5">
              <Heart size={12} strokeWidth={2} aria-hidden="true" />
              <MessageCircle size={12} strokeWidth={2} aria-hidden="true" />
              <Send size={12} strokeWidth={2} aria-hidden="true" />
              <span class="flex-1"></span>
              <Bookmark size={12} strokeWidth={2} aria-hidden="true" />
            </div>
            <div class="flex flex-col gap-1.5 px-2 pt-1.5 pb-2.5">
              {@render line(LINE_WIDTHS[index] * 0.4, "bg-foreground/20")}{@render line(LINE_WIDTHS[index + 2])}
            </div>
          {/each}
        {:else if app === "egogram"}
          <div class="flex shrink-0 gap-2 px-2.5 py-2">
            {#each [0, 1, 2, 3, 4] as index (index)}
              <span class="size-7 shrink-0 rounded-full p-0.5" style:background={EGOGRAM_GRADIENT}>
                <span class="block size-full rounded-full border-2 border-background bg-foreground/10"></span>
              </span>
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
    {#if phoneEgogram}
      <!-- Egogram's tab bar, on its home feed: five equal tabs for home, short videos, messages, search, and your profile,
           the selected one in a pill that nearly fills its tab. -->
      <div class="grid h-7 shrink-0 grid-cols-5 items-center border-t border-border bg-background px-0.5" aria-hidden="true">
        <span class="mx-0.5 grid h-5.5 place-items-center rounded-full bg-foreground/10"><House size={12} strokeWidth={2.2} /></span>
        <span class="grid place-items-center"><SquarePlay size={12} strokeWidth={2} /></span>
        <span class="grid place-items-center"><Send size={12} strokeWidth={2} /></span>
        <span class="grid place-items-center"><Search size={12} strokeWidth={2} /></span>
        <span class="grid place-items-center"><span class="size-3.5 rounded-full bg-foreground/20"></span></span>
      </div>
    {/if}
  </div>
{/if}
