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
  import type { VaultDefaultPlaces } from "$lib/vault/state";
  import { DATA_ENTRIES, vaultPathTrail, type DataEntry } from "$lib/vault/welcome-data-folder";

  /**
   * The still illustration on the welcome screen about where data lives: the vault's folder, named by its real path, open
   * in a plain sketch of a file manager in the theme's colors. The desktop shell shows the entries as a grid of icons
   * with a line on what each holds, adds the window's controls and history arrows, and, on a wide canvas, a sidebar of
   * places with the folder's parent selected; a phone lists the entries by name.
   */
  let {
    width,
    height,
    path,
    places = null,
  }: {
    width: number;
    height: number;
    path: string;
    /** The system's names for the sidebar's other places; a place without one is left out. */
    places?: VaultDefaultPlaces | null;
  } = $props();

  const { t } = getLocalization();
  /** The window's frame, matching the other welcome scenes. */
  const BEZEL = 6;
  /** From this canvas width, the window has room for its sidebar of places. */
  const SIDEBAR_MIN_W = 540;
  const desktopShell = BUILD_PLATFORM_PROFILE.shell === "desktop";
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";
  /** App storage on a phone has no browsable parents, so the phone shows only the folder itself. */
  const TRAIL_DEPTH = desktopShell ? 2 : 1;

  /** Every Lucide icon shares one component type. */
  type PlaceIcon = typeof Folder;

  /** A sidebar place named by the system's own folder name for it. */
  type Place = { icon: PlaceIcon; place: keyof VaultDefaultPlaces };

  /**
   * The sidebar's places: Home under its common label rather than the account's name, then the folder's parent, named by
   * its real path, among the places named as the system names them.
   */
  const PLACES: readonly (Place | { icon: PlaceIcon; parent: true } | { icon: PlaceIcon; home: true })[] = [
    { icon: House, home: true },
    { icon: Monitor, place: "desktop" },
    { icon: FileText, parent: true },
    { icon: Download, place: "downloads" },
    { icon: Image, place: "pictures" },
  ];

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const wide = $derived(canvasW >= SIDEBAR_MIN_W);
  const trail = $derived(vaultPathTrail(path, TRAIL_DEPTH));
  const parent = $derived(trail.length > 1 ? trail[0] : null);

  function entryIcon(entry: DataEntry): PlaceIcon {
    if (entry.folder) return Folder;
    return entry.detail === "database" ? Database : FileBraces;
  }
</script>

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
    <div class="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
      {#if desktopShell && CONTROLS_LEADING}{@render controls()}{/if}
      {#if desktopShell}
        <span class="flex shrink-0 items-center gap-0.5 text-muted-foreground/60">
          <ChevronLeft size={15} strokeWidth={1.8} aria-hidden="true" />
          <ChevronRight size={15} strokeWidth={1.8} aria-hidden="true" />
        </span>
      {/if}
      <span class="flex h-8 min-w-0 flex-1 items-center gap-1.5 rounded-md bg-foreground/6 px-2.5 text-xs">
        {#each trail as segment, index (index)}
          {#if index < trail.length - 1}
            <span class="min-w-0 shrink truncate text-muted-foreground">{segment}</span>
            <ChevronRight size={12} strokeWidth={1.8} class="shrink-0 text-muted-foreground/60" aria-hidden="true" />
          {:else}
            <span class="flex min-w-0 items-center gap-1.5 font-medium">
              <Folder size={14} strokeWidth={1.8} class="shrink-0 text-primary" fill="currentColor" fill-opacity="0.3" aria-hidden="true" />
              <span class="truncate">{segment}</span>
            </span>
          {/if}
        {/each}
      </span>
      {#if desktopShell && !CONTROLS_LEADING}{@render controls()}{/if}
    </div>

    <div class="flex min-h-0 flex-1">
      {#if wide}
        <!-- The places, with the folder's parent selected. -->
        <div class="flex w-40 shrink-0 flex-col gap-0.5 border-r border-border bg-foreground/2 px-2 py-2.5 text-xs">
          {#each PLACES as place, index (index)}
            {@const selected = "parent" in place && parent !== null}
            {@const label =
              "place" in place
                ? (places?.[place.place] ?? null)
                : "home" in place
                  ? desktopShell
                    ? t("vaultSetup.data.home")
                    : null
                  : parent}
            {#if label}
              <span
                class={[
                  "flex h-7 items-center gap-2 rounded-md px-2",
                  selected ? "bg-foreground/8 font-medium text-foreground" : "text-muted-foreground",
                ]}
              >
                <place.icon size={13} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
                <span class="truncate">{label}</span>
              </span>
            {/if}
          {/each}
        </div>
      {/if}

      {#if desktopShell}
        <!-- The folder's entries as a grid of icons, each named with a line on what it holds under it. -->
        <div class="grid min-w-0 flex-1 auto-rows-min grid-cols-[repeat(auto-fill,8rem)] content-start gap-2 p-4 text-xs">
          {#each DATA_ENTRIES as entry (entry.name)}
            {@const EntryIcon = entryIcon(entry)}
            <div class="flex min-w-0 flex-col items-center gap-1 rounded-lg px-1.5 py-2 text-center">
              <EntryIcon
                size={40}
                strokeWidth={1.3}
                class={["mb-1 shrink-0", entry.folder ? "text-primary" : "text-muted-foreground"]}
                fill={entry.folder ? "currentColor" : "none"}
                fill-opacity={entry.folder ? 0.3 : undefined}
                aria-hidden="true"
              />
              <span class="max-w-full truncate font-medium">{entry.name}</span>
              <span class="line-clamp-2 text-muted-foreground">{t(`vaultSetup.data.entries.${entry.detail}`)}</span>
            </div>
          {/each}
        </div>
      {:else}
        <!-- A phone lists the entries by name only, since its private storage is never browsed. -->
        <div class="flex min-w-0 flex-1 flex-col text-xs">
          {#each DATA_ENTRIES as entry (entry.name)}
            {@const EntryIcon = entryIcon(entry)}
            <div class="flex min-h-11 items-center gap-3 border-b border-border/60 px-3.5">
              <EntryIcon
                size={16}
                strokeWidth={1.7}
                class={["shrink-0", entry.folder ? "text-primary" : "text-muted-foreground"]}
                fill={entry.folder ? "currentColor" : "none"}
                fill-opacity={entry.folder ? 0.3 : undefined}
                aria-hidden="true"
              />
              <span class="truncate font-medium">{entry.name}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
