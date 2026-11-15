<!--
  @component
  The floating formatting toolbar a Chat composer shows over selected text, with Slack's
  formatting actions and shortcuts. It shows for a pointer or keyboard selection on desktop and
  stays hidden for touch selections and in the Android shell, where the platform selection menu
  is the formatting surface. Pressing a button keeps the editor selection.
-->
<script lang="ts">
  import { onMount, tick } from "svelte";
  import Bold from "@lucide/svelte/icons/bold";
  import Code from "@lucide/svelte/icons/code";
  import Italic from "@lucide/svelte/icons/italic";
  import List from "@lucide/svelte/icons/list";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import SquareCode from "@lucide/svelte/icons/square-code";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import TextQuote from "@lucide/svelte/icons/text-quote";
  import type { ChatComposerEditor, ChatComposerFormatState } from "$lib/chat/composer/editor";
  import {
    CHAT_COMPOSER_FORMAT_ACTIONS,
    type ChatComposerFormatActionDefinition,
    type ChatComposerFormatActionId,
  } from "$lib/chat/composer/format-actions";
  import {
    chatFormatAriaKeyShortcut,
    chatFormatToolbarPlacement,
    chatFormatToolbarRovingIndex,
    chatFormatToolbarVisible,
    sameSelection,
    type ChatFormatToolbarInputKind,
    type ChatFormatToolbarPlacement,
  } from "$lib/chat/composer/format-toolbar";
  import type { ChatComposerSelection } from "$lib/chat/composer/rich-text";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { safeViewportRect } from "$lib/utils/keep-in-viewport";
  import { portal } from "$lib/utils/portal";

  let {
    editorRoot,
    editor,
    disabled,
    menuOpen,
    composing,
  }: {
    /** The editor host the toolbar formats. */
    editorRoot: HTMLDivElement | undefined;
    editor: () => ChatComposerEditor | undefined;
    disabled: boolean;
    /** Whether an autocomplete menu is open over the composer. */
    menuOpen: boolean;
    /** Whether an input method composition owns the editor. */
    composing: boolean;
  } = $props();

  const { t } = getLocalization();
  const viewport = getViewport();
  const icons = {
    bold: Bold,
    italic: Italic,
    strikethrough: Strikethrough,
    orderedList: ListOrdered,
    bulletList: List,
    blockquote: TextQuote,
    code: Code,
    codeBlock: SquareCode,
  } satisfies Record<ChatComposerFormatActionId, typeof Bold>;

  let toolbar: HTMLDivElement | undefined = $state();
  let selection = $state<ChatComposerSelection | null>(null);
  let formats = $state<ChatComposerFormatState | null>(null);
  let focused = $state(false);
  let dismissed = $state<ChatComposerSelection | null>(null);
  let lastInput = $state<ChatFormatToolbarInputKind>(viewport.coarsePointer ? "touch" : "pointer");
  let placement = $state<ChatFormatToolbarPlacement | null>(null);
  let activeIndex = $state(0);
  const mobileShell = typeof document !== "undefined" && document.documentElement.dataset.shell === "mobile";

  const visible = $derived(chatFormatToolbarVisible({
    focused,
    disabled,
    selection,
    composing,
    menuOpen,
    dismissed,
    mobileShell,
    lastInput,
  }));

  $effect(() => {
    if (!visible) {
      placement = null;
      return;
    }
    void tick().then(place);
  });

  function focusInside(): boolean {
    const active = document.activeElement;
    return active !== null && (active === editorRoot || (toolbar?.contains(active) ?? false));
  }

  function refresh(): void {
    focused = focusInside();
    const current = editor();
    if (!current || !editorRoot || document.activeElement !== editorRoot) {
      if (!focused) selection = null;
      return;
    }
    const next = current.selection();
    if (dismissed && !sameSelection(dismissed, next)) dismissed = null;
    selection = next;
    formats = current.formatState();
    if (visible) place();
  }

  /** Measures the selected text and moves the toolbar against it, hiding it when it cannot fit. */
  function place(): void {
    if (!visible || !toolbar || !editorRoot) return;
    const range = window.getSelection()?.rangeCount ? window.getSelection()?.getRangeAt(0) : null;
    if (!range || !editorRoot.contains(range.commonAncestorContainer)) {
      if (document.activeElement === editorRoot) placement = null;
      return;
    }
    const safe = safeViewportRect();
    const editorRect = editorRoot.getBoundingClientRect();
    placement = chatFormatToolbarPlacement({
      selection: range.getBoundingClientRect(),
      clip: editorRect,
      viewport: safe,
      width: toolbar.offsetWidth,
      height: toolbar.offsetHeight,
    });
  }

  function dismiss(): void {
    if (selection) dismissed = selection;
  }

  /**
   * Handles a keydown in the editor before the composer does: Escape dismisses a visible toolbar
   * without reaching the composer, and Alt+F10 moves focus into it.
   *
   * @returns Whether the toolbar consumed the event.
   */
  export function handleEditorKeydown(event: KeyboardEvent): boolean {
    lastInput = "keyboard";
    if (!visible) return false;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss();
      return true;
    }
    if (event.key === "F10" && event.altKey && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      event.stopPropagation();
      focusButton(activeIndex);
      return true;
    }
    return false;
  }

  function focusButton(index: number): void {
    activeIndex = index;
    toolbar?.querySelectorAll<HTMLButtonElement>("button")[index]?.focus();
  }

  function returnToEditor(): void {
    const current = editor();
    if (current && selection) current.focus(selection);
    else editorRoot?.focus();
  }

  function handleToolbarKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      returnToEditor();
      return;
    }
    const next = chatFormatToolbarRovingIndex(event.key, activeIndex, CHAT_COMPOSER_FORMAT_ACTIONS.length);
    if (next === null) return;
    event.preventDefault();
    focusButton(next);
  }

  function apply(definition: ChatComposerFormatActionDefinition, index: number): void {
    const current = editor();
    if (!current) return;
    activeIndex = index;
    if (selection && document.activeElement !== editorRoot) current.focus(selection);
    current.applyFormat(definition.action);
    refresh();
  }

  function pressed(definition: ChatComposerFormatActionDefinition): boolean {
    if (!formats) return false;
    return definition.action.kind === "mark"
      ? formats.marks.includes(definition.action.mark)
      : formats.lines[definition.action.format];
  }

  function label(definition: ChatComposerFormatActionDefinition): string {
    return t(`chat.composer.format.${definition.id}`);
  }

  onMount(() => {
    const pointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && editorRoot?.contains(event.target)) lastInput = event.pointerType === "touch" ? "touch" : "pointer";
    };
    const focusChanged = () => queueMicrotask(refresh);
    const scrolled = (event: Event) => {
      if (visible && !(event.target instanceof Node && toolbar?.contains(event.target))) place();
    };
    document.addEventListener("selectionchange", refresh);
    document.addEventListener("pointerdown", pointerDown, true);
    document.addEventListener("focusin", focusChanged);
    document.addEventListener("focusout", focusChanged);
    document.addEventListener("scroll", scrolled, true);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("selectionchange", refresh);
      document.removeEventListener("pointerdown", pointerDown, true);
      document.removeEventListener("focusin", focusChanged);
      document.removeEventListener("focusout", focusChanged);
      document.removeEventListener("scroll", scrolled, true);
      window.removeEventListener("resize", dismiss);
    };
  });
