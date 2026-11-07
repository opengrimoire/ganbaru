<script lang="ts">
  import Bell from "@lucide/svelte/icons/bell";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import ChartNoAxesColumn from "@lucide/svelte/icons/chart-no-axes-column";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Feather from "@lucide/svelte/icons/feather";
  import Heart from "@lucide/svelte/icons/heart";
  import House from "@lucide/svelte/icons/house";
  import Image from "@lucide/svelte/icons/image";
  import Mail from "@lucide/svelte/icons/mail";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import Repeat2 from "@lucide/svelte/icons/repeat-2";
  import Search from "@lucide/svelte/icons/search";
  import Share from "@lucide/svelte/icons/share";
  import Smile from "@lucide/svelte/icons/smile";
  import UserRound from "@lucide/svelte/icons/user-round";
  import type { Component } from "svelte";
  import WelcomeParodyIcon from "./WelcomeParodyIcon.svelte";

  /**
   * Y's website in the distraction blocker's welcome scenes, drawn with placeholder lines in the layout of a well-known
   * microblogging site: a navigation rail, the home timeline with its tabs, composer, and posts, and a side column with
   * search and trends. The timeline scrolls by `scroll`, from 0 to 1, over `travel` pixels, while both side columns
   * stay. The rail shows its labels and the side column shows only when the browser has room for them.
   */
  let { scroll, travel }: { scroll: number; travel: number } = $props();

  /** The navigation rail's destinations, each with its label's width as a fraction of the room; the first is selected. */
  const NAV: readonly { icon: Component; label: number }[] = [
    { icon: House, label: 0.55 },
    { icon: Search, label: 0.75 },
    { icon: Bell, label: 0.95 },
    { icon: Mail, label: 0.6 },
    { icon: Bookmark, label: 0.7 },
    { icon: UserRound, label: 0.5 },
  ];
  /** The composer's attachment tools. */
  const COMPOSER_TOOLS: readonly Component[] = [Image, Smile, CalendarClock, MapPin];
  /**
   * The timeline's posts: the author's name and handle widths in pixels, the text's line widths as fractions of the
   * room, and whether a photo follows.
   */
  const POSTS: readonly { name: number; handle: number; lines: readonly number[]; media: boolean }[] = [
    { name: 26, handle: 34, lines: [0.94, 0.42], media: false },
    { name: 18, handle: 40, lines: [0.88, 0.7], media: true },
    { name: 32, handle: 28, lines: [0.6], media: false },
    { name: 22, handle: 36, lines: [0.9, 0.84, 0.3], media: false },
    { name: 28, handle: 30, lines: [0.76], media: true },
  ];
  /** The trends' category and topic widths, as fractions of the card's room. */
  const TRENDS: readonly { category: number; topic: number }[] = [
    { category: 0.7, topic: 0.5 },
    { category: 0.55, topic: 0.72 },
    { category: 0.75, topic: 0.4 },
  ];
  /** The post actions that carry a count: replies, reposts, likes, and views. */
  const POST_ACTIONS: readonly Component[] = [MessageCircle, Repeat2, Heart, ChartNoAxesColumn];
</script>

