<script lang="ts">
  import { flushSync, tick, type Snippet } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Check from "@lucide/svelte/icons/check";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalKeyboardLayer } from "$lib/modal-focus";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import ShortcutDescription from "./ShortcutDescription.svelte";
  import {
    pickSelectPopoverGeometry,
    type SelectPopoverGeometry,
    type SelectPopoverHorizontalAlign,
    type SelectPopoverRect,
  } from "$lib/utils/select-popover-position";

  interface Option {
    value: string;
    label: string;
    summary?: string;
    /** Keep unavailable choices visible without allowing selection. */
    disabled?: boolean;
    /** Optional inline style applied to the option label (e.g. for font previews). */
    style?: string;
  }

  let {
    value,
    options,
    onChange,
    label,
    description,
    descriptionShortcuts = [],
    ariaLabel,
    canReset = false,
    onReset,
    inline = false,
    showSelectedSummary = true,
    showActiveCheck = true,
    alignOptionSummaryEnd = false,
    popoverAlign = "start",
    popoverBoundaryElement = null,
    disabled = false,
    unavailable = false,
    appearance = "default",
    class: className = "",
    triggerLabel,
    leading,
    searchPlaceholder,
    searchValue = "",
    onSearchChange,
    emptyLabel,
    contentAlign = "end",
    triggerProps = {},
    textSize = "compact",
  }: {
    value: string;
    options: readonly Option[];
    onChange: (value: string) => void;
    label?: string;
    description?: string;
    descriptionShortcuts?: readonly string[];
    ariaLabel?: string;
    canReset?: boolean;
    onReset?: () => void;
    inline?: boolean;
    showSelectedSummary?: boolean;
    showActiveCheck?: boolean;
    alignOptionSummaryEnd?: boolean;
    popoverAlign?: SelectPopoverHorizontalAlign;
    popoverBoundaryElement?: HTMLElement | null;
    disabled?: boolean;
    /** Keeps the final look of a control whose behavior is not implemented yet; see `control-unavailable`. */
    unavailable?: boolean;
    appearance?: "default" | "quiet";
    class?: string;
    /** Label for an action picker that does not retain a selected value. */
    triggerLabel?: string;
    /** Optional visual shared by the selected value and each menu option. */
    leading?: Snippet<[string]>;
    /** Enables a search field inside the floating menu. */
    searchPlaceholder?: string;
    searchValue?: string;
    /** Supply this for bounded or asynchronous searches owned by the caller. */
    onSearchChange?: (query: string) => void;
    emptyLabel?: string;
    contentAlign?: "start" | "end";
    /** Native trigger metadata and focus/navigation handlers for labels and editable grids. */
    triggerProps?: Omit<HTMLButtonAttributes, "children" | "onclick" | "class" | "disabled">;
    /** Text size of the trigger and menu: compact for settings controls, collection to match collection tables and panels. */
    textSize?: "compact" | "collection";
  } = $props();

  const { t } = getLocalization();
  const textClass = $derived(textSize === "collection" ? "text-collection" : "text-[0.8rem]");

  const ESTIMATED_DROPDOWN_HEIGHT = 240;
  const DEFAULT_POPOVER_GEOMETRY: SelectPopoverGeometry = {
    top: 0,
    left: 0,
    width: null,
    minWidth: 0,
    maxWidth: 0,
    maxHeight: 0,
    placement: "below",
  };

  let open = $state(false);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let popoverEl: HTMLDivElement | undefined = $state();
  let scrollEl: HTMLDivElement | undefined = $state();
  let popoverGeometry = $state<SelectPopoverGeometry>(DEFAULT_POPOVER_GEOMETRY);
  let popoverReady = $state(false);

  const menuId = $props.id();
  let localSearch = $state("");
  const query = $derived(onSearchChange ? searchValue : localSearch);
  const visibleOptions = $derived(onSearchChange || !searchPlaceholder
    ? options
    : options.filter((option) => option.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())));
  const current = $derived(options.find((o) => o.value === value));

  function toRect(rect: DOMRect): SelectPopoverRect {
    return {
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left,
      width: rect.width,
      height: rect.height,
    };
  }

  function getBoundaryRect(): SelectPopoverRect {
    if (!triggerEl) {
      return {
        top: 0,
        right: window.innerWidth,
        bottom: window.innerHeight,
        left: 0,
        width: window.innerWidth,
        height: window.innerHeight,
      };
    }
    const boundaryEl = popoverBoundaryElement
      ?? triggerEl.closest<HTMLElement>("[data-settings-content]")
      ?? triggerEl.closest<HTMLElement>("[data-settings-modal-panel]");
    const viewportRect: SelectPopoverRect = {
      top: 0,
      right: window.innerWidth,
      bottom: window.innerHeight,
      left: 0,
      width: window.innerWidth,
      height: window.innerHeight,
    };
    if (!boundaryEl) return viewportRect;
    const boundary = boundaryEl.getBoundingClientRect();
    const top = Math.max(viewportRect.top, boundary.top);
    const right = Math.min(viewportRect.right, boundary.right);
    const bottom = Math.min(viewportRect.bottom, boundary.bottom);
    const left = Math.max(viewportRect.left, boundary.left);
    return {
      top,
      right,
      bottom,
      left,
      width: Math.max(0, right - left),
      height: Math.max(0, bottom - top),
    };
  }

  function computePosition() {
    if (!triggerEl) return;
    popoverGeometry = pickSelectPopoverGeometry({
      triggerRect: toRect(triggerEl.getBoundingClientRect()),
      boundaryRect: getBoundaryRect(),
      contentHeight: popoverEl ? popoverEl.offsetHeight - (scrollEl?.clientHeight ?? 0) + (scrollEl?.scrollHeight ?? 0) : ESTIMATED_DROPDOWN_HEIGHT,
      contentWidth: popoverEl?.scrollWidth,
      horizontalAlign: popoverAlign,
    });
    popoverReady = true;
  }

  function popoverStyle(): string {
    if (!popoverReady) {
      return "top: 0px; left: 0px; min-width: max-content; max-width: max-content; max-height: none; visibility: hidden;";
    }
    const width = popoverGeometry.width === null ? "" : ` width: ${popoverGeometry.width}px;`;
    return `top: ${popoverGeometry.top}px; left: ${popoverGeometry.left}px;${width} min-width: ${popoverGeometry.minWidth}px; max-width: ${popoverGeometry.maxWidth}px; max-height: ${popoverGeometry.maxHeight}px; visibility: visible;`;
  }

  /** Open the menu without moving the surrounding scroll position. */
  async function toggle(): Promise<void> {
    if (disabled || unavailable) return;
    if (open) {
      open = false;
      return;
    }
    popoverReady = false;
    open = true;
    await tick();
    computePosition();
    await tick();
    if (!open) return;
    const focusTarget = searchPlaceholder
      ? popoverEl?.querySelector<HTMLInputElement>("input")
      : popoverEl?.querySelector<HTMLButtonElement>('[aria-selected="true"]:not(:disabled)')
        ?? popoverEl?.querySelector<HTMLButtonElement>('[role="option"]:not(:disabled)');
    focusTarget?.focus({ preventScroll: true });
  }

  /** Select a value and return keyboard focus to its trigger. */
  function select(next: string): void {
    if (disabled || options.find((option) => option.value === next)?.disabled) return;
    onChange(next);
    open = false;
    triggerEl?.focus({ preventScroll: true });
  }

  /** Keep menu navigation and dismissal local to the open control. */
  function handleKeydown(e: KeyboardEvent): void {
    if (!open || !(e.target instanceof Node)) return;
    if (!popoverEl?.contains(e.target) && !triggerEl?.contains(e.target)) return;
    if (e.key === "Escape" || e.key === "Tab") {
      // Both keys only close the menu and return focus to the trigger, so neither the browser
      // nor a surrounding modal trap moves focus while the layer still owns the keyboard.
      e.preventDefault();
      e.stopPropagation();
      flushSync(() => { open = false; });
      triggerEl?.focus({ preventScroll: true });
      return;
    }
    const buttons = [...(popoverEl?.querySelectorAll<HTMLButtonElement>('[role="option"]:not(:disabled)') ?? [])];
    const index = buttons.findIndex((button) => button === document.activeElement);
    const inSearch = e.target instanceof HTMLInputElement;
    if (inSearch && (e.key === "Home" || e.key === "End")) return;
    if (e.key === "Enter" && inSearch) {
      e.preventDefault();
      e.stopPropagation();
      buttons[0]?.click();
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
    if (buttons.length === 0) return;
    const next = e.key === "Home" ? 0
      : e.key === "End" ? buttons.length - 1
      : e.key === "ArrowDown" ? (index + 1) % buttons.length
      : (index <= 0 ? buttons.length : index) - 1;
    buttons[next]?.focus({ preventScroll: true });
    const button = buttons[next];
    if (button && scrollEl) {
      const top = button.offsetTop - scrollEl.offsetTop;
      const bottom = top + button.offsetHeight;
      if (top < scrollEl.scrollTop) scrollEl.scrollTop = Math.max(0, top);
      else if (bottom > scrollEl.scrollTop + scrollEl.clientHeight) {
        scrollEl.scrollTop = bottom - scrollEl.clientHeight;
      }
    }
  }

  $effect(() => {
    if (!open) return;
    visibleOptions;
    void tick().then(() => { if (open) computePosition(); });
  });

  $effect(() => {
    if (!open) return;
    function handleClickOutside(e: MouseEvent) {
      const target = e.target;
      if (!(target instanceof Node)) return;
      if (triggerEl?.contains(target)) return;
      if (popoverEl?.contains(target)) return;
      open = false;
    }
    function handleScroll(e: Event) {
      if (e.target instanceof Node && popoverEl?.contains(e.target)) return;
      open = false;
    }
    function handleResize() {
      computePosition();
    }
    // The open menu owns the keyboard as its own layer, so it keeps working inside modal dialogs.
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    window.addEventListener("mousedown", handleClickOutside, true);
    window.addEventListener("scroll", handleScroll, true);
    window.addEventListener("resize", handleResize);
    return () => {
      deactivateKeyboard();
      window.removeEventListener("mousedown", handleClickOutside, true);
      window.removeEventListener("scroll", handleScroll, true);
      window.removeEventListener("resize", handleResize);
    };
  });
</script>


{#snippet selectControl()}
  <div class={cn("relative min-w-0 w-44 max-[480px]:flex-1", className)}>
    <button
      bind:this={triggerEl}
      {...triggerProps}
      type="button"
      {disabled}
      onclick={toggle}
      onkeydown={(event) => {
        if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
          event.preventDefault();
          void toggle();
        }
        triggerProps.onkeydown?.(event);
      }}
      aria-haspopup={searchPlaceholder ? "dialog" : "listbox"}
      aria-expanded={open}
      aria-controls={open ? menuId : undefined}
      aria-disabled={unavailable ? "true" : undefined}
      aria-label={ariaLabel ?? label}
      class={cn(
        "flex h-7 w-full max-w-full items-center gap-2 rounded-md font-medium text-foreground transition-colors disabled:cursor-not-allowed max-[480px]:w-full",
        unavailable && "control-unavailable",
        textClass,
        appearance === "quiet"
          ? "justify-end px-1.5 hover:bg-accent/60 disabled:opacity-45 disabled:hover:bg-transparent"
          : "justify-between border border-border bg-transparent px-2.5 hover:bg-accent disabled:hover:bg-transparent",
      )}
    >
      <span class={cn("flex min-w-0 flex-1 items-center gap-1.5", appearance === "quiet" && contentAlign === "end" && "justify-end text-right")}>
        {#if leading && current}{@render leading(current.value)}{/if}
        <span class="truncate" style={current?.style}>{triggerLabel ?? current?.label ?? value}</span>
        {#if showSelectedSummary && current?.summary}
          <span class="shrink-0 text-[0.733333rem] text-muted-foreground">{current.summary}</span>
        {/if}
      </span>
      <ChevronDown
        size={13}
        strokeWidth={2}
        class={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")}
      />
    </button>
    {#if open}
      <div
        bind:this={popoverEl}
        use:portal={triggerEl?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
        id={menuId}
        role={searchPlaceholder ? "dialog" : "listbox"}
        aria-label={ariaLabel ?? label}
        data-app-floating-surface
        class="surface-floating fixed z-80 flex flex-col overflow-hidden"
        style={popoverStyle()}
      >
        {#if searchPlaceholder}
          <input
            value={query}
            aria-label={searchPlaceholder}
            placeholder={searchPlaceholder}
            class="field-bare min-h-9 w-full flex-none border-b border-border px-3"
            oninput={(event) => {
              localSearch = event.currentTarget.value;
              onSearchChange?.(event.currentTarget.value);
            }}
          />
        {/if}
        <div bind:this={scrollEl} use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-x-hidden overflow-y-auto" role={searchPlaceholder ? "listbox" : undefined} aria-label={searchPlaceholder ? (ariaLabel ?? label) : undefined}>
        {#each visibleOptions as option (option.value)}
          {@const isActive = option.value === value}
          <button
            type="button"
            role="option"
            aria-selected={isActive}
            disabled={option.disabled}
            onclick={() => select(option.value)}
            class="menu-item justify-between gap-3"
          >
            <span
              class={cn(
                "min-w-0 flex-1",
                alignOptionSummaryEnd
                  ? "grid grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4"
                  : "flex items-center gap-1.5",
              )}
            >
              {#if leading}{@render leading(option.value)}{/if}
              <span class="truncate" style={option.style}>{option.label}</span>
              {#if option.summary}
                <span class="max-w-72 truncate justify-self-end text-panel-detail text-muted-foreground" title={option.summary}>{option.summary}</span>
              {/if}
            </span>
            {#if showActiveCheck && isActive}
              <Check size={12} strokeWidth={2.5} class="shrink-0" />
            {/if}
          </button>
        {:else}
          {#if emptyLabel}<p class="px-2 py-1.5 text-muted-foreground">{emptyLabel}</p>{/if}
        {/each}
        </div>
      </div>
    {/if}
  </div>
{/snippet}

{#if inline}
  {@render selectControl()}
{:else}
  <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
    {#if label}
      <div class="min-w-0 flex-1">
        <div class="text-[0.866667rem] text-foreground">{label}</div>
        {#if descriptionShortcuts.length > 0}
          <ShortcutDescription shortcuts={descriptionShortcuts} />
        {:else if description}
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{description}</div>
        {/if}
      </div>
    {/if}
    <div class="flex items-center justify-end gap-1.5 max-[480px]:justify-between">
      {@render selectControl()}
      {#if onReset}
        <button
          onclick={onReset}
          disabled={!canReset}
          aria-label={t("common.reset")}
          data-app-tooltip-disabled="true"
          class={cn(
            "flex h-7 w-7 items-center justify-center rounded-md border border-border bg-secondary text-secondary-foreground transition-colors",
            canReset
              ? "hover:bg-accent hover:text-accent-foreground"
              : "cursor-not-allowed opacity-40",
          )}
        >
          <RotateCcw size={12} strokeWidth={2.25} />
        </button>
      {/if}
    </div>
  </div>
{/if}
