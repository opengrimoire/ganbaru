<script lang="ts">
  import ArrowBigDown from "@lucide/svelte/icons/arrow-big-down";
  import ArrowBigUp from "@lucide/svelte/icons/arrow-big-up";
  import Bell from "@lucide/svelte/icons/bell";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import CircleArrowOutUpRight from "@lucide/svelte/icons/circle-arrow-out-up-right";
  import Compass from "@lucide/svelte/icons/compass";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Forward from "@lucide/svelte/icons/forward";
  import House from "@lucide/svelte/icons/house";
  import Menu from "@lucide/svelte/icons/menu";
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import MessageCircleMore from "@lucide/svelte/icons/message-circle-more";
  import Newspaper from "@lucide/svelte/icons/newspaper";
  import Plus from "@lucide/svelte/icons/plus";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import SquarePlus from "@lucide/svelte/icons/square-plus";
  import Star from "@lucide/svelte/icons/star";
  import type { Component } from "svelte";
  import { BLOCKER_APP_NAMES } from "$lib/vault/welcome-blocker";
  import WelcomeParodyIcon, { PARODY_COLORS } from "./WelcomeParodyIcon.svelte";

  /**
   * Bluedit's website in the distraction blocker's welcome scenes, drawn with placeholder lines in the layout of a
   * well-known forum site: a header with search, a side bar of feeds and communities, and the home feed of voted posts.
   * The feed scrolls by `scroll`, from 0 to 1, over `travel` pixels, while the header and the side bar stay. The side
   * bar shows only when the browser has room for it, and a menu button stands in for it otherwise.
   */
  let { scroll, travel }: { scroll: number; travel: number } = $props();

  /** The side bar's feeds, each with its label's width as a fraction of the room; the first, the home feed, is selected. */
  const FEEDS: readonly { icon: Component; label: number }[] = [
    { icon: House, label: 0.6 },
    { icon: CircleArrowOutUpRight, label: 0.55 },
    { icon: Newspaper, label: 0.5 },
    { icon: Compass, label: 0.58 },
    { icon: Plus, label: 0.85 },
  ];
  /** The side bar's collapsed sections, as fractions of the room their headings take. */
  const SECTIONS = [0.7, 0.5];
  /** The joined communities' names, as fractions of the room. */
  const COMMUNITIES = [0.75, 0.55, 0.68, 0.48];
  /**
   * The feed's posts: the community's and the age's widths in pixels, the title's line widths as fractions of the room,
   * whether media follows, and whether you upvoted it.
   */
  const POSTS: readonly { community: number; age: number; title: readonly number[]; media: boolean; upvoted: boolean }[] = [
    { community: 26, age: 16, title: [0.92, 0.5], media: true, upvoted: true },
    { community: 34, age: 20, title: [0.8], media: true, upvoted: false },
    { community: 22, age: 14, title: [0.88, 0.62], media: false, upvoted: false },
    { community: 30, age: 18, title: [0.7], media: true, upvoted: true },
  ];
</script>

