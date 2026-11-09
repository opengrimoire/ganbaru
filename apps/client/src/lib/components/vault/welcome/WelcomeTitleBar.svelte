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
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { workspaceTabKindIcon, workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";

  /**
   * A desktop welcome scene's title bar, drawn as the title bar draws it: the open tabs and the new tab button, then
   * the utilities the scene shows. The Calendar's tab carries its name, and a tab in the Novel project carries the
   * project's name beside its kind's icon.
   */
  let {
    height,
    endPadding,
    tabs,
    children,
  }: {
    /** The bar's height in pixels. */
    height: number;
    /** The room after the last utility, in pixels. */
    endPadding: number;
    tabs: readonly WelcomeTitleTab[];
    /** The title bar's utilities. */
    children?: Snippet;
  } = $props();

  const { t } = getLocalization();

  /** The tab strip's icon size and stroke, as the title bar draws them. */
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;
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
  <div class="flex shrink-0 items-center gap-0.5">{@render children?.()}</div>
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
