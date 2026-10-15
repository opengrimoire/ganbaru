<script lang="ts">
  import Lock from "@lucide/svelte/icons/lock";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MobileNavigationPresentation } from "$lib/mobile/layout";
  import { mobileTabBarSlots, mobileTabCountLabel } from "$lib/mobile/tab-bar";
  import { cn } from "$lib/utils";
  import type { WorkspaceTab } from "$lib/workspace-tabs/model";
  import { workspaceTabFullLabel, workspaceTabKindIcon, workspaceTabLabel } from "$lib/workspace-tabs/presentation";

  /**
   * The phone shell's workspace tab bar: the first open tabs that fit, the active one always among them, and a
   * tabs button with the open count that opens the full list.
   */
  let {
    tabs,
    activeTabId,
    tabsOpen = false,
    presentation,
    onSelect,
    onOpenTabs,
  }: {
    tabs: readonly WorkspaceTab[];
    activeTabId: string;
    /** Whether the tabs sheet is showing. */
    tabsOpen?: boolean;
    presentation: MobileNavigationPresentation;
    onSelect: (tabId: string) => void;
    onOpenTabs: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const slots = $derived(mobileTabBarSlots(tabs, activeTabId));
  const countLabel = $derived(mobileTabCountLabel(tabs.length, (value) => formatNumber(localization.locale, value)));
  const iconSize = $derived(presentation === "rail" ? 21 : 20);
</script>

{#snippet itemContent(selected: boolean, icon: import("svelte").Snippet, label: string)}
  {#if presentation === "bottom"}
    <span
      class={cn("relative grid h-8 w-14 place-items-center rounded-full transition-colors", selected ? "bg-accent" : "group-active:bg-accent/60")}
      data-mobile-navigation-indicator={selected || undefined}
    >
      {@render icon()}
    </span>
  {:else}
    <span class="relative grid place-items-center">{@render icon()}</span>
  {/if}
  <span class="max-w-full truncate">{label}</span>
{/snippet}

<nav
  aria-label={t("mobile.primaryNavigation")}
  class={cn(
    "mobile-primary-navigation",
    presentation === "rail"
      ? "flex w-(--mobile-nav-rail-w) shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-2 py-3 text-sidebar-foreground"
      : "grid h-(--mobile-bottom-nav-h) shrink-0 border-t border-sidebar-border px-2 text-foreground",
  )}
  style={presentation === "bottom" ? `background-color: var(--cal-bg); grid-template-columns: repeat(${slots.length + 1}, minmax(0, 1fr));` : undefined}
>
  {#each slots as tab (tab.id)}
    {@const Icon = workspaceTabKindIcon(tab.location.kind)}
    {@const selected = tab.id === activeTabId}
    {@const label = workspaceTabLabel(tab, t)}
    {@const fullLabel = workspaceTabFullLabel(tab, t)}
    {#snippet icon()}
      <Icon size={iconSize} strokeWidth={selected ? 2 : 1.7} aria-hidden="true" />
      {#if tab.locked}
        <Lock class="absolute -right-0.5 -bottom-0.5 rounded-full bg-(--cal-bg) p-px" size={11} strokeWidth={2} aria-hidden="true" data-mobile-tab-locked />
      {/if}
    {/snippet}
    <button
      type="button"
      aria-current={selected ? "page" : undefined}
      aria-label={tab.locked ? t("titleBar.tab.lockedTab", fullLabel) : fullLabel}
      onclick={() => onSelect(tab.id)}
      class={cn(
        "group relative flex min-w-0 flex-col items-center justify-center gap-1 text-xs font-medium transition-colors",
        presentation === "rail" && "mb-2 min-h-12 rounded-xl",
        presentation === "rail"
          ? selected
            ? "bg-sidebar-accent text-sidebar-accent-foreground"
            : "text-sidebar-foreground/70 active:bg-sidebar-accent/70"
          : selected
            ? "text-foreground"
            : "text-muted-foreground",
      )}
    >
      {@render itemContent(selected, icon, label)}
    </button>
  {/each}

  {#snippet countBox()}
    <span
      class="grid h-5 min-w-5 place-items-center rounded-md border-[1.5px] border-current px-1 text-[0.666667rem] leading-none font-semibold tabular-nums"
      aria-hidden="true"
      data-mobile-tab-count
    >{countLabel}</span>
  {/snippet}
  <button
    type="button"
    aria-label={t("titleBar.tab.showTabs", tabs.length)}
    aria-haspopup="dialog"
    aria-expanded={tabsOpen}
    onclick={onOpenTabs}
    class={cn(
      "group relative flex min-w-0 flex-col items-center justify-center gap-1 text-xs font-medium transition-colors",
      presentation === "rail" && "mt-auto min-h-12 rounded-xl",
      presentation === "rail"
        ? tabsOpen
          ? "bg-sidebar-accent text-sidebar-accent-foreground"
          : "text-sidebar-foreground/70 active:bg-sidebar-accent/70"
        : tabsOpen
          ? "text-foreground"
          : "text-muted-foreground",
    )}
    data-mobile-tabs-button
  >
    {@render itemContent(tabsOpen, countBox, t("titleBar.tab.tabs"))}
  </button>
</nav>