</script>

{#if visible && editorRoot}
  <div
    bind:this={toolbar}
    use:portal={editorRoot.closest<HTMLElement>("[data-floating-root]") ?? "body"}
    class="format-toolbar surface-floating"
    class:placed={placement !== null}
    style:left={placement ? `${placement.left}px` : "0px"}
    style:top={placement ? `${placement.top}px` : "0px"}
    data-app-floating-surface
    data-chat-format-toolbar
    data-side={placement?.side}
    role="toolbar"
    tabindex="-1"
    aria-label={t("chat.composer.format.toolbar")}
    aria-orientation="horizontal"
    onkeydown={handleToolbarKeydown}
    onpointerdown={(event) => event.preventDefault()}
  >
    {#each CHAT_COMPOSER_FORMAT_ACTIONS as definition, index (definition.id)}
      {@const Icon = icons[definition.id]}
      {#if index > 0 && CHAT_COMPOSER_FORMAT_ACTIONS[index - 1]?.group !== definition.group}
        <span class="format-separator" aria-hidden="true"></span>
      {/if}
      <button
        type="button"
        class="format-button"
        aria-label={label(definition)}
        aria-pressed={pressed(definition)}
        aria-keyshortcuts={chatFormatAriaKeyShortcut(definition.shortcut)}
        data-app-tooltip={t("chat.composer.format.withShortcut", label(definition), formatShortcut(definition.shortcut))}
        tabindex={index === activeIndex ? 0 : -1}
        onclick={() => apply(definition, index)}
      ><Icon size={15} aria-hidden="true" /></button>
    {/each}
  </div>
{/if}

<style>
  .format-toolbar { position: fixed; z-index: 100; display: flex; align-items: center; gap: 0.1rem; padding: 0.2rem; visibility: hidden; pointer-events: none; }
  .format-toolbar.placed { visibility: visible; pointer-events: auto; }
  .format-button { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 auto; place-items: center; border-radius: 0.4rem; color: var(--muted-foreground); }
  .format-button:hover, .format-button:focus-visible { background: var(--accent); color: var(--foreground); outline: none; }
  .format-button[aria-pressed="true"] { background: color-mix(in srgb, var(--primary) 14%, transparent); color: var(--foreground); }
  .format-separator { width: 1px; height: 1.1rem; flex: 0 0 auto; margin-inline: 0.15rem; background: var(--border); }
</style>
