<script lang="ts">
  import type { Snippet } from "svelte";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Feather from "@lucide/svelte/icons/feather";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Settings from "@lucide/svelte/icons/settings";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";

  /**
   * A phone welcome scene's top bar, drawn as the phone shell draws it: the label of what the tab shows, which is the
   * Calendar's name or the Novel project's context switcher, then the Focus ring, device linking, Quick notes, Music,
   * and Settings.
   */
  let {
    height,
    identity,
    identityExpanded = false,
    identityOpacity = 1,
    identityTouch,
    ring,
    ringLit = false,
    quickNotesOpen = false,
    musicOpen = false,
  }: {
    /** The bar's height in pixels. */
    height: number;
    /** What the tab shows: the Calendar, or a place in the Novel project, which has a navigator to open. */
    identity: "calendar" | "project";
    /** Whether the project's navigator is open. */
    identityExpanded?: boolean;
    /** How far the label shows, from 0 to 1, as one tab gives way to another. */
    identityOpacity?: number;
    /** A touch on the label. */
    identityTouch?: Snippet;
    /** The Focus ring with any touch on it, when the scene runs Focus; an idle ring otherwise. */
    ring?: Snippet;
    /** Whether the Focus ring is lit, as it is while Focus runs or its menu is open. */
    ringLit?: boolean;
    quickNotesOpen?: boolean;
    musicOpen?: boolean;
  } = $props();

  const { t } = getLocalization();

  /** The top bar's utility icon size and stroke, as the phone shell draws them. */
  const UTILITY_ICON_SIZE = 19;
  const UTILITY_STROKE_WIDTH = 1.8;
  /** The Focus ring's size in the phone's top bar. */
  const RING_SIZE = 20;
  const UTILITY_BUTTON_CLASS = "flex h-full w-(--mobile-chrome-column) shrink-0 items-center justify-center text-muted-foreground";
  const UTILITY_ICON_CLASS = "relative grid size-(--mobile-chrome-highlight) place-items-center rounded-full";
</script>

{#snippet utility(icon: typeof Settings, open: boolean)}
  {@const Icon = icon}
  <span class={cn(UTILITY_BUTTON_CLASS, open && "text-foreground")}>
    <span class={cn(UTILITY_ICON_CLASS, open && "bg-accent/70")}><Icon size={UTILITY_ICON_SIZE} strokeWidth={UTILITY_STROKE_WIDTH} /></span>
  </span>
{/snippet}

<div class="flex shrink-0 items-center justify-between gap-1 bg-(--cal-bg) pr-(--mobile-chrome-inset) pl-2 text-foreground" style:height={`${height}px`}>
  <span class="flex h-full min-w-0 flex-1 items-center">
    <span
      class={cn(
        "relative flex h-9 max-w-full min-w-0 items-center gap-2 rounded-full pr-2.5 pl-2 text-identity font-medium",
        identityExpanded ? "bg-accent" : "bg-accent/55",
      )}
      style:opacity={identityOpacity < 1 ? identityOpacity : undefined}
    >
      <span class="grid size-5 shrink-0 place-items-center">
        {#if identity === "calendar"}
          <CalendarIcon size={COMPACT_IDENTITY_ICON_SIZE} strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH} />
        {:else}
          <Feather size={COMPACT_IDENTITY_ICON_SIZE} strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH} />
        {/if}
      </span>
      <span class="min-w-0 truncate">{identity === "calendar" ? workspaceTabKindLabel("calendar", t) : t("vaultSetup.projects.project")}</span>
      {#if identity === "project"}<ChevronDown size={16} class="shrink-0 text-muted-foreground" />{/if}
      {@render identityTouch?.()}
    </span>
  </span>
  <span class="flex h-full shrink-0 items-center">
    <span class={cn(UTILITY_BUTTON_CLASS, ringLit && "text-foreground")}>
      <span class={cn(UTILITY_ICON_CLASS, ringLit && "bg-accent/70")}>
        {#if ring}
          {@render ring()}
        {:else}
          <PomodoroProgressRing
            active={false}
            remainingSeconds={0}
            totalSeconds={0}
            size={RING_SIZE}
            trackClass="stroke-muted-foreground/35"
            progressClass="text-foreground/76 stroke-foreground/76"
          />
        {/if}
      </span>
    </span>
    {@render utility(RefreshCw, false)}
    {@render utility(StickyNote, quickNotesOpen)}
    {@render utility(Music, musicOpen)}
    {@render utility(Settings, false)}
  </span>
</div>
