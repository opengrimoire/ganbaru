<script lang="ts">
  /**
   * Icon button that opens a menu of sidebar orders. With `allowGlobal`, the menu starts with "Use global setting" and
   * reports it as `null`, so a project can drop its own order.
   */
  import { tick } from "svelte";
  import ArrowUpNarrowWide from "@lucide/svelte/icons/arrow-up-narrow-wide";
  import Check from "@lucide/svelte/icons/check";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    SIDEBAR_SORT_ORDER_LABEL_KEYS,
    SIDEBAR_SORT_ORDERS,
    type SidebarSortOrder,
  } from "$lib/utils/sidebar-order";

  type OrderOption = "global" | SidebarSortOrder;

  const VIEWPORT_GAP_PX = 8;
  const TRIGGER_GAP_PX = 4;

  let {
    order,
    override = null,
    allowGlobal = false,
    label,
    open = $bindable(false),
    buttonClass,
    activeClass,
    iconClass,
    iconSize,
    iconStrokeWidth,
    onSelect,
  }: {
    /** Order in effect. */
    order: SidebarSortOrder;
    /** Order set for this scope, or `null` while it follows the global setting. Read only with `allowGlobal`. */
    override?: SidebarSortOrder | null;
    allowGlobal?: boolean;
    /** Accessible name and tooltip of the button and the menu. */
    label: string;
    open?: boolean;
    buttonClass?: string;
    /** Classes added to the button while the menu is open. */
    activeClass?: string;
    iconClass?: string;
    iconSize?: number;
    iconStrokeWidth?: number;
    /** Receives the chosen order, or `null` for the global setting. */
    onSelect: (order: SidebarSortOrder | null) => void;
  } = $props();

  const { t } = getLocalization();
  const options = $derived<readonly OrderOption[]>(allowGlobal ? ["global", ...SIDEBAR_SORT_ORDERS] : SIDEBAR_SORT_ORDERS);
  let hovered = $state<OrderOption | null>(null);
  let menuStyle = $state("");
  let buttonElement = $state<HTMLButtonElement | null>(null);
  let menuElement = $state<HTMLDivElement | null>(null);

  function checked(option: OrderOption): boolean {
    if (!allowGlobal) return order === option;
    return option === "global" ? override === null : override === option;
  }

  function optionLabel(option: OrderOption): string {
    return option === "global" ? t("common.useGlobalSetting") : t(SIDEBAR_SORT_ORDER_LABEL_KEYS[option]);
  }

  /** Separate the global item from the orders, and the orders into name, modified, created, and custom groups. */
  function startsGroup(option: OrderOption, index: number): boolean {
    if (index === 0 || option === "global") return false;
    return options[index - 1] === "global" || SIDEBAR_SORT_ORDERS.indexOf(option) % 2 === 0;
  }

  function select(option: OrderOption): void {
    open = false;
    onSelect(option === "global" ? null : option);
  }

  /** Highlight the option nearest the pointer while it crosses a separator, so the highlight never blinks out. */
  function highlightNearest(event: MouseEvent, before: OrderOption | undefined, after: OrderOption): void {
    const divider = event.currentTarget;
    if (!(divider instanceof HTMLDivElement) || !before) return;
    const rect = divider.getBoundingClientRect();
    hovered = event.clientY < rect.top + rect.height / 2 ? before : after;
  }

  function position(): void {
    if (!buttonElement) return;
    const triggerRect = buttonElement.getBoundingClientRect();
    const availableWidth = Math.max(0, window.innerWidth - VIEWPORT_GAP_PX * 2);
    const width = Math.min(menuElement?.getBoundingClientRect().width ?? 0, availableWidth);
    const maxLeft = Math.max(VIEWPORT_GAP_PX, window.innerWidth - width - VIEWPORT_GAP_PX);
    const left = Math.min(Math.max(VIEWPORT_GAP_PX, triggerRect.left), maxLeft);
    const top = triggerRect.bottom + TRIGGER_GAP_PX;
    const maxHeight = Math.max(0, window.innerHeight - top - VIEWPORT_GAP_PX);
    menuStyle = `left: ${left}px; top: ${top}px; width: max-content; max-width: ${availableWidth}px; max-height: ${maxHeight}px;`;
  }

  function toggle(): void {
    if (open) {
      open = false;
      return;
    }
    hovered = null;
    position();
    open = true;
    void tick().then(position);
  }

  $effect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (buttonElement?.contains(target) || menuElement?.contains(target)) return;
      open = false;
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      open = false;
      buttonElement?.focus();
    };
    window.addEventListener("pointerdown", closeOutside, true);
    window.addEventListener("keydown", closeOnEscape, true);
    window.addEventListener("resize", position);
    return () => {
      window.removeEventListener("pointerdown", closeOutside, true);
      window.removeEventListener("keydown", closeOnEscape, true);
      window.removeEventListener("resize", position);
    };
  });
</script>

<div class="relative inline-flex shrink-0">
  <button
    bind:this={buttonElement}
    type="button"
    class={cn(buttonClass, open && activeClass)}
    aria-label={label}
    aria-expanded={open}
    aria-haspopup="menu"
    data-app-tooltip={label}
    onclick={toggle}
  >
    <ArrowUpNarrowWide class={iconClass} size={iconSize} strokeWidth={iconStrokeWidth} aria-hidden="true" />
  </button>
  {#if open}
    <div
      bind:this={menuElement}
      class="surface-floating fixed z-60 flex flex-col overflow-hidden"
      style={menuStyle}
      role="menu"
      tabindex="-1"
      aria-label={label}
      data-sidebar-order-menu
      onmouseleave={() => {
        hovered = null;
      }}
    >
      <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto" use:scrollEdgeFadeAction>
        {#each options as option, index (option)}
          {#if startsGroup(option, index)}
            <div
              class="menu-separator"
              role="separator"
              onmousemove={(event) => highlightNearest(event, options[index - 1], option)}
            ></div>
          {/if}
          <button
            type="button"
            class={cn("menu-item grid grid-cols-[max-content_1rem]", hovered === option && "bg-accent")}
            role="menuitemradio"
            aria-checked={checked(option)}
            onmouseenter={() => {
              hovered = option;
            }}
            onclick={() => select(option)}
          >
            <span>{optionLabel(option)}</span>
            {#if checked(option)}<Check class="size-4" />{/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>
