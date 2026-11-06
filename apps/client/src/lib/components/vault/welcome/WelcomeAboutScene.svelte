<script lang="ts" module>
  /** Canvas geometry in logical pixels. */
  const CANVAS_W = 384;
  const CANVAS_H = 200;

  export const ABOUT_SCENE_WIDTH = CANVAS_W;
  export const ABOUT_SCENE_HEIGHT = CANVAS_H;
</script>

<script lang="ts">
  import Folder from "@lucide/svelte/icons/folder";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { getEventColor } from "$lib/calendar/utils";
  import type { EventColor } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { View } from "$lib/navigation";
  import { getTheme } from "$lib/stores/theme.svelte";
  import {
    ABOUT_EDITED_ITEM,
    aboutCodeReveal,
    aboutComputerItem,
    aboutEdit,
    aboutLink,
    aboutPackets,
    aboutPhone,
    aboutPhoneItem,
    aboutSweeping,
  } from "$lib/vault/welcome-about-scene";
  import { workspaceTabKindIcon } from "$lib/workspace-tabs/presentation";

  /**
   * One frame of the welcome scene about the app itself: a computer and a phone holding the same vault. The vault
   * fills in on the computer, its items travel to the phone along a direct link, an edit on the phone travels back,
   * and a sweep shows the code underneath both screens. Every style is a function of `elapsed`, with no CSS
   * transitions, so text is never handed to a compositing layer mid-animation.
   */
  let { elapsed }: { elapsed: number } = $props();

  const { t } = getLocalization();
  const theme = getTheme();

  /** The drawn devices, matching the drawn phones of the Android access guides with their `p-1.5` bezel. */
  const BEZEL = 6;
  const COMPUTER = { left: 0, top: 16, width: 212, height: 168 } as const;
  const PHONE = { left: 292, top: 8, width: 92, height: 184 } as const;
  /** How far the phone rises as it arrives. */
  const PHONE_RISE = 10;
  const COMPUTER_TOP_BAR_H = 20;
  /** The link runs between the two devices, with a little air at each end. */
  const LINK_Y = CANVAS_H / 2;
  const LINK_START = COMPUTER.left + COMPUTER.width + 8;
  const LINK_END = PHONE.left - 8;
  /** How far an item rises into place as it appears. */
  const ITEM_RISE = 4;

  /** Palette slots, so the drawing follows the active theme's event colors. */
  const BLUE = 18;
  const PURPLE = 22;
  const ORANGE = 7;
  const CYAN = 16;

  /** The vault's items, one per main feature, with the length of their drawn titles on each device. */
  const ITEMS: readonly { kind: View; color: EventColor; computer: number; phone: number }[] = [
    { kind: "calendar", color: BLUE, computer: 92, phone: 36 },
    { kind: "projects", color: ORANGE, computer: 70, phone: 28 },
    { kind: "notes", color: PURPLE, computer: 84, phone: 32 },
    { kind: "chat", color: CYAN, computer: 58, phone: 24 },
  ];
  /** How much longer the edited item's title grows on each device. */
  const EDIT_GROWTH = { computer: 28, phone: 12 } as const;

  type CodeTone = "keyword" | "call" | "text" | "type" | "plain";
  /** Lines of code as indentation and runs of tokens, each run a width and a tone. */
  const CODE: readonly (readonly [indent: number, runs: readonly (readonly [width: number, tone: CodeTone])[]])[] = [
    [0, [[18, "keyword"], [34, "call"], [10, "plain"]]],
    [1, [[14, "keyword"], [22, "plain"], [40, "text"]]],
    [1, [[28, "call"], [12, "plain"], [24, "type"]]],
    [2, [[46, "plain"]]],
    [2, [[16, "keyword"], [30, "text"]]],
    [1, [[8, "plain"]]],
    [0, []],
    [0, [[18, "keyword"], [26, "type"], [10, "plain"]]],
    [1, [[20, "plain"], [36, "call"]]],
    [1, [[22, "keyword"], [30, "plain"]]],
    [0, [[8, "plain"]]],
  ];
  /** The phone shows the same code at a narrower width. */
  const PHONE_CODE_SCALE = 0.45;

  const phone = $derived(aboutPhone(elapsed));
  const link = $derived(aboutLink(elapsed));
  const packets = $derived(aboutPackets(elapsed));
  const edit = $derived(aboutEdit(elapsed));
  const reveal = $derived(aboutCodeReveal(elapsed));
  const sweeping = $derived(aboutSweeping(elapsed));
  const sweepX = $derived(reveal * CANVAS_W);
  const computerCode = $derived(screenReveal(COMPUTER.left + BEZEL, COMPUTER.width - 2 * BEZEL));
  const phoneCode = $derived(screenReveal(PHONE.left + BEZEL, PHONE.width - 2 * BEZEL));

  /** How much of a screen, from its left edge, the sweep has turned into code, from 0 to 1. */
  function screenReveal(left: number, width: number): number {
    return Math.min(1, Math.max(0, (sweepX - left) / width));
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

  function titleWidth(index: number, device: "computer" | "phone"): number {
    const base = ITEMS[index][device];
    return index === ABOUT_EDITED_ITEM ? base + EDIT_GROWTH[device] * edit[device] : base;
  }

  function highlight(index: number): string | undefined {
    if (index !== ABOUT_EDITED_ITEM || edit.highlight <= 0) return undefined;
    return `color-mix(in srgb, var(--accent) ${edit.highlight * 100}%, transparent)`;
  }
</script>

{#snippet item(index: number, shown: number, device: "computer" | "phone")}
  {@const entry = ITEMS[index]}
  {@const colors = getEventColor(entry.color, theme.current)}
  {@const Icon = workspaceTabKindIcon(entry.kind)}
  {@const compact = device === "phone"}
  <div
    class="relative -mx-1 flex items-center rounded-md px-1"
    class:gap-2={!compact}
    class:gap-1.5={compact}
    style:height={compact ? "20px" : "24px"}
    style:top={`${(1 - shown) * ITEM_RISE}px`}
    style:opacity={shown}
    style:background-color={highlight(index)}
  >
    <span
      class="grid shrink-0 place-items-center rounded-[5px]"
      style:width={compact ? "14px" : "16px"}
      style:height={compact ? "14px" : "16px"}
      style:background-color={colors.bg}
      style:color={colors.text}
    >
      <Icon size={compact ? 8 : 10} strokeWidth={2.2} aria-hidden="true" />
    </span>
    <span class="h-1 rounded-full bg-foreground/20" style:width={`${titleWidth(index, device)}px`}></span>
    {#if !compact}
      <span class="ml-auto h-1 w-5 rounded-full bg-foreground/10"></span>
    {/if}
  </div>
{/snippet}

{#snippet vaultName(compact: boolean)}
  <div class="flex items-center gap-1.5 font-medium text-foreground" class:text-[11px]={!compact} class:text-[9px]={compact}>
    <Folder size={compact ? 9 : 11} strokeWidth={2} class="text-muted-foreground" aria-hidden="true" />
    {t("common.appName")}
  </div>
{/snippet}

{#snippet code(scale: number, lineHeight: number)}
  {#each CODE as [indent, runs], line (line)}
    <div class="flex items-center gap-1" style:height={`${lineHeight}px`} style:padding-left={`${indent * 8 * scale}px`}>
      {#each runs as [width, tone], run (run)}
        <span class="h-1 shrink-0 rounded-full" style:width={`${Math.max(4, width * scale)}px`} style:background-color={toneColor(tone)}></span>
      {/each}
    </div>
  {/each}
{/snippet}

<div class="relative" style:width={`${CANVAS_W}px`} style:height={`${CANVAS_H}px`}>
  <!-- The link between the devices, with nothing in between them. -->
  <div
    class="absolute h-0.5 opacity-60"
    style:left={`${LINK_START}px`}
    style:top={`${LINK_Y - 1}px`}
    style:width={`${(LINK_END - LINK_START) * link}px`}
    style:background-image="repeating-linear-gradient(to right, var(--muted-foreground) 0 4px, transparent 4px 8px)"
  ></div>

  {#each packets as position, index (index)}
    <span
      class="absolute size-2 rounded-full bg-primary"
      style:left={`${LINK_START + position * (LINK_END - LINK_START) - 4}px`}
      style:top={`${LINK_Y - 4}px`}
      style:box-shadow="0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent)"
      style:opacity={Math.min(1, position * 8, (1 - position) * 8)}
    ></span>
  {/each}

  <!-- The computer. -->
  <div
    class="absolute rounded-3xl bg-foreground/6 p-1.5"
    style:left={`${COMPUTER.left}px`}
    style:top={`${COMPUTER.top}px`}
    style:width={`${COMPUTER.width}px`}
    style:height={`${COMPUTER.height}px`}
  >
    <div class="relative h-full overflow-hidden rounded-2xl bg-background">
      <div class="flex items-center justify-end gap-2.5 px-3 text-muted-foreground" style:height={`${COMPUTER_TOP_BAR_H}px`}>
        <Minus size={8} strokeWidth={2} aria-hidden="true" />
        <Square size={7} strokeWidth={2} aria-hidden="true" />
        <X size={8} strokeWidth={2} aria-hidden="true" />
      </div>
      <div class="absolute inset-x-0 bottom-0 px-3" style:top={`${COMPUTER_TOP_BAR_H}px`} style:clip-path={`inset(0 0 0 ${computerCode * 100}%)`}>
        {@render vaultName(false)}
        <div class="mt-1.5">
          {#each ITEMS as _, index (index)}
            {@render item(index, aboutComputerItem(elapsed, index), "computer")}
          {/each}
        </div>
      </div>
      {#if computerCode > 0}
        <div class="absolute inset-x-0 bottom-0 pl-3" style:top={`${COMPUTER_TOP_BAR_H}px`} style:clip-path={`inset(0 ${(1 - computerCode) * 100}% 0 0)`}>
          {@render code(1, 11.5)}
        </div>
      {/if}
    </div>
  </div>

  <!-- The phone, arriving once the computer's vault is full. -->
  {#if phone > 0}
    <div
      class="absolute rounded-3xl bg-foreground/6 p-1.5"
      style:left={`${PHONE.left}px`}
      style:top={`${PHONE.top + (1 - phone) * PHONE_RISE}px`}
      style:width={`${PHONE.width}px`}
      style:height={`${PHONE.height}px`}
      style:opacity={phone}
    >
      <div class="relative h-full overflow-hidden rounded-2xl bg-background">
        <div class="absolute inset-0 px-2.5 pt-4" style:clip-path={`inset(0 0 0 ${phoneCode * 100}%)`}>
          {@render vaultName(true)}
          <div class="mt-1.5">
            {#each ITEMS as _, index (index)}
              {@render item(index, aboutPhoneItem(elapsed, index), "phone")}
            {/each}
          </div>
        </div>
        {#if phoneCode > 0}
          <div class="absolute inset-0 pl-2.5 pt-4" style:clip-path={`inset(0 ${(1 - phoneCode) * 100}% 0 0)`}>
            {@render code(PHONE_CODE_SCALE, 10)}
            {@render code(PHONE_CODE_SCALE, 10)}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <!-- The sweep that opens both screens onto their code. -->
  {#if sweeping}
    <div
      class="absolute inset-y-1 w-0.5 rounded-full"
      style:left={`${sweepX - 1}px`}
      style:background-image="linear-gradient(to bottom, transparent, var(--primary) 18%, var(--primary) 82%, transparent)"
      style:box-shadow="0 0 12px color-mix(in srgb, var(--primary) 45%, transparent)"
    ></div>
  {/if}
</div>