{#snippet bar(width: string, tone = "bg-foreground/12")}
  <span class="block h-1.5 shrink-0 rounded-full {tone}" style:width></span>
{/snippet}

<div class="@container flex h-full overflow-hidden bg-background text-foreground" aria-hidden="true">
  <!-- The navigation rail: the logo, the destinations with Home selected, and the post button, icons only when narrow. -->
  <nav class="flex w-8 shrink-0 flex-col items-center gap-2.5 pt-2.5 @lg:w-22 @lg:items-stretch @lg:pl-3">
    <span class="mb-0.5 flex"><WelcomeParodyIcon app="y" size={14} /></span>
    {#each NAV as item, index (index)}
      <span class="flex items-center gap-2">
        <item.icon size={12} strokeWidth={index === 0 ? 2.6 : 1.9} class="shrink-0" />
        <span class="hidden flex-1 @lg:block">
          {@render bar(`${item.label * 100}%`, index === 0 ? "bg-foreground/45" : "bg-foreground/20")}
        </span>
      </span>
    {/each}
    <span class="mt-0.5 grid size-5.5 place-items-center rounded-full bg-foreground text-background @lg:hidden">
      <Feather size={10} strokeWidth={2.2} />
    </span>
    <span class="mt-0.5 mr-2.5 hidden h-5.5 items-center justify-center rounded-full bg-foreground @lg:flex">
      {@render bar("40%", "bg-background/70")}
    </span>
  </nav>

  <!-- The home timeline: its two tabs with the first selected, then the composer and the posts, which scroll. -->
  <div class="flex min-w-0 flex-1 flex-col border-x border-border">
    <div class="grid h-7 shrink-0 grid-cols-2 border-b border-border">
      <span class="relative grid place-items-center">
        {@render bar("2.25rem", "bg-foreground/50")}
        <span class="absolute bottom-0 left-1/2 h-0.5 w-8 -translate-x-1/2 rounded-full bg-foreground"></span>
      </span>
      <span class="grid place-items-center">{@render bar("2.75rem", "bg-foreground/15")}</span>
    </div>
    <div class="min-h-0 flex-1 overflow-hidden">
      <div class="flex flex-col" style:transform={`translateY(${-scroll * travel}px)`}>
        <div class="flex gap-2 border-b border-border px-2.5 pt-2.5 pb-2">
          <span class="size-5 shrink-0 rounded-full bg-foreground/15"></span>
          <div class="flex min-w-0 flex-1 flex-col gap-2.5 pt-1">
            {@render bar("55%", "bg-foreground/10")}
            <div class="flex items-center gap-2 text-muted-foreground">
              {#each COMPOSER_TOOLS as Tool, index (index)}
                <Tool size={9} strokeWidth={2} class="shrink-0" />
              {/each}
              <span class="ml-auto grid h-4.5 w-10 shrink-0 place-items-center rounded-full bg-foreground/25">
                {@render bar("50%", "bg-background/70")}
              </span>
            </div>
          </div>
        </div>
        {#each POSTS as post, index (index)}
          <div class="flex gap-2 border-b border-border px-2.5 py-2">
            <span class="size-5 shrink-0 rounded-full bg-foreground/15"></span>
            <div class="flex min-w-0 flex-1 flex-col gap-1.5">
              <span class="flex items-center gap-1.5">
                {@render bar(`${post.name}px`, "bg-foreground/40")}
                {@render bar(`${post.handle}px`, "bg-foreground/12")}
                <Ellipsis size={9} class="ml-auto shrink-0 text-muted-foreground" />
              </span>
              {#each post.lines as fraction, line (line)}
                {@render bar(`${fraction * 100}%`, "bg-foreground/20")}
              {/each}
              {#if post.media}
                <span class="mt-0.5 block aspect-video w-full rounded-lg border border-border bg-foreground/8"></span>
              {/if}
              <span class="mt-0.5 flex items-center gap-1 text-muted-foreground">
                {#each POST_ACTIONS as Action, action (action)}
                  <span class="flex flex-1 items-center gap-1">
                    <Action size={9} strokeWidth={2} class="shrink-0" />
                    {@render bar("0.5rem", "bg-foreground/10")}
                  </span>
                {/each}
                <Bookmark size={9} strokeWidth={2} class="shrink-0" />
                <Share size={9} strokeWidth={2} class="shrink-0" />
              </span>
            </div>
          </div>
        {/each}
      </div>
    </div>
  </div>

  <!-- The side column: the search field and a card of trends, shown only when the timeline keeps enough room. -->
  <div class="hidden w-28 shrink-0 flex-col gap-2 px-2 pt-2 @md:flex">
    <span class="flex h-5.5 shrink-0 items-center gap-1.5 rounded-full border border-border px-2 text-muted-foreground">
      <Search size={9} strokeWidth={2} class="shrink-0" />
      {@render bar("45%", "bg-foreground/10")}
    </span>
    <div class="flex flex-col gap-2.5 rounded-xl border border-border px-2 py-2">
      {@render bar("75%", "bg-foreground/45")}
      {#each TRENDS as trend, index (index)}
        <span class="flex items-start gap-1">
          <span class="flex min-w-0 flex-1 flex-col gap-1">
            {@render bar(`${trend.category * 100}%`, "bg-foreground/10")}
            {@render bar(`${trend.topic * 100}%`, "bg-foreground/35")}
          </span>
          <Ellipsis size={8} class="shrink-0 text-muted-foreground" />
        </span>
      {/each}
    </div>
  </div>
</div>
