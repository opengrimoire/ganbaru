<script lang="ts">
  import { tick, untrack, type Component, type Snippet } from "svelte";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { SUBMENU_CLOSE_DELAY_MS, SUBMENU_OPEN_DELAY_MS } from "$lib/utils/menu-aim";
  import { anchoredPanelContentHeight, anchoredPanelStyle, anchoredPanelWidth, anchoredSidePanelStyle, anchoredSubmenuWidth } from "$lib/utils/anchored-panel";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import ArrowDownUp from "@lucide/svelte/icons/arrow-down-up";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Plus from "@lucide/svelte/icons/plus";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Group from "@lucide/svelte/icons/group";
  import X from "@lucide/svelte/icons/x";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import CollectionPanel from "./CollectionPanel.svelte";
  import { getCollectionSettingsNavigation } from "./collection-settings-context";
  import { COLLECTION_ACTION_PANEL_WIDTH, COLLECTION_PANEL_WIDTH } from "./collection-panel-width";

  /**
   * A collection toolbar menu, settings row, or column menu.
   * A full-width row inside another floating menu is a submenu: it opens beside its row on hover or ArrowRight instead of covering it.
   */
  let { label, ariaLabel, kind = "layout", iconOnly = false, fullWidth = false, primary = false, showHeader, activeCount = 0, dismissOnAction = false, summary, leading, icon, rootClass, triggerClass, triggerAttributes, triggerElement = $bindable(), pressed, contextMenu = false, beforeOpen, disabled = false, onOpenChange, children }: {
    label: string;
    ariaLabel?: string;
    kind?: "layout" | "filter" | "sort" | "properties" | "property" | "group" | "actions" | "new" | "new-options";
    iconOnly?: boolean;
    fullWidth?: boolean;
    primary?: boolean;
    showHeader?: boolean;
    activeCount?: number;
    dismissOnAction?: boolean;
    summary?: string;
    leading?: Snippet;
    icon?: Component;
    /** Classes for the element that wraps the trigger, such as to keep a toolbar tab from shrinking. */
    rootClass?: string;
    triggerClass?: string;
    /** Data attributes for the trigger, such as the markers that let collection tables own its highlight. */
    triggerAttributes?: Partial<Record<`data-${string}`, string>>;
    /** The trigger button, such as to anchor a draft that an action in this menu opens after the menu closes. */
    triggerElement?: HTMLButtonElement;
    /** Exposes the trigger as a pressed toggle, such as the current tab of a view switcher. */
    pressed?: boolean;
    /** Also opens the panel from the trigger's context menu, which a right click or a touch long press raises. */
    contextMenu?: boolean;
    /**
     * Runs before the panel opens from a trigger press or its context menu. Returning false keeps it closed,
     * such as when a press only selects an inactive view tab.
     */
    beforeOpen?: (source: "press" | "context") => boolean;
    disabled?: boolean;
    /** Called when the panel opens or closes, such as to load content that is shown only while open. */
    onOpenChange?: (open: boolean) => void;
    children: Snippet;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const navigation = getCollectionSettingsNavigation();
  const settingsRow = $derived(fullWidth && navigation !== undefined);
  const rowTrigger = $derived(settingsRow || (fullWidth && !leading));
  const countLabel = $derived(activeCount > 0 ? formatNumber(localization.locale, activeCount) : "");
  const id = $props.id();
  const VIEWPORT_HEIGHT_FRACTION = 0.7;
  /** How much narrower a submenu is than the menu it opens from. */
  const SUBMENU_WIDTH_INSET = 32;
  /** Dispatched on the parent menu when one of its submenus opens, so sibling submenus close. */
  const SUBMENU_OPEN_EVENT = "collection-submenu-open";
  /** Parent menu controls whose hover takes the highlight from an open submenu's row; padding, dividers, and labels do not. */
  const HOVER_TARGET_SELECTOR = "button, a[href], input, select, textarea, [role^='menuitem']";
  const icons = { layout: SlidersHorizontal, filter: ListFilter, sort: ArrowDownUp, properties: Columns3, property: Columns3, group: Group, actions: Ellipsis, new: Plus, "new-options": ChevronDown };
  const Icon = $derived(icon ?? icons[kind]);
  const dismisses = $derived(kind === "actions" || dismissOnAction);
  let open = $state(false);
  // A local trigger keeps panel placement independent of the binding a parent may rebuild on reload.
  let trigger: HTMLButtonElement | undefined = $state();
  $effect(() => { triggerElement = trigger; });
  let panel: HTMLDivElement | null = $state(null);
  const expanded = $derived(settingsRow ? navigation?.isActive(trigger) ?? false : open);
  const submenu = $derived(rowTrigger && !settingsRow && Boolean(trigger?.parentElement?.closest("[data-app-floating-surface]")));
  const headerVisible = $derived(showHeader ?? (!submenu && kind !== "actions" && kind !== "property"));
  /** Set while a submenu was opened by hover and keyboard focus has not moved into it. */
  let hoverOpened = false;
  /** Set while the mouse rests on another control of the parent menu, so an open submenu's row drops its highlight and only the hovered control shows one. */
  let pointerElsewhere = $state(false);
  let openTimer: ReturnType<typeof setTimeout> | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  let reportedOpen = false;
  /** Set by `show()` until the menu or page can open. */
  let showRequested = $state(false);

  $effect(() => {
    if (!panel) return;
    const floating = floatPanel(panel);
    return () => floating.destroy();
  });

  $effect(() => {
    const value = open;
    if (value === reportedOpen) return;
    reportedOpen = value;
    untrack(() => onOpenChange?.(value));
  });

  $effect(() => {
    if (!showRequested || disabled || !trigger) return;
    showRequested = false;
    untrack(() => {
      if (!expanded) toggle();
    });
  });

  $effect(() => () => {
    clearTimeout(openTimer);
    cancelHoverClose();
  });

  /** Open a detail page within settings or a separate toolbar popover. */
  function toggle(): void {
    if (disabled) return;
    if (settingsRow && navigation && trigger) {
      navigation.navigate({ label, children, trigger });
      return;
    }
    clearTimeout(openTimer);
    if (submenu && open && hoverOpened) {
      enterSubmenu();
      return;
    }
    hoverOpened = false;
    if (!open && beforeOpen?.("press") === false) return;
    open = !open;
  }

  /**
   * Open this menu, or its settings page, unless it is already shown. A request made while the row is disabled or
   * its trigger is still mounting waits until it can open, so a caller that opens settings and this page together
   * does not depend on render order.
   */
  export function show(): void {
    showRequested = true;
  }

  /** Open a toolbar menu from the trigger's context menu instead of the browser menu. */
  function openFromContextMenu(event: MouseEvent): void {
    if (!contextMenu) return;
    event.preventDefault();
    event.stopPropagation();
    if (disabled || open || settingsRow || submenu || beforeOpen?.("context") === false) return;
    clearTimeout(openTimer);
    hoverOpened = false;
    open = true;
  }

  /** Open a submenu, or move focus into it when hover already opened it. */
  function enterSubmenu(): void {
    if (disabled) return;
    clearTimeout(openTimer);
    hoverOpened = false;
    if (open && panel) focusFirst(panel);
    else open = true;
  }

  /** Open a submenu after the mouse rests on its row, unless focus is in a sibling submenu the person is using. */
  function hoverTrigger(event: PointerEvent): void {
    if (!submenu || disabled || event.pointerType !== "mouse") return;
    cancelHoverClose();
    if (open) return;
    const parentSurface = trigger?.parentElement?.closest("[data-app-floating-surface]");
    const focusedSurface = document.activeElement?.closest("[data-app-floating-surface]");
    if (focusedSurface && focusedSurface !== parentSurface && parentSurface?.contains(focusedSurface)) return;
    clearTimeout(openTimer);
    openTimer = setTimeout(() => {
      if (open || disabled) return;
      hoverOpened = true;
      open = true;
    }, SUBMENU_OPEN_DELAY_MS);
  }

  function cancelHoverClose(): void {
    clearTimeout(closeTimer);
    closeTimer = undefined;
  }

  /** Focus the panel's marked initial control, or else its first enabled field or action. */
  function focusFirst(node: HTMLElement): void {
    const target = node.querySelector<HTMLElement>("[data-collection-menu-body] [data-collection-menu-initial-focus]:not(:disabled)")
      ?? node.querySelector<HTMLElement>("[data-collection-menu-body] input:not(:disabled), [data-collection-menu-body] button:not(:disabled)");
    (target ?? node).focus({ preventScroll: true });
  }

  /** Close the panel and return keyboard focus to the invoking control. */
  function close(): void {
    open = false;
    trigger?.focus({ preventScroll: true });
  }

  /** Whether an action closes this panel: it is in the panel itself, or in nested menus that all close after their own actions. */
  function actionReachesPanel(target: Element, node: HTMLElement): boolean {
    let surface = target.closest("[data-app-floating-surface]");
    while (surface && surface !== node) {
      if (!surface.hasAttribute("data-collection-menu-dismiss")) return false;
      surface = surface.parentElement?.closest("[data-app-floating-surface]") ?? null;
    }
    return surface === node;
  }

  /** Whether arrow keys edit text in this element instead of moving between menus. */
  function editsText(target: Element): boolean {
    return target.matches("textarea, select, [contenteditable], input:not([type='checkbox']):not([type='radio']):not([type='button'])");
  }

  /** Keep settings outside clipped rows, with nested dropdowns inside the owning dialog. */
  function floatPanel(node: HTMLDivElement) {
    const moved = portal(node, trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body");
    const content = node.querySelector<HTMLElement>("[data-collection-menu-content]");
    const parentSurface = submenu ? trigger?.parentElement?.closest<HTMLElement>("[data-app-floating-surface]") ?? null : null;
    let disposed = false;
    let dismissed = false;
    let restoreFocus = true;
    let lastOwnedFocus: HTMLElement | null = null;
    let previousStyle = "";
    /** Distance from the panel edge to its first row, so a submenu's first row lines up with its trigger. */
    const firstRowOffset = () => (Number.parseFloat(window.getComputedStyle(node).borderTopWidth) || 0)
      + (content ? Number.parseFloat(window.getComputedStyle(content).paddingTop) || 0 : 0);
    const place = () => {
      if (disposed || !trigger) return;
      const preferredWidth = kind === "actions" || kind === "property" || kind === "new-options" ? COLLECTION_ACTION_PANEL_WIDTH : COLLECTION_PANEL_WIDTH;
      const input = {
        triggerRect: trigger.getBoundingClientRect(),
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight,
        preferredWidth: parentSurface ? anchoredSubmenuWidth(preferredWidth, parentSurface.getBoundingClientRect().width, SUBMENU_WIDTH_INSET) : preferredWidth,
        preferredMaxHeight: window.innerHeight * VIEWPORT_HEIGHT_FRACTION,
      };
      const width = `${Math.round(anchoredPanelWidth(input))}px`;
      if (node.style.width !== width) node.style.width = width;
      const contentHeight = anchoredPanelContentHeight(node, content ? [content] : []);
      const style = parentSurface
        ? anchoredSidePanelStyle({ ...input, contentHeight, alignOffset: firstRowOffset() })
        : anchoredPanelStyle({ ...input, contentHeight });
      if (style !== previousStyle) {
        node.style.cssText = style;
        previousStyle = style;
      }
    };
    const dismiss = () => {
      if (dismissed) return;
      dismissed = true;
      restoreFocus = false;
      const focused = document.activeElement;
      if (focused instanceof HTMLElement && node.contains(focused)) focused.blur();
      open = false;
    };
    const outside = (event: Event) => {
      if (event.target instanceof Node && !node.contains(event.target) && !trigger?.contains(event.target)) dismiss();
    };
    const focusin = (event: FocusEvent) => {
      if (event.target instanceof HTMLElement && event.target.closest("[data-app-floating-surface]") === node) {
        lastOwnedFocus = event.target;
        hoverOpened = false;
      }
      outside(event);
    };
    // A hover-opened submenu closes when the mouse settles elsewhere in its parent menu, unless focus moved into it.
    const hoverAway = (event: PointerEvent) => {
      if (event.pointerType !== "mouse" || !(event.target instanceof Node)) return;
      if (node.contains(event.target) || trigger?.contains(event.target)) {
        pointerElsewhere = false;
        cancelHoverClose();
        return;
      }
      pointerElsewhere = event.target instanceof Element && event.target.closest(HOVER_TARGET_SELECTOR) !== null;
      if (node.contains(document.activeElement) || closeTimer !== undefined) return;
      closeTimer = setTimeout(() => {
        closeTimer = undefined;
        if (!disposed && !node.contains(document.activeElement)) dismiss();
      }, SUBMENU_CLOSE_DELAY_MS);
    };
    const siblingOpened = (event: Event) => {
      if (event instanceof CustomEvent && event.detail !== trigger) dismiss();
    };
    const keydown = (event: KeyboardEvent) => {
      if (dismissed || !(event.target instanceof Element)) return;
      // Nested dropdowns own their first Escape press.
      const lostDisabledFocus = event.key === "Escape"
        && (event.target === document.body || event.target === document.documentElement)
        && (document.activeElement === document.body || document.activeElement === document.documentElement)
        && lastOwnedFocus?.isConnected && node.contains(lastOwnedFocus) && lastOwnedFocus.matches(":disabled")
        && !node.querySelector("[data-app-floating-surface]");
      if (event.target.closest("[data-app-floating-surface]") !== node && !lostDisabledFocus) return;
      if (event.key === "Escape" || (event.key === "ArrowLeft" && parentSurface && !editsText(event.target))) {
        event.preventDefault();
        event.stopPropagation();
        close();
        return;
      }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || !(event.target instanceof HTMLButtonElement)) return;
      if (event.target.hasAttribute("aria-haspopup") && !event.target.hasAttribute("data-collection-submenu-trigger")) return;
      // Secondary controls in a row, such as reorder handles and row action menus, handle the arrow keys themselves.
      if (event.target.hasAttribute("data-collection-menu-row-control")) return;
      const buttons = [...node.querySelectorAll<HTMLButtonElement>("[data-collection-menu-body] button:not(:disabled):not([data-collection-menu-row-control])")]
        .filter((button) => button.closest("[data-app-floating-surface]") === node);
      const index = buttons.indexOf(event.target);
      if (index < 0 || !buttons.length) return;
      event.preventDefault();
      const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
        : event.key === "ArrowDown" ? (index + 1) % buttons.length : (index + buttons.length - 1) % buttons.length;
      buttons[next]?.focus({ preventScroll: true });
    };
    const closeAfterAction = (event: MouseEvent) => {
      if (!dismisses || !(event.target instanceof Element) || !actionReachesPanel(event.target, node)) return;
      const button = event.target.closest("button");
      if (!button || button.disabled || button.hasAttribute("aria-haspopup") || button.hasAttribute("data-collection-menu-keep-open")) return;
      // Close after the click finishes dispatching: browsers run microtasks between listeners, so a microtask here
      // would close the panel before Svelte's delegated action handlers at the app root run.
      setTimeout(() => {
        if (disposed) return;
        if (node.contains(document.activeElement)) close();
        else open = false;
      });
    };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !node.contains(event.target)) place();
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    if (content) observer?.observe(content);
    parentSurface?.dispatchEvent(new CustomEvent(SUBMENU_OPEN_EVENT, { detail: trigger }));
    parentSurface?.addEventListener(SUBMENU_OPEN_EVENT, siblingOpened);
    parentSurface?.addEventListener("pointerover", hoverAway);
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", keydown, true);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", scroll, true);
    document.addEventListener("focusin", focusin);
    node.addEventListener("click", closeAfterAction);
    void tick().then(() => {
      if (disposed) return;
      place();
      if (!hoverOpened) focusFirst(node);
    });
    return {
      destroy() {
        disposed = true;
        cancelHoverClose();
        observer?.disconnect();
        parentSurface?.removeEventListener(SUBMENU_OPEN_EVENT, siblingOpened);
        parentSurface?.removeEventListener("pointerover", hoverAway);
        window.removeEventListener("pointerdown", outside, true);
        window.removeEventListener("keydown", keydown, true);
        window.removeEventListener("resize", place);
        window.removeEventListener("scroll", scroll, true);
        document.removeEventListener("focusin", focusin);
        node.removeEventListener("click", closeAfterAction);
        hoverOpened = false;
        pointerElsewhere = false;
        if (restoreFocus && node.contains(document.activeElement) && trigger?.isConnected) trigger.focus({ preventScroll: true });
        moved.destroy();
      },
    };
  }
