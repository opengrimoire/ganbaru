<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { anchoredPanelContentHeight, anchoredSidePanelStyle } from "$lib/utils/anchored-panel";
  import {
    SUBMENU_AIM_TOLERANCES,
    SUBMENU_OPEN_DELAY_MS,
    createHoverDelay,
    isPointerAimingAtSubmenu,
    type MenuAimPoint,
  } from "$lib/utils/menu-aim";
  import { portal } from "$lib/utils/portal";
  import type { WorkspaceTabIcon } from "$lib/workspace-tabs/presentation";

  /**
   * A row of a title bar menu that opens a submenu beside it, on hover, click, ArrowRight, or Enter, following the
   * hover submenu rules in docs/architecture/ui-foundations.md. Rows inside it are `menu-item` buttons with
   * `role="menuitem"`.
   */
  let {
    label,
    icon: Icon,
    disabled = false,
    title,
    children,
  }: {
    label: string;
    icon: WorkspaceTabIcon;
    disabled?: boolean;
    /** Why the row is disabled, shown on hover. */
    title?: string;
    children: Snippet;
  } = $props();

  const ITEM_SELECTOR = "[role='menuitem']:not(:disabled)";

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined = $state();
  let panel: HTMLDivElement | undefined = $state();
  let body: HTMLDivElement | undefined = $state();
  /** Lays the panel out at its natural width, unseen, so it can be measured before it is placed. */
  const MEASURE_STYLE = "position:fixed; left:0; top:0; width:max-content; visibility:hidden";
  let panelStyle = $state(MEASURE_STYLE);
  /** The pointer's last position while a close is pending, to tell whether it is moving toward the submenu. */
  let aimOrigin: MenuAimPoint | null = null;
  const delay = createHoverDelay();

  $effect(() => {
    if (!open || !panel || !body || !trigger) return;
    const surface = panel;
    const rows = body;
    const anchor = trigger;
    const style = window.getComputedStyle(surface);
    const alignOffset = (Number.parseFloat(style.borderTopWidth) || 0)
      + (Number.parseFloat(window.getComputedStyle(rows).paddingTop) || 0);
    panelStyle = anchoredSidePanelStyle({
      triggerRect: anchor.getBoundingClientRect(),
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      preferredWidth: Math.ceil(surface.getBoundingClientRect().width),
      contentHeight: anchoredPanelContentHeight(surface, [rows]),
      alignOffset,
    });
  });

  $effect(() => () => delay.cancel());

  function items(): HTMLElement[] {
    return Array.from(panel?.querySelectorAll<HTMLElement>(ITEM_SELECTOR) ?? []);
  }

  function show(focusFirst: boolean): void {
    if (disabled) return;
    delay.cancel();
    open = true;
    if (focusFirst) void tick().then(() => items()[0]?.focus());
  }

  function hide(refocus: boolean): void {
    delay.cancel();
    aimOrigin = null;
    open = false;
    panelStyle = MEASURE_STYLE;
    if (refocus) trigger?.focus();
  }

  function scheduleClose(event: PointerEvent): void {
    if (!open || event.pointerType !== "mouse") return;
    aimOrigin = { x: event.clientX, y: event.clientY };
    delay.schedule(() => hide(false));
  }

  /** Keep a pending close waiting while the pointer still travels toward the submenu. */
  function handleWindowPointerMove(event: PointerEvent): void {
    if (!open || !aimOrigin || !panel || !trigger || event.pointerType !== "mouse") return;
    const point = { x: event.clientX, y: event.clientY };
    const submenu = panel.getBoundingClientRect();
    const side = submenu.left >= trigger.getBoundingClientRect().right ? "right" : "left";
    if (isPointerAimingAtSubmenu({ origin: aimOrigin, point, submenu, side, ...SUBMENU_AIM_TOLERANCES })) {
      delay.schedule(() => hide(false));
    }
    aimOrigin = point;
  }

  function handleTriggerKeydown(event: KeyboardEvent): void {
    if (event.key !== "ArrowRight" && event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    show(true);
  }

  function handlePanelKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" || event.key === "ArrowLeft") {
      event.preventDefault();
      event.stopPropagation();
      hide(true);
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const rows = items();
    if (rows.length === 0) return;
    const current = rows.findIndex((row) => row === document.activeElement);
    const step = event.key === "ArrowDown" ? 1 : -1;
    rows[(current + step + rows.length) % rows.length]?.focus();
  }
</script>

<svelte:window onpointermove={handleWindowPointerMove} />

<button
  bind:this={trigger}
  type="button"
  role="menuitem"
  class="menu-item"
  {disabled}
  {title}
  aria-haspopup="menu"
  aria-expanded={open}
  onclick={() => (open ? hide(false) : show(false))}
  onkeydown={handleTriggerKeydown}
  onpointerenter={(event) => {
    if (event.pointerType === "mouse" && !open) delay.schedule(() => show(false), SUBMENU_OPEN_DELAY_MS);
    else if (open) delay.cancel();
  }}
  onpointerleave={(event) => {
    if (open) scheduleClose(event);
    else delay.cancel();
  }}
>
  <Icon size={15} strokeWidth={2.2} />
  <span class="min-w-0 flex-1 truncate">{label}</span>
  <ChevronRight class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.5} aria-hidden="true" />
</button>

{#if open}
  <div
    bind:this={panel}
    use:portal
    class="surface-floating z-60 overflow-hidden"
    style={panelStyle}
    role="menu"
    tabindex="-1"
    aria-label={label}
    data-app-floating-surface
    onpointerenter={() => {
      delay.cancel();
      aimOrigin = null;
    }}
    onpointerleave={scheduleClose}
    onkeydown={handlePanelKeydown}
  >
    <div bind:this={body} class="surface-floating-body">
      {@render children()}
    </div>
  </div>
{/if}