{#snippet bar(width: string, tone = "bg-foreground/12", height = "h-1.5")}
  <span class="block shrink-0 rounded-full {height} {tone}" style:width></span>
{/snippet}

{#snippet pill(Icon: Component, count: string)}
  <span class="flex h-4.5 items-center gap-1 rounded-full bg-foreground/8 px-1.5">
    <Icon size={9} strokeWidth={2} class="shrink-0" />
    {@render bar(count, "bg-foreground/25")}
  </span>
{/snippet}

<div class="@container flex h-full flex-col overflow-hidden bg-background text-foreground" aria-hidden="true">
  <!-- The header: the logo and wordmark, the search field, and chat, create, notifications, and your avatar. -->
  <div class="flex h-8 shrink-0 items-center gap-2 border-b border-border px-2.5">
    <Menu size={11} strokeWidth={2} class="shrink-0 @md:hidden" />
    <span class="shrink-0 text-[11px] leading-none font-bold tracking-tight">{BLOCKER_APP_NAMES.bluedit.toLowerCase()}</span>
    <span class="mx-auto flex h-5 max-w-40 min-w-0 flex-1 items-center gap-1.5 rounded-full bg-foreground/8 px-1">
      <span class="flex shrink-0 overflow-hidden rounded-full"><WelcomeParodyIcon app="bluedit" size={12} /></span>
      {@render bar("40%", "bg-foreground/15")}
    </span>
    <span class="flex shrink-0 items-center gap-2 text-foreground/80">
      <MessageCircleMore size={11} strokeWidth={1.9} class="shrink-0" />
      <span class="flex items-center gap-1">
        <SquarePlus size={11} strokeWidth={1.9} class="shrink-0" />
        <span class="hidden @lg:block">{@render bar("1.25rem", "bg-foreground/40")}</span>
      </span>
      <Bell size={11} strokeWidth={1.9} class="shrink-0" />
      <span class="size-3.5 shrink-0 rounded-full bg-foreground/20"></span>
    </span>
  </div>

  <div class="flex min-h-0 flex-1">
    <!-- The side bar: the feeds with home selected, the collapsed sections, and the joined communities. -->
    <div class="hidden w-24 shrink-0 flex-col gap-0.5 overflow-hidden border-r border-border px-1.5 pt-1.5 @md:flex">
      {#each FEEDS as feed, index (index)}
        <span class="flex h-5 shrink-0 items-center gap-1.5 rounded-md px-1.5 {index === 0 ? 'bg-foreground/8' : ''}">
          <feed.icon size={10} strokeWidth={index === 0 ? 2.4 : 1.9} class="shrink-0" />
          {@render bar(`${feed.label * 100}%`, index === 0 ? "bg-foreground/45" : "bg-foreground/20")}
        </span>
      {/each}
      {#each SECTIONS as fraction, index (index)}
        <span class="mt-1 flex h-5 shrink-0 items-center justify-between border-t border-border px-1.5 pt-1 text-muted-foreground">
          {@render bar(`${fraction * 70}%`, "bg-foreground/12", "h-1")}
          <ChevronDown size={9} strokeWidth={2} class="shrink-0" />
        </span>
      {/each}
      <span class="mt-1 flex h-5 shrink-0 items-center justify-between border-t border-border px-1.5 pt-1 text-muted-foreground">
        {@render bar("55%", "bg-foreground/12", "h-1")}
        <ChevronUp size={9} strokeWidth={2} class="shrink-0" />
      </span>
      {#each COMMUNITIES as fraction, index (index)}
        <span class="flex h-5 shrink-0 items-center gap-1.5 px-1.5">
          <span class="size-3 shrink-0 rounded-full bg-foreground/15"></span>
          <span class="min-w-0 flex-1">{@render bar(`${fraction * 100}%`, "bg-foreground/20")}</span>
          <Star size={8} strokeWidth={2} class="shrink-0 text-muted-foreground" />
        </span>
      {/each}
    </div>

    <!-- The home feed: its sort and view pickers, then the posts with their votes, comments, and share, which scroll. -->
    <div class="min-w-0 flex-1 overflow-hidden">
      <div class="flex flex-col px-2.5" style:transform={`translateY(${-scroll * travel}px)`}>
        <span class="flex h-6 shrink-0 items-center gap-2.5 text-muted-foreground">
          <span class="flex items-center gap-0.5">{@render bar("1.5rem", "bg-foreground/30")}<ChevronDown size={9} strokeWidth={2} /></span>
          <span class="flex items-center gap-0.5"><Rows2 size={9} strokeWidth={2} /><ChevronDown size={9} strokeWidth={2} /></span>
        </span>
        {#each POSTS as post, index (index)}
          <div class="flex flex-col gap-1.5 border-t border-border py-2">
            <span class="flex items-center gap-1">
              <span class="size-3 shrink-0 rounded-full bg-foreground/15"></span>
              {@render bar(`${post.community}px`, "bg-foreground/40")}
              <span class="size-0.5 shrink-0 rounded-full bg-foreground/30"></span>
              {@render bar(`${post.age}px`, "bg-foreground/12")}
              <Ellipsis size={9} class="ml-auto shrink-0 text-muted-foreground" />
            </span>
            {#each post.title as fraction, line (line)}
              {@render bar(`${fraction * 100}%`, "bg-foreground/45", "h-2")}
            {/each}
            {#if post.media}
              <span class="mt-0.5 block aspect-video w-full rounded-lg border border-border bg-foreground/8"></span>
            {/if}
            <span class="mt-0.5 flex items-center gap-1.5 text-foreground/80">
              <span class="flex h-4.5 items-center gap-1 rounded-full px-1.5 {post.upvoted ? 'text-white' : 'bg-foreground/8'}" style:background-color={post.upvoted ? PARODY_COLORS.bluedit : undefined}>
                <ArrowBigUp size={9} strokeWidth={2} class="shrink-0" fill={post.upvoted ? "currentColor" : "none"} />
                {@render bar("0.6rem", post.upvoted ? "bg-white/70" : "bg-foreground/25")}
                <ArrowBigDown size={9} strokeWidth={2} class="shrink-0" />
              </span>
              {@render pill(MessageCircle, "0.4rem")}
              {@render pill(Forward, "1.25rem")}
            </span>
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>
