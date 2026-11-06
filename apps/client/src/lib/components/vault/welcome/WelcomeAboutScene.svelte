<script lang="ts" module>
  /** Canvas geometry in logical pixels: a fixed width, and a height the room chooses between these bounds. */
  const CANVAS_W = 384;
  const MIN_CANVAS_H = 256;
  const MAX_CANVAS_H = 360;

  export const ABOUT_SCENE_WIDTH = CANVAS_W;
  export const ABOUT_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const ABOUT_SCENE_MAX_HEIGHT = MAX_CANVAS_H;
</script>

<script lang="ts">
  import CodeXml from "@lucide/svelte/icons/code-xml";
  import Globe from "@lucide/svelte/icons/globe";
  import User from "@lucide/svelte/icons/user";
  import { getEventColor } from "$lib/calendar/utils";
  import type { EventColor } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import {
    ABOUT_CODE_LINES,
    ABOUT_SCREEN_ROWS,
    aboutCard,
    aboutCodeLine,
    aboutCopy,
    aboutRecipient,
    aboutScreenRow,
  } from "$lib/vault/welcome-about-scene";

  /**
   * One frame of the welcome scene about the app itself: its code, written out in the open, and the devices of five
   * people around it. Each receives its own copy of the code, which then fills with that person's own things, so the
   * code is shared with everyone while what each person keeps stays theirs. Every style is a function of `elapsed`,
   * with no CSS transitions, so text is never handed to a compositing layer mid-animation.
   */
  let { elapsed, height }: { elapsed: number; height: number } = $props();

  const { t } = getLocalization();
  const theme = getTheme();

  /** The public code, centered at the top. */
  const CARD_W = 156;
  const CARD_H = 104;
  const CARD_LEFT = (CANVAS_W - CARD_W) / 2;
  const CARD_HEADER_H = 26;
  const CODE_LINE_H = 11;
  /** How far the code and the devices rise into place as they appear. */
  const RISE = 6;
  /** Breathing room at the top and bottom, which grows a little with the canvas. */
  const MIN_PAD = 10;
  const PAD_SHARE = 0.15;
  /** How far the copies' paths spread along the code's bottom edge, as a share of each device's offset. */
  const FAN = 0.35;
  const PACKET = 8;

  /** Palette slots, so the drawing follows the active theme's event colors. */
  const PINK = 1;
  const ORANGE = 7;
  const YELLOW = 8;
  const GREEN = 12;
  const TEAL = 14;
  const CYAN = 16;
  const BLUE = 18;
  const PURPLE = 22;

  type CodeTone = "keyword" | "call" | "text" | "type" | "plain";
  /** Lines of code as indentation and runs of tokens, each run a width and a tone. */
  const CODE: readonly (readonly [indent: number, runs: readonly (readonly [width: number, tone: CodeTone])[]])[] = [
    [0, [[16, "keyword"], [30, "call"], [10, "plain"]]],
    [1, [[12, "keyword"], [20, "plain"], [36, "text"]]],
    [1, [[26, "call"], [10, "plain"], [22, "type"]]],
    [2, [[40, "plain"], [14, "keyword"]]],
    [1, [[14, "keyword"], [28, "text"]]],
    [0, [[8, "plain"]]],
  ];

  /** A person's device below the code, with its center, how far the arc lifts it, and that person's colors. */
  interface Recipient {
    kind: "laptop" | "phone";
    x: number;
    lift: number;
    person: EventColor;
    items: readonly { color: EventColor; width: number }[];
  }

  const RECIPIENTS: readonly Recipient[] = [
    { kind: "laptop", x: 38, lift: 18, person: ORANGE, items: [{ color: BLUE, width: 30 }, { color: GREEN, width: 22 }, { color: ORANGE, width: 26 }] },
    { kind: "phone", x: 112, lift: 5, person: TEAL, items: [{ color: PURPLE, width: 12 }, { color: YELLOW, width: 9 }, { color: TEAL, width: 11 }] },
    { kind: "laptop", x: 192, lift: 0, person: PURPLE, items: [{ color: CYAN, width: 24 }, { color: PINK, width: 32 }, { color: PURPLE, width: 20 }] },
    { kind: "phone", x: 272, lift: 5, person: PINK, items: [{ color: ORANGE, width: 10 }, { color: BLUE, width: 13 }, { color: GREEN, width: 8 }] },
    { kind: "laptop", x: 346, lift: 18, person: BLUE, items: [{ color: YELLOW, width: 26 }, { color: TEAL, width: 20 }, { color: PINK, width: 30 }] },
  ];

  const DEVICE_SIZE = {
    laptop: { width: 66, height: 44, base: 78 },
    phone: { width: 32, height: 56, base: 0 },
  } as const;
  const LAPTOP_BASE_H = 4;

  const pad = $derived(MIN_PAD + (height - MIN_CANVAS_H) * PAD_SHARE);
  const cardTop = $derived(pad);
  const cardBottom = $derived(cardTop + CARD_H);
  const card = $derived(aboutCard(elapsed));

  /** The bottom of device `index`, where it rests on the arc. */
  function deviceBottom(index: number): number {
    return height - pad - RECIPIENTS[index].lift;
  }

  /** The top of device `index`'s drawing, base included. */
  function deviceTop(index: number): number {
    const recipient = RECIPIENTS[index];
    const size = DEVICE_SIZE[recipient.kind];
    return deviceBottom(index) - size.height - (recipient.kind === "laptop" ? LAPTOP_BASE_H : 0);
  }

  /** Where the copy for device `index` leaves the code, and where it lands. */
  function copyPath(index: number): { x1: number; y1: number; x2: number; y2: number } {
    const x = RECIPIENTS[index].x;
    return { x1: CANVAS_W / 2 + (x - CANVAS_W / 2) * FAN, y1: cardBottom, x2: x, y2: deviceTop(index) - 4 };
  }

  function toneColor(tone: CodeTone): string {
    switch (tone) {
      case "keyword": return getEventColor(PURPLE, theme.current).bg;
      case "call": return getEventColor(BLUE, theme.current).bg;
      case "text": return getEventColor(ORANGE, theme.current).bg;
      case "type": return getEventColor(CYAN, theme.current).bg;
      case "plain": return "color-mix(in srgb, var(--foreground) 22%, transparent)";
    }
  }
