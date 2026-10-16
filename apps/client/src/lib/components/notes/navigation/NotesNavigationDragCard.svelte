<script lang="ts">
  /**
   * The row a Notes sidebar drag lifts: a copy of the folder or note row that follows the pointer, names the
   * destination it would move to, and settles into the row's new place or back into the old one when released.
   */
  import Folder from "@lucide/svelte/icons/folder";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import {
    COMPACT_IDENTITY_EMOJI_SCALE,
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import type { NotesNavigationDragCardPhase, NotesNavigationRect } from "$lib/notes/navigation/drag-motion";
  import type { NotesPageIcon as NotesPageIconValue } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";

  let {
    kind,
    title,
    icon = null,
    frame,
    phase,
    forbidden = false,
    hint = null,
    durationMs,
  }: {
    kind: "folder" | "page";
    title: string;
    icon?: NotesPageIconValue | null;
    frame: NotesNavigationRect;
    phase: NotesNavigationDragCardPhase;
    forbidden?: boolean;
    hint?: string | null;
    durationMs: number;
  } = $props();

  let shownHint = $state<string | null>(null);

  $effect.pre(() => {
    if (hint) shownHint = hint;
  });
</script>

<div
  use:portal
  class={cn(
    "notes-navigation-drag-card explorer-sidebar pointer-events-none fixed top-0 left-0 z-90 rounded-md",
    phase === "lifted"
      ? "scale-[1.02] shadow-xl ring-1 ring-border starting:scale-100 starting:shadow-none"
      : "scale-100 shadow-none",
    phase === "vanishing" && "scale-95 opacity-0",
    phase === "lifted" && forbidden && "opacity-60",
  )}
  data-phase={phase}
  style:translate={`${frame.left}px ${frame.top}px`}
  style:width={`${frame.width}px`}
  style:height={`${frame.height}px`}
  style:--notes-drag-card-duration={`${durationMs}ms`}
  aria-hidden="true"
>
  <div class="explorer-row flex h-full min-w-0 items-center gap-1.5 px-2">
    {#if kind === "folder"}
      <Folder size={COMPACT_IDENTITY_ICON_SIZE} class="explorer-row-icon" strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH} />
    {:else}
      <NotesPageIcon
        {icon}
        size={COMPACT_IDENTITY_ICON_SIZE}
        strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
        emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
        class="explorer-row-icon"
      />
    {/if}
    <span class="min-w-0 flex-1 truncate explorer-row-label">{title}</span>
  </div>
  {#if shownHint}
    <span
      class={cn(
        "notes-navigation-drag-hint absolute top-full left-2 mt-1 flex max-w-full items-center gap-1 rounded-md bg-primary px-1.5 py-0.5 text-xs text-primary-foreground shadow-md starting:opacity-0",
        (phase !== "lifted" || !hint) && "opacity-0",
      )}
    >
      <FolderInput class="size-3 shrink-0" />
      <span class="truncate">{shownHint}</span>
    </span>
  {/if}
</div>

<style>
  .notes-navigation-drag-card {
    background: var(--popover);
    color: var(--popover-foreground);
    transition-property: scale, box-shadow, opacity;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .notes-navigation-drag-card[data-phase="settling"] {
    background: transparent;
    transition-property: translate, width, height, scale, box-shadow, opacity, background-color;
    transition-duration: var(--notes-drag-card-duration);
    transition-timing-function: cubic-bezier(0.2, 0, 0, 1);
  }

  .notes-navigation-drag-card[data-phase="vanishing"] {
    transition-property: translate, scale, box-shadow, opacity;
    transition-timing-function: cubic-bezier(0.2, 0, 0, 1);
    transition-duration: var(--notes-drag-card-duration);
  }

  .notes-navigation-drag-hint {
    transition: opacity 120ms ease-out;
  }

  @media (prefers-reduced-motion: reduce) {
    .notes-navigation-drag-card,
    .notes-navigation-drag-hint {
      transition: none;
    }
  }
</style>