</script>

<div class={cn(fullWidth ? "flex w-full" : "inline-flex", "min-w-0", rootClass)} class:collection-menu-row={rowTrigger}>
  <button
    bind:this={trigger}
    type="button"
    {...triggerAttributes}
    {disabled}
    class={cn("collection-menu-trigger inline-flex h-8 min-w-0 items-center gap-1.5 rounded px-2 text-collection font-normal transition-colors focus-visible:outline-none focus-visible:bg-accent disabled:cursor-not-allowed disabled:text-muted-foreground", primary ? "bg-primary text-primary-foreground hover:bg-primary/90" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground", rowTrigger && "justify-start rounded-md text-foreground transition-none hover:bg-accent", pressed === true && !primary && "bg-accent text-foreground", triggerClass)}
    class:w-full={fullWidth}
    class:bg-accent={!primary && ((expanded && !(submenu && pointerElsewhere)) || pressed === true)}
    class:text-foreground={!primary && (expanded || pressed === true || activeCount > 0)}
    data-collection-settings-row={settingsRow ? "" : undefined}
    data-collection-submenu-trigger={submenu ? "" : undefined}
    aria-label={ariaLabel ?? (countLabel ? `${label} (${countLabel})` : label)}
    aria-expanded={expanded}
    aria-controls={open && !settingsRow ? id : undefined}
    aria-haspopup="dialog"
    aria-pressed={pressed}
    onclick={toggle}
    oncontextmenu={contextMenu ? openFromContextMenu : undefined}
    onpointerenter={hoverTrigger}
    onpointerleave={() => clearTimeout(openTimer)}
    onkeydown={(event) => {
      if (submenu) {
        if (event.key === "ArrowRight") {
          event.preventDefault();
          enterSubmenu();
        }
        return;
      }
      if (!expanded && (event.key === "ArrowDown" || event.key === "ArrowUp") && !settingsRow) {
        event.preventDefault();
        toggle();
      }
    }}
  >
    {#if leading}{@render leading()}{:else}<Icon class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />{/if}
    {#if !iconOnly}<span class="truncate">{label}</span>{/if}
    {#if rowTrigger}
      <span class="min-w-0 flex-1 truncate text-right text-muted-foreground">{summary ?? countLabel}</span>
      <ChevronRight class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.5} aria-hidden="true" />
    {:else if countLabel}<span class="rounded bg-accent px-1 text-[0.733333rem] tabular-nums">{countLabel}</span>{/if}
  </button>
  {#if open && !settingsRow}
    <CollectionPanel bind:element={panel} {label} id={id} data-collection-menu-dismiss={dismisses ? "" : undefined} class="fixed z-80">
      <div use:scrollEdgeFadeAction class="min-h-0 overflow-x-hidden overflow-y-auto">
        <div data-collection-menu-content class="@container flow-root h-max p-1.5">
          {#if headerVisible}
            <div class="mb-1.5 flex items-center justify-between gap-2 px-1">
              <span class="font-medium">{label}</span>
              <button type="button" class="collection-menu-control inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("common.close")} onclick={close}>
                <X class="size-3.5" aria-hidden="true" />
              </button>
            </div>
          {/if}
          <div data-collection-menu-body>{@render children()}</div>
        </div>
      </div>
    </CollectionPanel>
  {/if}
</div>

<style>
  @media (any-pointer: coarse) and (any-hover: none) {
    .collection-menu-trigger {
      min-height: var(--panel-row-height);
      min-width: 2.75rem;
    }
  }
</style>
