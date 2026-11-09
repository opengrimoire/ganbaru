<script lang="ts" module>
  import type { View } from "$lib/navigation";

  /** A tab in a desktop welcome scene's title bar. */
  export interface WelcomeTitleTab {
    kind: View;
    /** Whether the tab is the one showing. */
    active: boolean;
    /** Whether the pointer is on the tab. */
    hovered?: boolean;
    /** The name the scene's pointer finds the tab by. */
    cursor?: string;
  }

  /** The tab strip's icon size and stroke, as the title bar draws them. */
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;
  /** A utility button's width and the gap between two, in pixels, as the title bar draws them. */
  const UTILITY_BUTTON_PX = 28;
  const UTILITY_GAP_PX = 2;
  /** The utilities after the Focus ring: device linking, Quick notes, Music, and Settings. */
  const UTILITIES_AFTER_RING = 4;

  /** The Focus ring's size and strokes in the title bar, for a scene that draws a running ring. */
  export const WELCOME_TITLE_RING_SIZE = ICON_SIZE + 0.5;
  export const WELCOME_TITLE_RING_TRACK_CLASS = "stroke-foreground/20 dark:stroke-white/20";
  export const WELCOME_TITLE_RING_PROGRESS_CLASS = "text-foreground/68 dark:text-white/76 stroke-foreground/68 dark:stroke-white/76";
  /** The room between the Focus ring's end and the last utility's end, in pixels, which a menu under the ring keeps. */
  export const WELCOME_TITLE_AFTER_RING_PX = UTILITIES_AFTER_RING * (UTILITY_BUTTON_PX + UTILITY_GAP_PX);
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Settings from "@lucide/svelte/icons/settings";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import X from "@lucide/svelte/icons/x";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import WelcomeTitleIcon from "$lib/components/vault/welcome/WelcomeTitleIcon.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { workspaceTabKindIcon, workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";

  /**
   * A desktop welcome scene's title bar, drawn as the title bar draws it: the open tabs and the new tab button, then
   * the Focus ring, device linking, Quick notes, Music, and Settings. The theme switch is left out, since the welcome
   * screens do not introduce it. The Calendar's tab carries its name, and a tab in the Novel project carries the
   * project's name beside its kind's icon.
   */
  let {
    height,
    endPadding,
    tabs,
    ring,
    ringOpen = false,
    ringHovered = false,
    ringCursor,
    quickNotesOpen = false,
    musicOpen = false,
    musicHovered = false,
    musicCursor,
  }: {
    /** The bar's height in pixels. */
    height: number;
    /** The room after the last utility, in pixels. */
    endPadding: number;
    tabs: readonly WelcomeTitleTab[];
    /** The Focus ring, when the scene runs Focus; an idle ring otherwise. */
    ring?: Snippet;
    /** Whether the Focus ring's menu is open. */
    ringOpen?: boolean;
    /** Whether the pointer is on the Focus ring. */
    ringHovered?: boolean;
    /** The name the scene's pointer finds the Focus ring by. */
    ringCursor?: string;
    quickNotesOpen?: boolean;
    musicOpen?: boolean;
    /** Whether the pointer is on the Music button. */
    musicHovered?: boolean;
    /** The name the scene's pointer finds the Music button by. */
    musicCursor?: string;
  } = $props();

  const { t } = getLocalization();
</script>

<div class="flex shrink-0 items-center gap-2 bg-sidebar" style:height={`${height}px`} style:padding-right={`${endPadding}px`}>
  <div class="flex min-w-0 flex-1 items-center gap-0.5 pl-1.5">
    <div class="flex min-w-0 items-center gap-0.5 overflow-hidden">
      {#each tabs as tab (tab.kind)}
        {@const Icon = workspaceTabKindIcon(tab.kind)}
        <span
          data-cursor={tab.cursor}
          data-active={tab.active ? "" : undefined}
          class={cn(
            "title-tab relative flex items-center rounded-md text-sm font-medium",
            tab.active
              ? "bg-card text-foreground shadow-(--tab-active-shadow) dark:text-white"
              : tab.hovered
                ? "bg-sidebar-accent text-sidebar-accent-foreground"
                : "text-sidebar-foreground/70",
          )}
        >
          <span class="title-tab-face flex h-full min-w-0 flex-1 items-center gap-1.5 pr-1 pl-3">
            <Icon class="shrink-0" size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
            <span class="title-tab-label min-w-0 flex-1 truncate text-start">
              {tab.kind === "calendar" ? workspaceTabKindLabel(tab.kind, t) : t("vaultSetup.projects.project")}
            </span>
          </span>
          <span class={cn("title-tab-close mr-1 flex shrink-0 items-center justify-center rounded-sm", tab.active || tab.hovered ? "opacity-70" : "opacity-0")}>
            <X size={12} strokeWidth={1.75} />
          </span>
        </span>
      {/each}
    </div>
    <span class="flex size-7 shrink-0 items-center justify-center rounded-md text-sidebar-foreground/70">
      <Plus size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
    </span>
  </div>
  <div class="flex shrink-0 items-center gap-0.5">
    <span data-cursor={ringCursor} class={cn("grid size-7 place-items-center rounded-lg", (ringOpen || ringHovered) && "bg-sidebar-accent")}>
      {#if ring}
        {@render ring()}
      {:else}
        <PomodoroProgressRing
          active={false}
          remainingSeconds={0}
          totalSeconds={0}
          size={WELCOME_TITLE_RING_SIZE}
          trackClass={WELCOME_TITLE_RING_TRACK_CLASS}
          progressClass={WELCOME_TITLE_RING_PROGRESS_CLASS}
        />
      {/if}
    </span>
    <WelcomeTitleIcon icon={RefreshCw} />
    <WelcomeTitleIcon icon={StickyNote} open={quickNotesOpen} />
    <WelcomeTitleIcon icon={Music} open={musicOpen} hovered={musicHovered} cursor={musicCursor} />
    <WelcomeTitleIcon icon={Settings} />
  </div>
</div>

<style>
  /* The title bar's tab sizing: every tab starts at the same width, they shrink evenly, and each drops what its own width cannot show. */
  .title-tab {
    container-type: inline-size;
    flex: 0 1 auto;
    width: 12.5rem;
    min-width: 2.25rem;
    height: 28px;
  }
  .title-tab-close {
    width: 18px;
    height: 18px;
  }
  @container (width < 6rem) {
    .title-tab:not([data-active]) .title-tab-close {
      display: none;
    }
  }
  @container (width < 4.5rem) {
    .title-tab-label {
      display: none;
    }
    .title-tab-face {
      justify-content: center;
      padding-left: 0;
    }
  }
  @container (width < 3.5rem) {
    .title-tab-close {
      display: none;
    }
    .title-tab-face {
      padding-right: 0;
    }
  }
</style>
