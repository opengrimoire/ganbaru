<script lang="ts" module>
  import type { View } from "$lib/navigation";

  /** A tab in a phone welcome scene's tab bar. */
  export interface WelcomePhoneTab {
    kind: View;
    /** Whether the tab is the one showing. */
    active: boolean;
    /** How far a touch on the tab shows, from 0 to 1. */
    touch?: number;
  }
</script>

<script lang="ts">
  import SquarePlus from "@lucide/svelte/icons/square-plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { workspaceTabKindIcon, workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";
  import WelcomeTouch from "./WelcomeTouch.svelte";

  /**
   * A phone welcome scene's tab bar, drawn as the phone shell draws it along the bottom: the open tabs, then the tabs
   * button with its add sign. The Calendar's tab carries its name, and a tab in the Novel project carries the project's.
   */
  let { tabs }: { tabs: readonly WelcomePhoneTab[] } = $props();

  const { t } = getLocalization();

  /** The tab bar's icon size and strokes, as the phone shell draws them. */
  const ICON_SIZE = 20;
  const ICON_STROKE_WIDTH = 1.7;
  const SELECTED_ICON_STROKE_WIDTH = 2;
  const ITEM_CLASS = "flex min-w-0 flex-col items-center justify-center gap-1 text-xs font-medium";
</script>

<div
  class="grid h-(--mobile-bottom-nav-h) shrink-0 border-t border-sidebar-border bg-(--cal-bg) px-2 text-foreground"
  style:grid-template-columns={`repeat(${tabs.length + 1}, minmax(0, 1fr))`}
>
  {#each tabs as tab (tab.kind)}
    {@const Icon = workspaceTabKindIcon(tab.kind)}
    <span class={cn(ITEM_CLASS, tab.active ? "text-foreground" : "text-muted-foreground")}>
      <span class={cn("relative grid h-8 w-14 place-items-center rounded-full", tab.active && "bg-accent")}>
        <Icon size={ICON_SIZE} strokeWidth={tab.active ? SELECTED_ICON_STROKE_WIDTH : ICON_STROKE_WIDTH} />
        <WelcomeTouch opacity={tab.touch ?? 0} />
      </span>
      <span class="max-w-full truncate">{tab.kind === "calendar" ? workspaceTabKindLabel(tab.kind, t) : t("vaultSetup.projects.project")}</span>
    </span>
  {/each}
  <span class={cn(ITEM_CLASS, "text-muted-foreground")}>
    <span class="grid h-8 w-14 place-items-center rounded-full"><SquarePlus size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} /></span>
    <span class="max-w-full truncate">{t("titleBar.tab.tabs")}</span>
  </span>
</div>
