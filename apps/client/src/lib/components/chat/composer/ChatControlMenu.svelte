<script lang="ts">
  import { tick } from "svelte";
  import Bot from "@lucide/svelte/icons/bot";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Brain from "@lucide/svelte/icons/brain";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import FilePenLine from "@lucide/svelte/icons/file-pen-line";
  import Folder from "@lucide/svelte/icons/folder";
  import GitPullRequest from "@lucide/svelte/icons/git-pull-request";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import PencilRuler from "@lucide/svelte/icons/pencil-ruler";
  import Settings from "@lucide/svelte/icons/settings";
  import Shield from "@lucide/svelte/icons/shield";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ShieldOff from "@lucide/svelte/icons/shield-off";
  import ShieldQuestionMark from "@lucide/svelte/icons/shield-question-mark";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import {
    pickSelectPopoverGeometry,
    type SelectPopoverGeometry,
    type SelectPopoverRect,
  } from "$lib/utils/select-popover-position";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  export type ChatControlIcon =
    | "bot"
    | "book-open"
    | "brain"
    | "file-pen"
    | "folder"
    | "git-pull-request"
    | "messages-square"
    | "pencil-ruler"
    | "shield"
    | "shield-alert"
    | "shield-check"
    | "shield-off"
    | "shield-question-mark"
    | "settings"
    | "sliders";

  export interface ChatControlOption {
    value: string;
    label: string;
    triggerLabel?: string;
    description?: string;
    icon: ChatControlIcon;
    disabled?: boolean;
  }

  let {
    value,
    options,
    placeholder,
    ariaLabel,
    dataField,
    onChange,
    compact = false,
    minimal = false,
    disabled = false,
    showTooltip = true,
  }: {
    value: string;
    options: readonly ChatControlOption[];
    placeholder?: ChatControlOption;
    ariaLabel: string;
    dataField?: string;
    onChange: (value: string) => void;
    compact?: boolean;
    minimal?: boolean;
    disabled?: boolean;
    showTooltip?: boolean;
  } = $props();

  const DEFAULT_GEOMETRY: SelectPopoverGeometry = {
    top: 0,
    left: 0,
    width: null,
    minWidth: 0,
    maxWidth: 0,
    maxHeight: 0,
    placement: "above",
  };

  let open = $state(false);
  let ready = $state(false);
  let trigger: HTMLButtonElement | undefined = $state();
  let popover: HTMLDivElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();
  let geometry = $state<SelectPopoverGeometry>(DEFAULT_GEOMETRY);
  const current = $derived(options.find((option) => option.value === value) ?? placeholder ?? options[0]);

  function rect(value: DOMRect): SelectPopoverRect {
    return {
      top: value.top,
      right: value.right,
      bottom: value.bottom,
      left: value.left,
      width: value.width,
      height: value.height,
    };
  }

  /** Full popover height including content hidden by the inner scroll list, so placement stays correct after a max height applies. */
  function popoverContentHeight(): number | null {
    if (!popover || !list) return null;
    return list.scrollHeight + popover.offsetHeight - popover.clientHeight;
  }

  function computePosition(): void {
    if (!trigger) return;
    geometry = pickSelectPopoverGeometry({
      triggerRect: rect(trigger.getBoundingClientRect()),
      boundaryRect: {
        top: 0,
        right: window.innerWidth,
        bottom: window.innerHeight,
        left: 0,
        width: window.innerWidth,
        height: window.innerHeight,
      },
      contentHeight: popoverContentHeight() ?? 220,
      contentWidth: popover?.scrollWidth ?? 260,
      horizontalAlign: "start",
    });
    ready = true;
  }

  function popoverStyle(): string {
    if (!ready) return "visibility:hidden;top:0;left:0;";
    return `visibility:visible;top:${geometry.top}px;left:${geometry.left}px;min-width:${Math.max(220, geometry.minWidth)}px;max-width:${geometry.maxWidth}px;max-height:${geometry.maxHeight}px;`;
  }

  async function toggle(): Promise<void> {
    if (disabled) return;
    if (open) {
      open = false;
      return;
    }
    ready = false;
    open = true;
    await tick();
    computePosition();
    popover?.querySelector<HTMLButtonElement>("[aria-selected='true']")?.focus();
  }

  function select(option: ChatControlOption): void {
    if (option.disabled) return;
    onChange(option.value);
    open = false;
    queueMicrotask(() => trigger?.focus());
  }

  function handleTriggerKeydown(event: KeyboardEvent): void {
    if (!["ArrowDown", "ArrowUp"].includes(event.key)) return;
    event.preventDefault();
    void toggle();
  }

  function handlePopoverKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      open = false;
      queueMicrotask(() => trigger?.focus());
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const buttons = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    if (buttons.length === 0) return;
    event.preventDefault();
    const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex = event.key === "Home"
      ? 0
      : event.key === "End"
        ? buttons.length - 1
        : (Math.max(0, currentIndex) + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
    buttons[nextIndex]?.focus();
  }

  $effect(() => {
    if (!open) return;
    const handleOutsidePointer = (event: MouseEvent) => {
      if (!(event.target instanceof Node)) return;
      if (trigger?.contains(event.target) || popover?.contains(event.target)) return;
      open = false;
    };
    const handleViewportChange = () => computePosition();
    window.addEventListener("mousedown", handleOutsidePointer, true);
    window.addEventListener("resize", handleViewportChange);
    return () => {
      window.removeEventListener("mousedown", handleOutsidePointer, true);
      window.removeEventListener("resize", handleViewportChange);
    };
  });
