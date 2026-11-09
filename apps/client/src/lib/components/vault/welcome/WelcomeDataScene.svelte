<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 300;
  const MAX_CANVAS_W = 820;
  const MIN_CANVAS_H = 300;
  const MAX_CANVAS_H = 420;

  export const DATA_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const DATA_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const DATA_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const DATA_SCENE_MAX_HEIGHT = MAX_CANVAS_H;
</script>

<script lang="ts">
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Database from "@lucide/svelte/icons/database";
  import Download from "@lucide/svelte/icons/download";
  import FileBraces from "@lucide/svelte/icons/file-braces";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import House from "@lucide/svelte/icons/house";
  import Image from "@lucide/svelte/icons/image";
  import Monitor from "@lucide/svelte/icons/monitor";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM, BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { DATA_ENTRIES, vaultPathTrail, type DataEntry } from "$lib/vault/welcome-data-folder";

  /**
   * The still illustration on the welcome screen about where data lives: the vault's folder, named by its real path, open
   * in a plain sketch of a file manager in the theme's colors, with each entry beside a line on what it holds. A wide
   * canvas adds a sidebar of places with the folder's parent selected, and the desktop shell adds the window's controls
   * and history arrows.
   */
  let { width, height, path }: { width: number; height: number; path: string } = $props();

  const { t } = getLocalization();
  /** The window's frame, matching the other welcome scenes. */
  const BEZEL = 6;
  /** From this canvas width, the window has room for its sidebar of places. */
  const SIDEBAR_MIN_W = 540;
  /** From this canvas width, each entry's description sits beside its name instead of under it. */
  const INLINE_DETAIL_MIN_W = 440;
  const desktopShell = BUILD_PLATFORM_PROFILE.shell === "desktop";
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";
  /** App storage on a phone has no browsable parents, so the phone shows only the folder itself. */
  const TRAIL_DEPTH = desktopShell ? 2 : 1;

  /** Every Lucide icon shares one component type. */
  type PlaceIcon = typeof Folder;

  /** The sidebar's places, with the folder's parent selected; the others are placeholder bars of these widths. */
  const PLACES: readonly { icon: PlaceIcon; width: number; selected?: boolean }[] = [
    { icon: House, width: 0.5 },
    { icon: Monitor, width: 0.62 },
    { icon: FileText, width: 0, selected: true },
    { icon: Download, width: 0.7 },
    { icon: Image, width: 0.56 },
  ];

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const wide = $derived(canvasW >= SIDEBAR_MIN_W);
  const inlineDetail = $derived(canvasW >= INLINE_DETAIL_MIN_W);
  const trail = $derived(vaultPathTrail(path, TRAIL_DEPTH));
  const parent = $derived(trail.length > 1 ? trail[0] : null);

  function entryIcon(entry: DataEntry): PlaceIcon {
    if (entry.folder) return Folder;
    return entry.detail === "database" ? Database : FileBraces;
  }
</script>

{#snippet bar(fraction: number, extraClass = "")}
  <span class={`block h-1.5 rounded-full bg-foreground/12 ${extraClass}`} style:width={`${fraction * 100}%`}></span>
{/snippet}

{#snippet controls()}
  <span class="flex shrink-0 gap-1" aria-hidden="true">
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
  </span>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5" style:width={`${width}px`} style:height={`${height}px`}>
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-background text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    <!-- The window's bar: its history arrows and controls on the desktop, and the folder's path. -->
    <div class="flex h-10 shrink-0 items-center gap-2 border-b border-border px-3">
      {#if desktopShell && CONTROLS_LEADING}{@render controls()}{/if}
      {#if desktopShell}
        <span class="flex shrink-0 items-center gap-0.5 text-muted-foreground/60">
          <ChevronLeft size={14} strokeWidth={1.8} aria-hidden="true" />
          <ChevronRight size={14} strokeWidth={1.8} aria-hidden="true" />
        </span>
      {/if}
      <span class="flex h-6.5 min-w-0 flex-1 items-center gap-1 rounded-md bg-foreground/6 px-2 text-[11.5px]">
        {#each trail as segment, index (index)}
          {#if index < trail.length - 1}
            <span class="min-w-0 shrink truncate text-muted-foreground">{segment}</span>
            <ChevronRight size={11} strokeWidth={1.8} class="shrink-0 text-muted-foreground/60" aria-hidden="true" />
          {:else}
            <span class="flex min-w-0 items-center gap-1 font-medium">
              <Folder size={12} strokeWidth={1.8} class="shrink-0 text-primary" fill="currentColor" fill-opacity="0.3" aria-hidden="true" />
              <span class="truncate">{segment}</span>
            </span>
          {/if}
        {/each}
      </span>
      {#if desktopShell && !CONTROLS_LEADING}{@render controls()}{/if}
    </div>

    <div class="flex min-h-0 flex-1">
      {#if wide}
        <!-- The places, with the folder's parent selected and the rest as placeholder bars. -->
        <div class="flex w-40 shrink-0 flex-col gap-0.5 border-r border-border bg-foreground/2 px-2 py-2.5 text-[11px]">
          {#each PLACES as place, index (index)}
            <span
              class={[
                "flex h-7 items-center gap-2 rounded-md px-2",
                place.selected && parent ? "bg-foreground/8 font-medium text-foreground" : "text-muted-foreground",
              ]}
            >
              <place.icon size={13} strokeWidth={1.8} aria-hidden="true" />
              {#if place.selected && parent}
                <span class="truncate">{parent}</span>
              {:else}
                <span class="min-w-0 flex-1">{@render bar(place.selected ? 0.66 : place.width)}</span>
              {/if}
            </span>
          {/each}
        </div>
      {/if}

      <!-- The folder's entries. -->
      <div class="flex min-w-0 flex-1 flex-col">
        <div class="flex h-7 shrink-0 items-center gap-3 border-b border-border px-3.5">
          <span class="w-16">{@render bar(1)}</span>
          <span class="flex-1"></span>
          {#if wide}<span class="w-8">{@render bar(1)}</span>{/if}
        </div>
        {#each DATA_ENTRIES as entry (entry.name)}
          {@const EntryIcon = entryIcon(entry)}
          <div class="flex min-h-11 shrink-0 items-center gap-2.5 border-b border-border/60 px-3.5">
            <EntryIcon
              size={16}
              strokeWidth={1.7}
              class={["shrink-0", entry.folder ? "text-primary" : "text-muted-foreground"]}
              fill={entry.folder ? "currentColor" : "none"}
              fill-opacity={entry.folder ? 0.3 : undefined}
              aria-hidden="true"
            />
            <span class={["flex min-w-0 flex-1", inlineDetail ? "items-center gap-3" : "flex-col gap-0.5 py-1.5"]}>
              <span class={["truncate text-[12px] font-medium", inlineDetail && "w-28 shrink-0"]}>{entry.name}</span>
              <span class="min-w-0 truncate text-[11px] text-muted-foreground">{t(`vaultSetup.data.entries.${entry.detail}`)}</span>
            </span>
            {#if wide}<span class="w-8 shrink-0">{@render bar(1)}</span>{/if}
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>
