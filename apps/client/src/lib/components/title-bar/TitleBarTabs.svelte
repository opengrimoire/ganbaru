<script lang="ts">
  import Book from "@lucide/svelte/icons/book";
  import Calendar from "@lucide/svelte/icons/calendar";
  import Folder from "@lucide/svelte/icons/folder";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { mainTabViews, type DetachableTabView } from "$lib/navigation";
  import { getDetachedWindows } from "$lib/stores/detached-windows.svelte";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { cn } from "$lib/utils";
  import {
    DETACHED_VIEW_DRAG_MIME,
    serializeDetachedViewDragPayload,
  } from "$lib/windows/detached";

  let {
    detachedWindowView,
    windowLabel,
    pomodoroMenuOpen,
    onVolumeWheel,
    onOpenContextMenu,
  }: {
    detachedWindowView: DetachableTabView | undefined;
    windowLabel: string;
    pomodoroMenuOpen: boolean;
    onVolumeWheel: (event: WheelEvent) => void;
    onOpenContextMenu: (event: MouseEvent, view: DetachableTabView) => void;
  } = $props();

  const nav = getNavigation();
  const preferences = getPreferences();
  const viewport = getViewport();
  const detachedWindows = getDetachedWindows();
  const { t } = getLocalization();

  const iconSize = 14;
  const iconStrokeWidth = 1.5;
  const tabs: { view: DetachableTabView; label: () => string; icon: typeof Calendar }[] = [
    { view: "calendar", label: () => t("titleBar.tab.calendar"), icon: Calendar },
    { view: "projects", label: () => t("titleBar.tab.projects"), icon: Folder },
    { view: "notes", label: () => t("titleBar.tab.notes"), icon: Book },
    { view: "chat", label: () => t("titleBar.tab.chat"), icon: MessageSquare },
  ];
  const visibleTabs = $derived.by(() => {
    if (detachedWindowView) return tabs.filter((tab) => tab.view === detachedWindowView);
    const visibleViews = new Set(mainTabViews(detachedWindows.views));
    return tabs.filter((tab) => visibleViews.has(tab.view));
  });
  const compact = $derived(
    preferences.titleBarVisibility.compactTabs || viewport.below("regular"),
  );
  const activeOnly = $derived(viewport.below("narrow"));

  let tabElements: HTMLButtonElement[] = $state([]);
  let indicatorStyle = $state("");
  let indicatorFrame = 0;
  let wheelCooldown = false;

  function updateIndicator(): void {
    const index = visibleTabs.findIndex((tab) => tab.view === nav.current);
    const element = tabElements[index];
    const parent = element?.parentElement;
    if (!element || !parent) {
      indicatorStyle = "";
      return;
    }
    const parentRect = parent.getBoundingClientRect();
    const elementRect = element.getBoundingClientRect();
    indicatorStyle = `left: ${elementRect.left - parentRect.left}px; width: ${elementRect.width}px;`;
  }

  function scheduleIndicatorUpdate(): void {
    if (indicatorFrame) cancelAnimationFrame(indicatorFrame);
    indicatorFrame = requestAnimationFrame(() => {
      indicatorFrame = 0;
      updateIndicator();
    });
  }

  $effect(() => {
    void nav.current;
    void visibleTabs;
    void compact;
    void activeOnly;
    void preferences.fontFamilyId;
    void preferences.fontScale;
    scheduleIndicatorUpdate();
    return () => {
      if (indicatorFrame) cancelAnimationFrame(indicatorFrame);
      indicatorFrame = 0;
    };
  });

  export function handleWheel(event: WheelEvent): void {
    if (pomodoroMenuOpen) {
      onVolumeWheel(event);
      return;
    }
    event.preventDefault();
    if (wheelCooldown || Math.abs(event.deltaY) < 5 || visibleTabs.length === 0) return;
    wheelCooldown = true;
    const currentIndex = Math.max(
      0,
      visibleTabs.findIndex((tab) => tab.view === nav.current),
    );
    const nextIndex = Math.max(
      0,
      Math.min(visibleTabs.length - 1, currentIndex + (event.deltaY > 0 ? 1 : -1)),
    );
    if (nextIndex !== currentIndex) nav.navigate(visibleTabs[nextIndex].view);
    setTimeout(() => { wheelCooldown = false; }, 300);
  }

  function handleDragStart(event: DragEvent, view: DetachableTabView): void {
    if (!detachedWindowView || view !== detachedWindowView || !event.dataTransfer) return;
    const payload = serializeDetachedViewDragPayload(view, windowLabel);
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(DETACHED_VIEW_DRAG_MIME, payload);
    event.dataTransfer.setData("text/plain", payload);
  }
</script>

<div class="relative flex min-w-0 items-center gap-0.5 overflow-hidden pl-1.5">
  {#if indicatorStyle}
    <div
      class="tab-indicator absolute top-0 h-full rounded-md bg-card shadow-xs dark:bg-accent dark:shadow-none"
      style={indicatorStyle}
    ></div>
  {/if}
  {#each visibleTabs as tab, index}
    <button
      bind:this={tabElements[index]}
      onclick={() => nav.navigate(tab.view)}
      oncontextmenu={(event) => onOpenContextMenu(event, tab.view)}
      draggable={!!detachedWindowView}
      ondragstart={(event) => handleDragStart(event, tab.view)}
      class={cn(
        "titlebar-tab relative z-1 flex items-center rounded-md text-sm font-medium transition-colors",
        activeOnly && nav.current !== tab.view ? "hidden" : "",
        compact ? "titlebar-tab-compact justify-center" : "gap-1.5 px-3",
        nav.current === tab.view
          ? "text-foreground dark:text-white"
          : "text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
      )}
      title={t("titleBar.tab.withShortcut", tab.label(), `Alt+${index + 1}`)}
    >
      <tab.icon size={iconSize} strokeWidth={iconStrokeWidth} />
      {#if !compact}<span>{tab.label()}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .titlebar-tab {
    height: 28px;
  }

  .titlebar-tab-compact {
    width: 28px;
  }

  .tab-indicator {
    transition: none;
  }

  :global(html[data-focus-intent="keyboard"]) .titlebar-tab:focus {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }
</style>
