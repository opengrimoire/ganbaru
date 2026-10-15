<script lang="ts" module>
  /**
   * Transparent image the platform drags under the pointer. It is created once, ahead of any drag, because a
   * drag image must already be loaded when the drag starts.
   */
  const EMPTY_DRAG_IMAGE = (() => {
    const image = new Image(1, 1);
    image.src = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
    return image;
  })();
</script>

<script lang="ts">
  import { flushSync, tick, untrack } from "svelte";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import {
    TAB_ARRIVAL_TIMEOUT_MS,
    getWorkspaceTabs,
    type LiveTabDrag,
    type TabDragEnd,
  } from "$lib/stores/workspace-tabs.svelte";
  import { cn } from "$lib/utils";
  import { createSteadyPointerFilter, waitForPointerTravel } from "$lib/utils/hover-suppression";
  import type { WorkspaceTab, WorkspaceTabPlacement } from "$lib/workspace-tabs/model";
  import {
    shortWorkspaceTabProject,
    workspaceTabFullLabel,
    workspaceTabKindIcon,
    workspaceTabLabel,
    workspaceTabPositionShortcut,
    workspaceTabProjectHint,
  } from "$lib/workspace-tabs/presentation";
  import {
    tabDragLeftBar,
    tabDragPassedTrack,
    tabStripDropIndex,
    tabStripInsertIndex,
    tabStripInsertPosition,
    tabStripInsertTarget,
    tabStripMoveTarget,
    tabStripRight,
    tabStripVisualLeft,
    type TabStripSlot,
  } from "$lib/workspace-tabs/strip-drag";
  import TitleBarTabLauncher from "./TitleBarTabLauncher.svelte";
  import {
    TAB_WINDOW_DRAG_MIME,
    dragEndedOutsideWindow,
    tabWindowDragData,
    tearOffWindowPosition,
    type ScreenPoint,
    type TabDragGrab,
  } from "$lib/windows/tab-windows";

  let {
    pomodoroMenuOpen,
    onVolumeWheel,
    onOpenContextMenu,
    trackEnd,
  }: {
    pomodoroMenuOpen: boolean;
    onVolumeWheel: (event: WheelEvent) => void;
    onOpenContextMenu: (event: MouseEvent, tabId: string) => void;
    /** Empty title bar space after the strip; a tab dragged past its end leaves the bar. */
    trackEnd: HTMLElement | undefined;
  } = $props();

  type TabOrigin = { id: string; placement: WorkspaceTabPlacement };

  /** The dragged tab while this window holds it, measured in this strip. */
  interface HeldTabDrag extends TabDragGrab {
    id: string;
    /** Whether the tab may open in a new window of its own, which a window's last tab may not. */
    canTearOff: boolean;
  }

  /** A drag that started on one of this window's tabs, whose element must stay in the page until the drag ends. */
  interface NativeTabDrag {
    tab: WorkspaceTab;
    /** Neighbor that puts the tab back where it started when the drag is cancelled. */
    origin: TabOrigin | null;
  }

  const TAB_REORDER_MIME = "application/x-ganbaru-ai-workspace-tab";
  /** How long tabs take to slide into their new slots while one is dragged across them, and to settle when dropped. */
  const TAB_SLIDE_MS = 220;
  /** Ease-out curve of those slides, matching the cubic ease-out of the strip's reorder animation. */
  const TAB_EASE = "cubic-bezier(0.33, 1, 0.68, 1)";
  /** Marks the animations that slide tabs and the new tab button into place, apart from the page's other animations. */
  const SLIDE_ANIMATION_ID = "titlebar-tab-slide";
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;
  /** Events whose target tells where the pointer is. */
  const pointerTrackingEvents = ["pointermove", "pointerdown", "wheel"] as const;

  const tabs = getWorkspaceTabs();
  const preferences = getPreferences();
  const { t } = getLocalization();
  const reducedMotion = typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /** Icon-only tabs, chosen in the title bar settings. */
  const compact = $derived(preferences.titleBarVisibility.compactTabs);
  /** Tabs shrink like browser tabs; only when they no longer fit at their narrowest does the strip keep just the shown ones. */
  let activeOnly = $state(false);
  const canOpenTabs = $derived(tabs.canOpenTabs);
  /** The tab shown beside the active one, which reads as shown but not focused. */
  const partnerId = $derived(tabs.partnerTab?.id ?? null);
  /** The active tab's side of its split, so the indicator joins the partner tab like the tab itself does. */
  const activeSplitSide = $derived(splitSide(tabs.activeTabId));

  let strip: HTMLDivElement | undefined = $state();
  let addButton: HTMLButtonElement | undefined = $state();
  let tabElements: Record<string, HTMLElement> = $state({});
  let indicatorStyle = $state("");
  let indicatorFrame = 0;
  let wheelCooldown = false;
  let launcherOpen = $state(false);
  /** The tab drag under way in this vault, from this window or another. */
  const live = $derived(tabs.liveDrag);
  /**
   * The dragged tab while this window holds it. A tab moves to the window whose strip it is dragged over before
   * it is dropped, so the window holding it changes during the drag.
   */
  const heldId = $derived(live && tabs.holds(live.tab.id) ? live.tab.id : null);
  const canTearOffHeld = $derived(heldId !== null && tabs.detachBlock(heldId) === null);
  /** The drag started on one of this window's tabs, until it ends. */
  let nativeDrag = $state.raw<NativeTabDrag | null>(null);
  /** The dragged tab has been pulled off the title bar and floats under the pointer, leaving the window when dropped. */
  let offBar = $state(false);
  /** The drag has left this window, so nothing here draws the tab. */
  let outsideWindow = $state(false);
  /** Pointer position over this window while a tab is dragged. */
  let pointer = $state<{ x: number; y: number } | null>(null);
  /** How far the dragged tab is drawn from its slot so it stays under the pointer. */
  let movingOffset = $state(0);
  /** How far the new tab button is pushed right by a tab dragged past the last slot. */
  let addOffset = $state(0);
  /** This window's tab just dropped on the bar, easing from where it was drawn into its slot. */
  let settlingId = $state<string | null>(null);
  let settleTimer: ReturnType<typeof setTimeout> | undefined;
  /** This window's tab dropped elsewhere, kept out of the strip while it moves to its new window. */
  let departingId = $state<string | null>(null);
  let departureTimer: ReturnType<typeof setTimeout> | undefined;
  /** Hover highlights stay off from a drag's start until the pointer moves after it, so tabs a drag slides under the pointer do not light up. */
  let hoverHeld = $state(false);
  /**
   * Tab under the pointer as its latest event placed it. The strip tracks hover itself because WebKitGTK keeps
   * `:hover` where a native drag started, which lights up whichever tab takes that slot after the drop.
   */
  let pointerTabId = $state<string | null>(null);
  /** The pointer is on the new tab button, tracked like {@link pointerTabId}. */
  let pointerOnAdd = $state(false);
  /** The dragged tab no longer holds a slot in the strip. */
  const draggedTabLifted = $derived(heldId !== null && (offBar || (outsideWindow && canTearOffHeld)));
  /** Slot another window's tab takes in this strip while it is on its way here, or null while it is not on the bar. */
  let incomingIndex = $state<number | null>(null);
  /** How far another window's tab is drawn from its slot so it stays under the pointer. */
  let incomingOffset = $state(0);
  /** Another window's tab dropped here, which keeps its slot until it arrives. */
  let droppedTab = $state.raw<WorkspaceTab | null>(null);
  /** Another window's tab while it holds a slot in this strip. */
  const incomingTab = $derived(
    incomingIndex === null ? null : (droppedTab ?? (live && heldId === null ? live.tab : null)),
  );
  /**
   * The tabs the strip draws: this window's, with another window's dragged tab in the slot it would take. A tab
   * dragged from here keeps its element, out of sight, after it moves to another window, because the drag
   * ends on that element.
   */
  const shownTabs = $derived.by((): readonly WorkspaceTab[] => {
    const shown = [...tabs.tabs];
    if (incomingTab && incomingIndex !== null && !shown.some((tab) => tab.id === incomingTab.id)) {
      shown.splice(Math.min(incomingIndex, shown.length), 0, incomingTab);
    }
    const source = nativeDrag?.tab;
    if (source && !shown.some((tab) => tab.id === source.id)) shown.push(source);
    return shown;
  });
  /** A tab is dragged along this strip, so the others slide out of its way. */
  const stripDragActive = $derived(heldId !== null || incomingTab !== null);
  const hoverQuiet = $derived(stripDragActive || hoverHeld);
  const hoveredTabId = $derived(hoverQuiet ? null : pointerTabId);
  const addHovered = $derived(!hoverQuiet && pointerOnAdd);
  /** Tabs are dragged or still settling, so they may be drawn outside the strip. */
  const stripInMotion = $derived(stripDragActive || settlingId !== null);
  /** Tabs still slide into the places {@link slideLayout} gave them. */
  let slidingLayout = $state(false);
  /** Tabs are moving, so the active tab draws its own background and it moves with the tab, instead of the indicator. */
  const tabsDrawHighlight = $derived(stripInMotion || slidingLayout);
  /** The copy of a dragged tab that floats under the pointer once it is off the bar. */
  const floating = $derived.by((): { tab: WorkspaceTab; style: string } | null => {
    const at = pointer;
    const current = live;
    if (!at || outsideWindow || !current) return null;
    const style = `left: ${at.x - current.grabX}px; top: ${at.y - current.grabY}px; width: ${current.width}px;`;
    if (heldId !== null) {
      const tab = offBar ? tabs.tabs.find((candidate) => candidate.id === heldId) : undefined;
      return tab ? { tab, style } : null;
    }
    return incomingIndex === null && !droppedTab ? { tab: current.tab, style } : null;
  });
  /** Document drag entries minus exits while a tab is dragged, which reaches zero once the drag leaves the window. */
  let dragDepth = 0;
  let measureSequence = 0;

  /**
   * Whether the tabs are wider than the strip. Only tabs count: the active indicator is absolutely positioned and
   * can still sit where a closed tab was until the next frame, which would read as overflow on a short strip.
   */
  function stripOverflows(): boolean {
    if (!strip) return false;
    let contentRight = 0;
    for (const element of strip.querySelectorAll<HTMLElement>(".titlebar-tab")) {
      contentRight = Math.max(contentRight, element.offsetLeft + element.offsetWidth);
    }
    return contentRight > strip.clientWidth + 1;
  }

  /** Show every tab unless they overflow at their minimum width, measuring before the browser paints. */
  async function measureDensity(): Promise<void> {
    const sequence = ++measureSequence;
    activeOnly = false;
    await tick();
    if (sequence !== measureSequence) return;
    if (stripOverflows()) {
      activeOnly = true;
      await tick();
    }
    if (sequence === measureSequence) scheduleIndicatorUpdate();
  }

  $effect(() => {
    void tabs.tabs.length;
    void compact;
    void preferences.fontFamilyId;
    void preferences.fontScale;
    untrack(() => { void measureDensity(); });
  });

  $effect(() => {
    // The title bar width depends only on the window, so density changes cannot feed back into this observer.
    const element = strip?.closest<HTMLElement>(".app-title-bar");
    if (!element || typeof ResizeObserver === "undefined") return;
    let lastWidth = element.getBoundingClientRect().width;
    const observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? lastWidth;
      if (Math.abs(width - lastWidth) < 1) return;
      lastWidth = width;
      void measureDensity();
    });
    observer.observe(element);
    return () => observer.disconnect();
  });

  /**
   * The side of its split a tab shows on, or null when it is in none. A split's tabs sit next to each other in the
   * strip in the order the view shows them, so the first is also the left one here.
   */
  function splitSide(id: string): 0 | 1 | null {
    const split = tabs.splitOf(id);
    if (!split) return null;
    return split.tabIds[0] === id ? 0 : 1;
  }

  function updateIndicator(): void {
    const element = tabElements[tabs.activeTabId];
    const parent = element?.parentElement;
    // A tab kept off the strip on its way to another window, or already gone there, has no slot to mark.
    const away = tabs.activeTabId === departingId || !tabs.holds(tabs.activeTabId);
    if (!element || !parent || element.offsetParent === null || away) {
      indicatorStyle = "";
      return;
    }
    // Layout offsets ignore the transforms of slides still in flight, which would mark the slot a tab left.
    indicatorStyle = `left: ${element.offsetLeft}px; width: ${element.offsetWidth}px;`;
  }

  function scheduleIndicatorUpdate(): void {
    if (indicatorFrame) cancelAnimationFrame(indicatorFrame);
    indicatorFrame = requestAnimationFrame(() => {
      indicatorFrame = 0;
      updateIndicator();
    });
  }

  $effect(() => {
    void tabs.activeTabId;
    void tabs.tabs;
    void activeOnly;
    void departingId;
    // The indicator comes back once tabs stop moving, already under the active tab.
    if (!tabsDrawHighlight) updateIndicator();
    scheduleIndicatorUpdate();
    return () => {
      if (indicatorFrame) cancelAnimationFrame(indicatorFrame);
      indicatorFrame = 0;
    };
  });

  $effect(() => {
    if (tabs.launcherRequests === 0) return;
    untrack(() => {
      if (canOpenTabs) launcherOpen = true;
    });
  });

  export function handleWheel(event: WheelEvent): void {
    if (pomodoroMenuOpen) {
      onVolumeWheel(event);
      return;
    }
    event.preventDefault();
    if (wheelCooldown || Math.abs(event.deltaY) < 5) return;
    wheelCooldown = true;
    tabs.activateRelative(event.deltaY > 0 ? 1 : -1);
    setTimeout(() => { wheelCooldown = false; }, 300);
  }

  function tabTitle(tab: WorkspaceTab, index: number): string {
    const name = workspaceTabFullLabel(tab, t);
    const label = tab.locked ? t("titleBar.tab.lockedTab", name) : name;
    const shortcut = workspaceTabPositionShortcut(index, tabs.tabs.length);
    return shortcut ? t("titleBar.tab.withShortcut", label, shortcut) : label;
  }

  function handleAuxClick(event: MouseEvent, tab: WorkspaceTab): void {
    if (event.button !== 1) return;
    event.preventDefault();
    tabs.close(tab.id);
  }

  /** A tab opens as soon as it is pressed, as in a browser, so a tab dragged away is the one shown. */
  function handleMouseDown(event: MouseEvent, tab: WorkspaceTab): void {
    if (event.button === 1) event.preventDefault();
    const onClose = event.target instanceof Element && event.target.closest(".titlebar-tab-close") !== null;
    if (event.button === 0 && !onClose) tabs.activate(tab.id);
  }

  function closeFromButton(event: MouseEvent, tab: WorkspaceTab): void {
    event.stopPropagation();
    tabs.close(tab.id);
  }

  /** Neighbor that puts a tab back where it was, or null for a tab alone in its window. */
  function originOf(id: string): TabOrigin | null {
    const index = tabs.tabs.findIndex((tab) => tab.id === id);
    const next = tabs.tabs[index + 1];
    const previous = tabs.tabs[index - 1];
    if (next) return { id: next.id, placement: "before" };
    return previous ? { id: previous.id, placement: "after" } : null;
  }

  function handleDragStart(event: DragEvent, tab: WorkspaceTab): void {
    const transfer = event.dataTransfer;
    const element = event.currentTarget;
    if (!transfer || !(element instanceof HTMLElement)) return;
    const rect = element.getBoundingClientRect();
    const grab = { grabX: event.clientX - rect.left, grabY: event.clientY - rect.top, width: rect.width };
    transfer.effectAllowed = "move";
    transfer.setData(TAB_REORDER_MIME, tab.id);
    // A drag started on the close button skipped the press that opens the tab.
    tabs.activate(tab.id);
    nativeDrag = { tab, origin: originOf(tab.id) };
    const payload = tabs.beginDrag(tab.id, grab);
    if (payload) transfer.setData(TAB_WINDOW_DRAG_MIME, tabWindowDragData(payload));
    // The tab itself is drawn under the pointer while it is over a window, so the platform drags an empty image.
    transfer.setDragImage(EMPTY_DRAG_IMAGE, 0, 0);
    offBar = false;
    outsideWindow = false;
    pointer = { x: event.clientX, y: event.clientY };
    movingOffset = 0;
    dragDepth = 0;
  }

  /** A tab drag over this window: of the tab this window holds, or of a tab another window holds that may move here. */
  function dragKind(event: DragEvent): "held" | "foreign" | null {
    const types = event.dataTransfer?.types ?? [];
    if (!live || !(types.includes(TAB_REORDER_MIME) || types.includes(TAB_WINDOW_DRAG_MIME))) return null;
    return heldId === null ? "foreign" : "held";
  }

  /** The tab this window holds while it is dragged, measured in this strip. */
  function heldDrag(): HeldTabDrag | null {
    const current = live;
    if (!current || heldId === null) return null;
    const width = tabElements[heldId]?.offsetWidth || current.width;
    return { id: heldId, grabX: Math.min(current.grabX, width), grabY: current.grabY, width, canTearOff: canTearOffHeld };
  }

  function acceptDrag(event: DragEvent): void {
    event.preventDefault();
    // The document counts `dragenter` to tell when the drag leaves the window.
    if (event.type !== "dragenter") event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  /** Tabs the strip shows, by their layout position so slide animations in flight do not move the slots. */
  function shownSlots(): TabStripSlot[] {
    return shownTabs.flatMap((tab) => {
      const element = tabElements[tab.id];
      return element && element.offsetParent !== null
        ? [{ id: tab.id, left: element.offsetLeft, width: element.offsetWidth }]
        : [];
    });
  }

  /** Horizontal pointer position in the strip's layout coordinates. */
  function stripX(clientX: number): number {
    return strip ? clientX - strip.getBoundingClientRect().left + strip.scrollLeft : clientX;
  }

  /** Whether the pointer has been pulled far enough above or below the title bar to take a tab off it. */
  function pulledOffBar(clientY: number): boolean {
    const bar = strip?.closest<HTMLElement>(".app-title-bar");
    if (!strip || !bar) return false;
    return tabDragLeftBar(clientY, bar.getBoundingClientRect(), strip.getBoundingClientRect().height / 2);
  }

  /**
   * Right end, in strip coordinates, of the track a dragged tab slides along: past the last slot through the
   * empty title bar, up to the buttons at its end, keeping room for the new tab button that the tab pushes along.
   */
  function trackRight(): number {
    if (!strip || !trackEnd) return tabStripRight(shownSlots());
    const button = addButton
      ? addButton.offsetLeft + addButton.offsetWidth - strip.offsetLeft - strip.offsetWidth
      : 0;
    return stripX(trackEnd.getBoundingClientRect().right) - button;
  }

  /** Whether a tab dragged to `clientX` would reach the buttons at the end of the bar, which takes it off the bar. */
  function passedTrack(clientX: number, grabX: number, width: number): boolean {
    return tabDragPassedTrack(stripX(clientX), grabX, width, trackRight());
  }

  /** Push the new tab button along with a tab dragged past the last slot, whose right edge is at `right`. */
  function pushAddButton(right: number): void {
    addOffset = Math.max(0, right - tabStripRight(shownSlots()));
    // A pushed button rides with the tab instead of sliding into place.
    if (addOffset > 0 && addButton) stopSliding(addButton);
  }

  function stopSliding(element: HTMLElement): void {
    for (const animation of element.getAnimations()) {
      if (animation.id === SLIDE_ANIMATION_ID) animation.cancel();
    }
  }

  /** Shown tabs other than `skip`, and the new tab button: everything that moves when the strip changes. */
  function slidingElements(skip: string | null): HTMLElement[] {
    const elements = shownTabs.flatMap((tab) => {
      const element = tabElements[tab.id];
      return tab.id !== skip && element && element.offsetParent !== null ? [element] : [];
    });
    return addButton ? [...elements, addButton] : elements;
  }

  /**
   * Apply `change` to the strip, then slide the tabs and the new tab button from where they were drawn into their
   * new places, so nothing jumps when a tab is passed, lifted off the bar, or brought onto it. Only sideways
   * movement is animated, so a tab's text never stretches. `skip` is the dragged tab, which follows the pointer.
   */
  function slideLayout(change: () => void, skip: string | null): void {
    const before = reducedMotion
      ? new Map<HTMLElement, number>()
      : new Map(slidingElements(skip).map((element) => [element, element.getBoundingClientRect().left]));
    change();
    flushSync();
    for (const element of slidingElements(skip)) {
      const from = before.get(element);
      if (from === undefined) continue;
      stopSliding(element);
      const offset = from - element.getBoundingClientRect().left;
      if (Math.abs(offset) < 0.5) continue;
      const animation = element.animate(
        [{ transform: `translateX(${offset}px)` }, { transform: "none" }],
        { duration: TAB_SLIDE_MS, easing: TAB_EASE },
      );
      animation.id = SLIDE_ANIMATION_ID;
    }
    watchSlides();
  }

  /** Keep {@link slidingLayout} on until every slide in the strip has finished or been cancelled. */
  function watchSlides(): void {
    const running = slidingElements(null).flatMap((element) =>
      element.getAnimations().filter((animation) => animation.id === SLIDE_ANIMATION_ID),
    );
    slidingLayout = running.length > 0;
    if (running.length > 0) void Promise.allSettled(running.map((animation) => animation.finished)).then(watchSlides);
  }

  /**
   * Keep the dragged tab under the pointer along the strip. The tabs it passes trade places with it as soon as
   * it covers half of them, and it is then drawn shifted from its new slot so it never jumps.
   */
  function slideAlongStrip(current: HeldTabDrag, clientX: number): void {
    const slots = shownSlots();
    const left = tabStripVisualLeft(stripX(clientX), current.grabX, current.width, trackRight());
    const target = tabStripMoveTarget(
      slots.map((slot) => slot.id),
      current.id,
      tabStripDropIndex(slots, left + current.width / 2),
    );
    if (target) slideLayout(() => tabs.move(current.id, target.id, target.placement), current.id);
    const element = tabElements[current.id];
    movingOffset = element ? left - element.offsetLeft : 0;
    pushAddButton(left + current.width);
  }

  /**
   * Apply a `change` to where the held tab `id` is dragged. When it lifts out of the strip the window shows its
   * neighbor meanwhile, as a browser does, and when it lands back it is shown again.
   */
  function liftHeldTab(change: () => void, id: string): void {
    slideLayout(() => {
      const wasLifted = draggedTabLifted;
      change();
      if (draggedTabLifted === wasLifted) return;
      if (wasLifted) tabs.activate(id);
      else tabs.showNeighborOf(id);
    }, id);
  }

  /** Slide the dragged tab along the bar under the pointer, and lift it off once pulled away from the bar. */
  function dragOwnTab(event: DragEvent, current: HeldTabDrag): void {
    acceptDrag(event);
    if (outsideWindow) liftHeldTab(() => { outsideWindow = false; }, current.id);
    pointer = { x: event.clientX, y: event.clientY };
    const leaving = current.canTearOff
      && (pulledOffBar(event.clientY) || passedTrack(event.clientX, current.grabX, current.width));
    // The strip copy goes or comes at once, at full width, while the tabs around it slide.
    if (leaving !== offBar) liftHeldTab(() => { offBar = leaving; }, current.id);
    if (leaving) addOffset = 0;
    else slideAlongStrip(current, event.clientX);
  }

  /** Position among this window's tabs that a tab entering the strip at `clientX` takes. */
  function entryIndex(clientX: number): number {
    const slots = shownSlots();
    const target = tabStripInsertTarget(slots.map((slot) => slot.id), tabStripInsertIndex(slots, stripX(clientX)));
    return tabStripInsertPosition(tabs.tabs.map((tab) => tab.id), target);
  }

  /**
   * Draw another window's tab under the pointer in this strip, taking a slot as it enters and then trading
   * places with the tabs it passes, just like a tab of this window.
   */
  function slideIncoming(current: LiveTabDrag, clientX: number): void {
    if (incomingIndex === null) {
      const entry = entryIndex(clientX);
      slideLayout(() => { incomingIndex = entry; }, current.tab.id);
    }
    const element = tabElements[current.tab.id];
    if (!element) return;
    const width = element.offsetWidth;
    const slots = shownSlots();
    const left = tabStripVisualLeft(stripX(clientX), Math.min(current.grabX, width), width, trackRight());
    const target = tabStripMoveTarget(
      slots.map((slot) => slot.id),
      current.tab.id,
      tabStripDropIndex(slots, left + width / 2),
    );
    if (target) {
      const next = tabStripInsertPosition(tabs.tabs.map((tab) => tab.id), target);
      slideLayout(() => { incomingIndex = next; }, current.tab.id);
    }
    incomingOffset = left - element.offsetLeft;
    pushAddButton(left + width);
  }

  /** Whether another window's tab dragged to the pointer of `event` is off this window's bar. */
  function foreignOffBar(event: DragEvent, current: LiveTabDrag): boolean {
    if (pulledOffBar(event.clientY)) return true;
    const width = (incomingIndex === null ? undefined : tabElements[current.tab.id]?.offsetWidth) ?? current.width;
    return passedTrack(event.clientX, Math.min(current.grabX, width), width);
  }

  /** Neighbor next to which another window's tab joins this strip, from the slot it is drawn in or the pointer. */
  function incomingTarget(clientX: number): TabOrigin | null {
    return tabStripInsertTarget(tabs.tabs.map((tab) => tab.id), incomingIndex ?? entryIndex(clientX));
  }

  /**
   * Draw another window's tab in the strip while it is over the bar, and ask for it, so it moves here and opens
   * before it is dropped. Off the bar, or over a full strip, it only floats under the pointer.
   */
  function dragForeignTab(event: DragEvent, current: LiveTabDrag): void {
    if (droppedTab) return;
    pointer = { x: event.clientX, y: event.clientY };
    if (!tabs.canReceiveTab || foreignOffBar(event, current)) {
      leaveStrip(current.tab.id);
      return;
    }
    acceptDrag(event);
    slideIncoming(current, event.clientX);
    tabs.claim(current.tab.id, incomingTarget(event.clientX));
  }

  /**
   * Take another window's tab dropped on the bar into the slot it is drawn in, or, without a drawing, the slot
   * under the pointer. It keeps that slot until it arrives.
   */
  function dropForeignTab(event: DragEvent, current: LiveTabDrag): void {
    const id = current.tab.id;
    if (!tabs.canReceiveTab || foreignOffBar(event, current) || !tabs.claim(id, incomingTarget(event.clientX), { dropped: true })) {
      leaveStrip(id);
      return;
    }
    acceptDrag(event);
    if (incomingIndex !== null) {
      droppedTab = current.tab;
      settle(id);
    }
    tabs.finishDrag(id, { accepted: true, screen: tearOffWindowPosition(event.screenX, event.screenY) });
    incomingOffset = 0;
    endDrag();
  }

  /** Take another window's tab out of the strip once it is off the bar or outside the window, sliding the rest back. */
  function leaveStrip(id: string | null): void {
    if (incomingIndex !== null) slideLayout(() => { incomingIndex = null; }, id);
    addOffset = 0;
  }

  /** Stop drawing another window's tab, once its drag ended or it arrived. */
  function resetIncoming(): void {
    incomingIndex = null;
    incomingOffset = 0;
    droppedTab = null;
  }

  /**
   * Tab drags are handled for the whole window before any other drop target sees them, so a tab can be dropped
   * anywhere on the title bar and pulled off it over content that accepts other drags. `dragenter` is handled
   * like `dragover`, because WebKit takes the drop effect from `dragenter` whenever the pointer crosses into
   * another element, and would refuse a drop made right after the crossing.
   */
  function handleWindowDragOver(event: DragEvent): void {
    const kind = dragKind(event);
    const held = kind === "held" ? heldDrag() : null;
    if (held) dragOwnTab(event, held);
    else if (kind === "foreign" && live) dragForeignTab(event, live);
  }

  /**
   * A drop on this window ends the drag here. Another window's tab moves in unless it was pulled off the bar,
   * and the tab this window holds, already in its slot, opens in a new window right away when it was pulled off.
   */
  function handleWindowDrop(event: DragEvent): void {
    const kind = dragKind(event);
    if (kind === "foreign" && live) {
      dropForeignTab(event, live);
      return;
    }
    const held = kind === "held" ? heldDrag() : null;
    if (!held) return;
    acceptDrag(event);
    const screen = tearOffWindowPosition(event.screenX, event.screenY);
    const lifted = offBar;
    const origin = nativeDrag?.origin ?? null;
    tabs.finishDrag(held.id, { accepted: true, screen });
    if (lifted) holdDeparture(held.id, tabs.detach(held.id, screen), origin);
    else settle(held.id);
    endDrag();
  }

  /** Ease a tab dropped on the bar from where it was drawn into its slot. */
  function settle(id: string): void {
    clearTimeout(settleTimer);
    settlingId = id;
    settleTimer = setTimeout(() => { settlingId = null; }, TAB_SLIDE_MS);
  }

  /**
   * Keep a tab that left by drag out of the strip until it is gone, so it never shows here again on its way
   * out. When its `move` fails, or it has not gone in time, it comes back in front at `origin`.
   */
  function holdDeparture(id: string, move: Promise<void> | null, origin: TabOrigin | null): void {
    clearTimeout(departureTimer);
    departingId = id;
    const release = () => {
      clearTimeout(departureTimer);
      if (departingId !== id) return;
      slideLayout(() => {
        departingId = null;
        if (tabs.holds(id)) restoreTab(id, origin);
      }, null);
    };
    departureTimer = setTimeout(release, TAB_ARRIVAL_TIMEOUT_MS);
    void move?.catch((error: unknown) => {
      console.error("Failed to move tab to a new window:", error);
      release();
    });
  }

  /** End a drag that started on one of this window's tabs, as the platform reports it on that tab. */
  function handleDragEnd(event: DragEvent): void {
    const native = nativeDrag;
    if (!native) return;
    const leftWindow = dragEndedOutsideWindow(event, { width: window.innerWidth, height: window.innerHeight }, dragDepth);
    const end = tabs.finishDrag(native.tab.id, {
      accepted: event.dataTransfer?.dropEffect !== "none",
      screen: tearOffWindowPosition(event.screenX, event.screenY),
    });
    if (end) concludeDrag(end, outsideWindow || leftWindow, native.origin);
    else endDrag();
  }

  /**
   * End this window's drag as cancelled when its end never arrived. WebKitGTK may not report the end of a drag
   * dropped on the desktop shell, such as the GNOME top bar, yet no press reaches the page while a drag is under
   * way. Pointer moves prove nothing, because WebKit sends them during a drag after layout changes. Where the
   * tab was dropped is unknown, so it goes back where it started.
   */
  function recoverUnfinishedDrag(): void {
    const native = nativeDrag;
    if (!native) return;
    const end = tabs.finishDrag(native.tab.id, { accepted: false, screen: null });
    if (end) concludeDrag(end, false, native.origin);
    else endDrag();
  }

  $effect(() => {
    if (!nativeDrag) return;
    const options = { capture: true, passive: true };
    window.addEventListener("pointerdown", recoverUnfinishedDrag, options);
    return () => window.removeEventListener("pointerdown", recoverUnfinishedDrag, options);
  });

  /** Put a tab back where its drag started and show it. */
  function restoreTab(id: string, origin: TabOrigin | null): void {
    if (origin && tabs.tabs.some((tab) => tab.id === origin.id)) tabs.move(id, origin.id, origin.placement);
    tabs.activate(id);
  }

  /**
   * Act on the end of a drag that no drop on this window ended. When this window still holds the tab, a drop
   * another window took moves the tab there, a drag that ended `away` from this window opens the tab in a new
   * window, and otherwise, as when cancelled, the tab goes back where it started.
   */
  function concludeDrag(end: TabDragEnd, away: boolean, origin: TabOrigin | null): void {
    const id = end.tabId;
    if (!tabs.holds(id)) {
      endDrag();
      return;
    }
    const canTearOff = tabs.detachBlock(id) === null;
    if (end.accepted || (away && canTearOff)) {
      if (canTearOff) holdDeparture(id, end.accepted ? null : tabs.tearOff(id, end.screen), origin);
      endDrag();
      return;
    }
    slideLayout(() => {
      endDrag();
      restoreTab(id, origin);
    }, null);
  }

  /** Count tab drag entries and exits across the document, to tell when a drag left the window. */
  function trackDragDepth(event: DragEvent, change: 1 | -1): void {
    if (!dragKind(event)) return;
    dragDepth += change;
    if (heldId !== null) {
      const outside = dragDepth <= 0;
      if (outside !== outsideWindow) liftHeldTab(() => { outsideWindow = outside; }, heldId);
      if (outside) addOffset = 0;
    } else if (dragDepth <= 0 && !droppedTab) {
      leaveStrip(live?.tab.id ?? null);
      pointer = null;
    }
  }

  /** Forget the drag in this strip, keeping only another window's tab dropped here until it arrives. */
  function endDrag(): void {
    nativeDrag = null;
    offBar = false;
    outsideWindow = false;
    pointer = null;
    movingOffset = 0;
    addOffset = 0;
    dragDepth = 0;
    if (!droppedTab) {
      incomingIndex = null;
      incomingOffset = 0;
    }
  }

  $effect(() => () => {
    clearTimeout(settleTimer);
    clearTimeout(departureTimer);
  });

  $effect(() => {
    // The departing tab has gone to its new window.
    if (departingId !== null && !tabs.tabs.some((tab) => tab.id === departingId)) {
      untrack(() => {
        clearTimeout(departureTimer);
        departingId = null;
      });
    }
  });

  $effect(() => {
    if (stripDragActive) hoverHeld = true;
  });

  $effect(() => {
    // Engines replay the resting pointer after a drag, so only deliberate movement brings hover back.
    if (!hoverHeld || stripDragActive) return;
    return waitForPointerTravel(window, () => { hoverHeld = false; });
  });

  const steadyPointer = createSteadyPointerFilter();

  /** Records which tab, or the new tab button, the event's target sits in, skipping moves that jump like a replay. */
  function trackPointer(event: Event): void {
    const { clientX, clientY } = event as PointerEvent;
    const steady = steadyPointer(clientX, clientY);
    if (event.type === "pointermove" && !steady) return;
    const target = event.target instanceof Element ? event.target : null;
    const tab = target?.closest<HTMLElement>(".titlebar-tab");
    pointerTabId = tab && strip?.contains(tab) ? (tab.dataset.tabId ?? null) : null;
    pointerOnAdd = target !== null && addButton?.contains(target) === true;
  }

  function forgetPointer(event: PointerEvent): void {
    if (event.relatedTarget !== null) return;
    pointerTabId = null;
    pointerOnAdd = false;
  }

  $effect(() => {
    const options = { capture: true, passive: true };
    for (const type of pointerTrackingEvents) window.addEventListener(type, trackPointer, options);
    window.addEventListener("pointerout", forgetPointer, options);
    return () => {
      for (const type of pointerTrackingEvents) window.removeEventListener(type, trackPointer, options);
      window.removeEventListener("pointerout", forgetPointer, options);
    };
  });

  $effect(() => {
    const options = { capture: true };
    window.addEventListener("dragenter", handleWindowDragOver, options);
    window.addEventListener("dragover", handleWindowDragOver, options);
    window.addEventListener("drop", handleWindowDrop, options);
    return () => {
      window.removeEventListener("dragenter", handleWindowDragOver, options);
      window.removeEventListener("dragover", handleWindowDragOver, options);
      window.removeEventListener("drop", handleWindowDrop, options);
    };
  });

  // Another window saw the drag end, so no drag end reaches this one.
  $effect(() => tabs.onDragEnd((end) => concludeDrag(end, outsideWindow, nativeDrag?.origin ?? null)));

  let previousHeldId: string | null = null;
  $effect.pre(() => {
    const id = heldId;
    untrack(() => {
      const previous = previousHeldId;
      previousHeldId = id;
      if (id === previous || !live) return;
      if (id !== null && incomingIndex !== null) {
        // The tab drawn on its way here arrived, and goes on from where it was drawn.
        movingOffset = incomingOffset;
        incomingIndex = null;
        incomingOffset = 0;
      } else if (id === null) {
        // The tab moved to another window while still dragged, so nothing here draws it until it comes back.
        offBar = false;
        outsideWindow = false;
        pointer = null;
        movingOffset = 0;
        addOffset = 0;
      }
    });
  });

  $effect(() => {
    // Another drag started over this window, which must not keep the previous one's slot.
    void live;
    untrack(() => {
      if (!droppedTab && nativeDrag === null) {
        incomingIndex = null;
        incomingOffset = 0;
      }
    });
  });

  $effect(() => {
    // A tab dropped here holds the slot it was drawn in once it arrives, or gives it back when it does not.
    const dropped = droppedTab;
    if (!dropped) return;
    if (tabs.holds(dropped.id)) {
      untrack(resetIncoming);
      return;
    }
    const timer = setTimeout(() => slideLayout(resetIncoming, dropped.id), TAB_ARRIVAL_TIMEOUT_MS);
    return () => clearTimeout(timer);
  });
</script>

<svelte:document
  ondragenter={(event) => trackDragDepth(event, 1)}
  ondragleave={(event) => trackDragDepth(event, -1)}
/>

{#snippet tabFace(tab: WorkspaceTab, iconOnly: boolean)}
  {@const Icon = workspaceTabKindIcon(tab.location.kind)}
  {@const project = workspaceTabProjectHint(tab)}
  <Icon class="shrink-0" size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
  {#if !iconOnly}
    <span class="titlebar-tab-label min-w-0 flex-1 truncate text-start">
      {workspaceTabLabel(tab, t)}{#if project}<span class="opacity-60"> {t("titleBar.tab.projectHint", shortWorkspaceTabProject(project))}</span>{/if}
    </span>
  {/if}
{/snippet}

<div
  class="flex min-w-0 shrink items-center gap-0.5 pl-1.5"
  style:--tab-slide={`${TAB_SLIDE_MS}ms`}
  style:--tab-ease={TAB_EASE}
>
  <div
    bind:this={strip}
    class={cn(
      "relative flex min-w-0 items-center gap-0.5",
      // A dragged tab may slide past the last slot, and stays drawn there until it settles.
      stripInMotion ? "overflow-visible" : "overflow-hidden",
    )}
    role="tablist"
    aria-orientation="horizontal"
  >
    {#if indicatorStyle && !tabsDrawHighlight}
      <div
        class={cn(
          "tab-indicator absolute top-0 h-full rounded-md bg-card shadow-(--tab-active-shadow)",
          activeSplitSide === 0 && "rounded-r-none",
          activeSplitSide === 1 && "rounded-l-none",
        )}
        style={indicatorStyle}
      ></div>
    {/if}
    {#each shownTabs as tab, index (tab.id)}
      {@const arriving = tab.id === incomingTab?.id}
      {@const active = !arriving && tab.id === tabs.activeTabId}
      {@const fullLabel = workspaceTabFullLabel(tab, t)}
      {@const source = nativeDrag?.tab.id === tab.id}
      {@const moving = arriving || heldId === tab.id}
      {@const lifted = !arriving && (!tabs.holds(tab.id) || (moving && draggedTabLifted) || departingId === tab.id)}
      {@const settling = settlingId === tab.id}
      {@const hovered = hoveredTabId === tab.id}
      {@const side = moving ? null : splitSide(tab.id)}
      {@const partner = !moving && tab.id === partnerId}
      {@const name = tab.locked ? t("titleBar.tab.lockedTab", fullLabel) : fullLabel}
      <div
        bind:this={tabElements[tab.id]}
        data-tab-id={tab.id}
        data-active={active ? "" : undefined}
        class={cn(
          "titlebar-tab relative z-1 flex items-center rounded-md text-sm font-medium",
          compact && "titlebar-tab-compact",
          activeOnly && !active && !moving && !partner ? "hidden" : "",
          // A split's tabs join into one piece, each with its inner corners squared.
          side === 0 && "rounded-r-none",
          side === 1 && "rounded-l-none",
          // The highlight passes between the indicator and the tab itself at once, so it never fades in or out.
          active || moving
            ? "text-foreground dark:text-white"
            : partner
              ? cn("text-foreground transition-colors dark:text-white", hovered ? "bg-card/75" : "bg-card/50")
              : cn(
                "transition-colors",
                hovered
                  ? "bg-sidebar-accent text-sidebar-accent-foreground"
                  : cn("text-sidebar-foreground/70", side !== null && "bg-sidebar-accent/50"),
              ),
          (active || moving) && tabsDrawHighlight && "bg-card shadow-(--tab-active-shadow)",
          moving && "titlebar-tab-moving",
          lifted && "titlebar-tab-lifted",
          settling && "titlebar-tab-settling",
        )}
        style:translate={moving && !lifted ? `${arriving ? incomingOffset : movingOffset}px 0` : undefined}
        draggable={!arriving || source}
        inert={arriving && !source}
        role="presentation"
        title={tabTitle(tab, index)}
        ondragstart={(event) => handleDragStart(event, tab)}
        ondragend={handleDragEnd}
        oncontextmenu={(event) => onOpenContextMenu(event, tab.id)}
        onmousedown={(event) => handleMouseDown(event, tab)}
        onauxclick={(event) => handleAuxClick(event, tab)}
      >
        <button
          type="button"
          role="tab"
          aria-selected={active}
          aria-labelledby={`titlebar-tab-name-${tab.id}`}
          class={cn(
            "titlebar-tab-button flex h-full min-w-0 flex-1 items-center rounded-md",
            compact ? "justify-center" : "gap-1.5 pl-3 pr-1",
          )}
          onclick={() => tabs.activate(tab.id)}
        >
          <!-- Named by a label rather than aria-label, so the tooltip centers on the whole tab, close button included. -->
          <span id={`titlebar-tab-name-${tab.id}`} class="sr-only">{tabs.splitOf(tab.id) ? t("titleBar.split.inSplit", name) : name}</span>
          {@render tabFace(tab, compact)}
          {#if !compact && tab.locked}<Lock class="titlebar-tab-lock shrink-0 opacity-60" size={11} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />{/if}
        </button>
        {#if !compact}
          <button
            type="button"
            tabindex="-1"
            class={cn(
              "titlebar-tab-close mr-1 flex shrink-0 items-center justify-center rounded-sm transition-opacity",
              hovered && "hover:bg-sidebar-accent hover:opacity-100",
              active || hovered ? "opacity-70" : "opacity-0",
            )}
            aria-label={t("titleBar.tab.closeTab")}
            title={t("titleBar.tab.withShortcut", t("titleBar.tab.closeTab"), formatShortcut("Mod + W"))}
            onclick={(event) => closeFromButton(event, tab)}
          >
            <X size={12} strokeWidth={1.75} />
          </button>
        {/if}
      </div>
    {/each}
  </div>

  {#if canOpenTabs}
    <button
      bind:this={addButton}
      type="button"
      class={cn(
        "titlebar-add flex shrink-0 items-center justify-center rounded-md text-sidebar-foreground/70",
        addOffset > 0 && "titlebar-add-pushed",
        (addHovered || launcherOpen) && "bg-sidebar-accent text-sidebar-accent-foreground",
      )}
      style:translate={addOffset > 0 ? `${addOffset}px 0` : undefined}
      aria-label={t("titleBar.tab.launcher")}
      aria-haspopup="menu"
      aria-expanded={launcherOpen}
      title={t("titleBar.tab.withShortcut", t("titleBar.tab.newTab"), formatShortcut("Mod + T"))}
      onclick={() => { launcherOpen = !launcherOpen; }}
    >
      <Plus size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
    </button>
  {/if}
</div>

{#if floating}
  <div
    class={cn(
      "titlebar-tab-floating pointer-events-none fixed z-60 flex h-7 items-center rounded-md bg-card text-sm font-medium text-foreground shadow-(--tab-active-shadow) dark:text-white",
      compact ? "justify-center" : "gap-1.5 px-3",
    )}
    style={floating.style}
    aria-hidden="true"
  >
    {@render tabFace(floating.tab, compact)}
  </div>
{/if}

{#if launcherOpen && canOpenTabs}
  <TitleBarTabLauncher
    anchor={addButton}
    showOpenTabs={activeOnly}
    onClose={() => { launcherOpen = false; }}
  />
{/if}

<style>
  /*
   * Browser-like sizing: every tab starts at the same width and all of them shrink evenly when the bar runs out
   * of room. Each tab is a size container, so its own width decides what it can still show.
   */
  .titlebar-tab {
    container-type: inline-size;
    flex: 0 1 auto;
    width: 12.5rem;
    min-width: 2.25rem;
    height: 28px;
  }

  .titlebar-tab.titlebar-tab-compact {
    width: 28px;
    min-width: 28px;
  }

  @container (width < 6rem) {
    .titlebar-tab:not([data-active]) .titlebar-tab-close {
      display: none;
    }
  }

  @container (width < 4.5rem) {
    .titlebar-tab-label,
    .titlebar-tab :global(.titlebar-tab-lock) {
      display: none;
    }

    .titlebar-tab-button {
      justify-content: center;
      padding-left: 0;
    }
  }

  @container (width < 3.5rem) {
    .titlebar-tab-close {
      display: none;
    }

    .titlebar-tab-button {
      padding-right: 0;
    }
  }

  .titlebar-tab-close {
    width: 18px;
    height: 18px;
  }

  /* The new tab button eases back to its place once a tab dragged past the last slot stops pushing it. */
  .titlebar-add {
    width: 28px;
    height: 28px;
    transition: color 150ms ease, background-color 150ms ease, translate var(--tab-slide) var(--tab-ease);
  }

  .titlebar-add.titlebar-add-pushed {
    transition: color 150ms ease, background-color 150ms ease;
  }

  .tab-indicator {
    transition: none;
  }

  /* The dragged tab rides above its neighbors, following the pointer without easing. */
  .titlebar-tab.titlebar-tab-moving {
    z-index: 2;
  }

  .titlebar-tab-moving .titlebar-tab-close {
    visibility: hidden;
  }

  /* Dropped on the bar, the tab eases from where it was drawn into its slot. */
  .titlebar-tab.titlebar-tab-settling {
    z-index: 2;
    transition: translate var(--tab-slide) var(--tab-ease);
  }

  /* Pulled off the bar, the tab gives its slot back and the strip closes around it. */
  .titlebar-tab.titlebar-tab-lifted {
    width: 0;
    min-width: 0;
    margin-right: -0.125rem;
    overflow: hidden;
    visibility: hidden;
  }

  @media (prefers-reduced-motion: reduce) {
    .titlebar-add,
    .titlebar-tab.titlebar-tab-settling {
      transition: none;
    }
  }

  .titlebar-tab-floating {
    max-width: 12.5rem;
  }

  :global(html[data-focus-intent="keyboard"]) .titlebar-tab-button:focus {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }
</style>