</script>

{#snippet controlIcon(icon: ChatControlIcon, size = 14)}
  {#if icon === "bot"}<Bot {size} />
  {:else if icon === "book-open"}<BookOpen {size} />
  {:else if icon === "brain"}<Brain {size} />
  {:else if icon === "file-pen"}<FilePenLine {size} />
  {:else if icon === "folder"}<Folder {size} />
  {:else if icon === "git-pull-request"}<GitPullRequest {size} />
  {:else if icon === "messages-square"}<MessagesSquare {size} />
  {:else if icon === "pencil-ruler"}<PencilRuler {size} />
  {:else if icon === "settings"}<Settings {size} />
  {:else if icon === "shield-alert"}<ShieldAlert {size} />
  {:else if icon === "shield-check"}<ShieldCheck {size} />
  {:else if icon === "shield-off"}<ShieldOff {size} />
  {:else if icon === "shield-question-mark"}<ShieldQuestionMark {size} />
  {:else if icon === "sliders"}<SlidersHorizontal {size} />
  {:else}<Shield {size} />{/if}
{/snippet}

<button
  bind:this={trigger}
  type="button"
  class="control-trigger"
  class:compact
  class:minimal
  {disabled}
  aria-label={ariaLabel}
  aria-haspopup="listbox"
  aria-expanded={open}
  data-chat-field={dataField}
  onclick={() => void toggle()}
  onkeydown={handleTriggerKeydown}
  title={showTooltip ? current?.description ?? ariaLabel : undefined}
  data-app-tooltip-disabled={showTooltip ? undefined : "true"}
>
  {@render controlIcon(current?.icon ?? "sliders")}
  <span class="control-label">{current?.triggerLabel ?? current?.label ?? ariaLabel}</span>
  {#if !minimal}<ChevronDown size={12} class={open ? "open" : ""} />{/if}
</button>

{#if open}
  <div
    bind:this={popover}
    use:portal
    class="control-popover surface-floating flex flex-col overflow-hidden"
    class:below={geometry.placement === "below"}
    class:positioned={ready}
    role="listbox"
    tabindex="-1"
    aria-label={ariaLabel}
    data-app-floating-surface
    style={popoverStyle()}
    onkeydown={handlePopoverKeydown}
  >
    <div bind:this={list} class="surface-floating-body min-h-0 flex-1 overflow-y-auto" use:scrollEdgeFadeAction>
      {#each options as option (option.value)}
        <button
          type="button"
          role="option"
          class="menu-item"
          aria-selected={option.value === value}
          disabled={option.disabled}
          onclick={() => select(option)}
        >
          <span class="option-icon">{@render controlIcon(option.icon, 15)}</span>
          <span class="option-copy"><span class="option-title">{option.label}</span>{#if option.description}<span class="option-description text-panel-detail">{option.description}</span>{/if}</span>
          <Check size={13} class={option.value === value ? "visible" : ""} />
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .control-popover :global(svg.lucide) { stroke-width: var(--icon-stroke-width) !important; }
  .control-trigger { display: inline-flex; min-width: 0; height: 1.9rem; max-width: 10rem; flex: 0 1 auto; align-items: center; gap: 0.4rem; border-radius: 0.55rem; padding: 0.25rem 0.45rem; color: var(--muted-foreground); font-size: calc(0.766667rem * var(--type-scale)); white-space: nowrap; }
  .control-trigger:hover, .control-trigger[aria-expanded="true"] { background: var(--accent); color: var(--foreground); }
  .control-trigger.minimal { padding-inline: 0.35rem; }
  .control-trigger:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .control-trigger:disabled { cursor: not-allowed; opacity: 0.45; }
  .control-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .control-trigger :global(svg:last-child) { flex: 0 0 auto; transition: transform 120ms ease; }
  .control-trigger :global(svg.open:last-child) { transform: rotate(180deg); }
  .control-popover { position: fixed; z-index: 80; }
  .control-popover.positioned { animation: control-popover-enter 180ms cubic-bezier(0.22, 0.75, 0.18, 1); }
  .control-popover.below { --control-popover-enter-y: -0.25rem; }
  .option-icon { display: grid; place-items: center; color: inherit; width: 1.5rem; flex: 0 0 auto; }
  .option-copy { min-width: 0; flex: 1 1 0%; }
  .option-title, .option-description { display: block; overflow: hidden; text-overflow: ellipsis; }
  .option-title { font-weight: 500; }
  .option-description { margin-top: 0.1rem; color: var(--muted-foreground); white-space: normal; }
  .control-popover button > :global(svg:last-child) { visibility: hidden; }
  .control-popover button > :global(svg.visible:last-child) { visibility: visible; }
  @keyframes control-popover-enter { from { opacity: 0; transform: translateY(var(--control-popover-enter-y, 0.25rem)); } to { opacity: 1; transform: translateY(0); } }
  @container chat-composer (max-width: 520px) { .control-trigger.compact { width: 1.9rem; padding-inline: 0; justify-content: center; } .control-trigger.compact .control-label, .control-trigger.compact :global(svg:last-child) { display: none; } }
  @media (prefers-reduced-motion: reduce) { .control-popover.positioned { animation: none; } }
</style>