</script>

{#snippet screen(index: number)}
  {@const recipient = RECIPIENTS[index]}
  {@const compact = recipient.kind === "phone"}
  <div class="flex h-full flex-col gap-1" class:px-1.5={!compact} class:pt-1.5={!compact} class:px-1={compact} class:pt-2={compact}>
    {#each recipient.items.slice(0, ABOUT_SCREEN_ROWS) as entry, row (row)}
      {@const shown = aboutScreenRow(elapsed, index, row)}
      <div class="relative flex items-center gap-1" style:top={`${(1 - shown) * 3}px`} style:opacity={shown}>
        <span class="size-1.5 shrink-0 rounded-[2px]" style:background-color={getEventColor(entry.color, theme.current).bg}></span>
        <span class="h-0.75 rounded-full bg-foreground/20" style:width={`${entry.width}px`}></span>
      </div>
    {/each}
  </div>
{/snippet}

<div class="relative" style:width={`${CANVAS_W}px`} style:height={`${height}px`}>
  <!-- The copies' paths, each drawn as its copy travels, and the copies on their way. -->
  <svg class="absolute inset-0 overflow-visible" width={CANVAS_W} {height} aria-hidden="true">
    {#each RECIPIENTS as _, index (index)}
      {@const copy = aboutCopy(elapsed, index)}
      {#if copy > 0}
        {@const path = copyPath(index)}
        {@const x = path.x1 + (path.x2 - path.x1) * copy}
        {@const y = path.y1 + (path.y2 - path.y1) * copy}
        <line
          x1={path.x1}
          y1={path.y1}
          x2={x}
          y2={y}
          class="stroke-muted-foreground"
          stroke-width="1.5"
          stroke-dasharray="3 4"
          stroke-linecap="round"
          opacity="0.5"
        />
        {#if copy < 1}
          <circle cx={x} cy={y} r={PACKET / 2 + 3} class="fill-primary" opacity={0.22 * Math.min(1, (1 - copy) * 6)} />
          <circle cx={x} cy={y} r={PACKET / 2} class="fill-primary" opacity={Math.min(1, copy * 8, (1 - copy) * 6)} />
        {/if}
      {/if}
    {/each}
  </svg>

  <!-- The public code. -->
  {#if card > 0}
    <div
      class="absolute rounded-3xl bg-foreground/6 p-1.5"
      style:left={`${CARD_LEFT}px`}
      style:top={`${cardTop + (1 - card) * RISE}px`}
      style:width={`${CARD_W}px`}
      style:height={`${CARD_H}px`}
      style:opacity={card}
    >
      <div class="h-full overflow-hidden rounded-2xl bg-background px-3">
        <div class="flex items-center gap-1.5 text-[10px] font-medium text-foreground" style:height={`${CARD_HEADER_H}px`}>
          <CodeXml size={11} strokeWidth={2} class="text-muted-foreground" aria-hidden="true" />
          {t("common.appName")}
          <Globe size={10} strokeWidth={2} class="ml-auto text-muted-foreground" aria-hidden="true" />
        </div>
        {#each CODE.slice(0, ABOUT_CODE_LINES) as [indent, runs], line (line)}
          {@const written = aboutCodeLine(elapsed, line)}
          <div
            class="flex items-center gap-1"
            style:height={`${CODE_LINE_H}px`}
            style:padding-left={`${indent * 8}px`}
            style:clip-path={`inset(0 ${(1 - written) * 100}% 0 0)`}
          >
            {#each runs as [width, tone], run (run)}
              <span class="h-1 shrink-0 rounded-full" style:width={`${width}px`} style:background-color={toneColor(tone)}></span>
            {/each}
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Five people's devices, each holding its own copy and, once it arrives, that person's own things. -->
  {#each RECIPIENTS as recipient, index (index)}
    {@const shown = aboutRecipient(elapsed, index)}
    {#if shown > 0}
      {@const size = DEVICE_SIZE[recipient.kind]}
      {@const person = getEventColor(recipient.person, theme.current)}
      {@const top = deviceTop(index) + (1 - shown) * RISE}
      <div class="absolute" style:left={`${recipient.x - size.width / 2}px`} style:top={`${top}px`} style:opacity={shown}>
        <div
          class="bg-foreground/6 p-1"
          class:rounded-lg={recipient.kind === "laptop"}
          class:rounded-xl={recipient.kind === "phone"}
          style:width={`${size.width}px`}
          style:height={`${size.height}px`}
        >
          <div class="h-full overflow-hidden bg-background" class:rounded-sm={recipient.kind === "laptop"} class:rounded-lg={recipient.kind === "phone"}>
            {@render screen(index)}
          </div>
        </div>
        {#if recipient.kind === "laptop"}
          <div
            class="relative rounded-b-md bg-foreground/10"
            style:left={`${(size.width - size.base) / 2}px`}
            style:width={`${size.base}px`}
            style:height={`${LAPTOP_BASE_H}px`}
          ></div>
        {/if}
        <span
          class="absolute -right-2 -bottom-2 grid size-4.5 place-items-center rounded-full ring-2 ring-background"
          style:background-color={person.bg}
          style:color={person.text}
        >
          <User size={10} strokeWidth={2.4} aria-hidden="true" />
        </span>
      </div>
    {/if}
  {/each}
</div>
