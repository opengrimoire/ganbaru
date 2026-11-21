<!--
  @component
  The surface every Chat composer shares: the rich text editor host, paste and drop of images and
  text, the token menu slot, the context meter, and a toolbar with the caller's controls. The
  caller constructs the `ChatComposerEditor` on the bound `editorRoot` and owns sending, so the
  frame holds no draft or send logic.
-->
<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import type { ChatComposerEditor } from "$lib/chat/composer/editor";
  import { sanitizeComposerText } from "$lib/chat/composer/hidden-characters";
  import { clipboardImageFiles, type ContextMeterRead } from "$lib/chat/composer/model";
  import { MAX_TEXT_SNIPPET_BYTES, classifyPastedText } from "$lib/chat/composer/paste";
  import { revealContenteditableComposerCaret } from "$lib/chat/composer/scroll";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatFormattingToolbar from "./ChatFormattingToolbar.svelte";

  let {
    editorRoot = $bindable(),
    editor,
    hero = false,
    collapsible = false,
    collapsed = false,
    disabled,
    label,
    placeholder,
    role = "textbox",
    menuOpen = false,
    menuId = "chat-composer-menu",
    activeOptionId,
    showMeter = true,
    meter = null,
    compactionReported = false,
    meterNote = null,
    onKeydown,
    onFiles,
    onLargeText,
    onFocusChange,
    onPaste,
    onCopy,
    onCompositionChange,
    above,
    menu,
    leading,
    trailing,
    actions,
    below,
  }: {
    /** The editor host element, bound so the caller can construct the editor on it. */
    editorRoot?: HTMLDivElement;
    /** The editor the caller constructed on `editorRoot`, once it exists. */
    editor: () => ChatComposerEditor | undefined;
    hero?: boolean;
    /** Whether the composer animates between its single row and its full layout. */
    collapsible?: boolean;
    /**
     * Whether a collapsible composer rests as one rounded row with the editor between the first
     * leading control and the actions; the other controls fade out and cannot be reached.
     */
    collapsed?: boolean;
    disabled: boolean;
    /** Accessible name of the editor. */
    label: string;
    placeholder: string;
    /**
     * The editor's role: `textbox`, or `combobox` when its token menus act as an autocomplete
     * listbox the editor announces.
     */
    role?: "textbox" | "combobox";
    /** Whether a token menu is open, so the editor controls the element named by `menuId`. */
    menuOpen?: boolean;
    /** Id of the open token menu. */
    menuId?: string;
    /** Id of the highlighted token menu option. */
    activeOptionId?: string;
    /** Whether the context meter shows; hidden where no provider session can exist. */
    showMeter?: boolean;
    meter?: ContextMeterRead | null;
    /** Whether the provider reported an automatic context compaction. */
    compactionReported?: boolean;
    /** A line the meter details add, such as where the work runs. */
    meterNote?: string | null;
    onKeydown: (event: KeyboardEvent) => void;
    /** Receives images pasted or dropped into the composer. */
    onFiles: (files: File[]) => void;
    /**
     * Receives pasted or dropped text of at least `LARGE_PASTE_ATTACHMENT_BYTES`, with hidden
     * characters removed, to attach as a text attachment. Without it all text is inserted inline.
     */
    onLargeText?: (text: string) => void;
    onFocusChange?: (focused: boolean) => void;
    /**
     * Runs before the frame's own paste handling; returning `true` means the caller handled the
     * paste, such as one carrying references, and the frame does nothing more.
     */
    onPaste?: (event: ClipboardEvent) => boolean;
    /** Handles a copy from the editor, such as one that keeps references in the clipboard. */
    onCopy?: (event: ClipboardEvent) => void;
    /** Reports when an input method composition starts and ends. */
    onCompositionChange?: (composing: boolean) => void;
    /** Content above the editor, such as notices, attachment chips, and panels. */
    above?: Snippet;
    /** The open token menu, rendered over the editor. */
    menu?: Snippet;
    /** Toolbar controls at the start. */
    leading?: Snippet;
    /** Toolbar controls at the end, before the meter. */
    trailing?: Snippet;
    /** Send and stop controls after the meter. */
    actions?: Snippet;
    /** Content below the editor, such as requests and errors. */
    below?: Snippet;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let meterOpen = $state(false);
  let meterElement: HTMLElement | undefined = $state();
  let removedHiddenCharacters = $state(0);
  let pasteRefusal = $state<string | null>(null);
  let composing = $state(false);
  let formattingToolbar: ChatFormattingToolbar | undefined = $state();
  /** The frame's own notices need the full layout, so they keep a collapsible composer open. */
  const restingCollapsed = $derived(collapsible && collapsed && removedHiddenCharacters === 0 && !pasteRefusal);

  function handleKeydown(event: KeyboardEvent): void {
    if (formattingToolbar?.handleEditorKeydown(event)) return;
    onKeydown(event);
  }

  function setComposing(next: boolean): void {
    composing = next;
    onCompositionChange?.(next);
  }

  onMount(() => {
    const selectionChanged = () => {
      if (document.activeElement === editorRoot) editor()?.handleSelectionChange();
    };
    const closeMeter = (event: PointerEvent) => {
      if (meterOpen && event.target instanceof Node && !meterElement?.contains(event.target)) meterOpen = false;
    };
    document.addEventListener("selectionchange", selectionChanged);
    window.addEventListener("pointerdown", closeMeter, true);
    return () => {
      document.removeEventListener("selectionchange", selectionChanged);
      window.removeEventListener("pointerdown", closeMeter, true);
    };
  });

  function insertText(text: string): void {
    editor()?.insertPlainText(text);
    void tick().then(() => {
      if (editorRoot) revealContenteditableComposerCaret(editorRoot);
    });
  }

  /**
   * Places text arriving from outside the editor: hidden characters are removed with a notice,
   * large text goes to `onLargeText`, and text over the attachment bound is refused visibly.
   */
  function receiveText(raw: string): void {
    const { text, hiddenCharacters } = sanitizeComposerText(raw);
    removedHiddenCharacters = hiddenCharacters;
    pasteRefusal = null;
    if (!text) return;
    const placement = onLargeText ? classifyPastedText(text) : "inline";
    if (placement === "too_large") {
      pasteRefusal = t(
        "chat.composer.paste.tooLarge",
        formatNumber(localization.locale, MAX_TEXT_SNIPPET_BYTES / (1024 * 1024), { style: "unit", unit: "megabyte" }),
      );
    } else if (placement === "attachment") {
      onLargeText?.(text);
    } else {
      insertText(text);
    }
  }

  function handlePaste(event: ClipboardEvent): void {
    if (onPaste?.(event)) return;
    const files = clipboardImageFiles(event.clipboardData);
    if (files.length > 0) {
      event.preventDefault();
      onFiles(files);
      return;
    }
    if ([...(event.clipboardData?.types ?? [])].includes("text/plain")) {
      event.preventDefault();
      receiveText(event.clipboardData?.getData("text/plain") ?? "");
    }
  }

  function handleDrop(event: DragEvent): void {
    event.preventDefault();
    const files = [...(event.dataTransfer?.files ?? [])].filter((file) => file.type.startsWith("image/"));
    if (files.length > 0) {
      onFiles(files);
      return;
    }
    const text = event.dataTransfer?.getData("text/plain");
    if (text) receiveText(text);
  }

  function contextLabel(): string {
    if (!meter) return t("chat.composer.contextUnavailable");
    const used = formatNumber(localization.locale, meter.usedTokens);
    return meter.maximumTokens === null
      ? t("chat.composer.contextUnknown", used)
      : t("chat.composer.contextUsed", used, formatNumber(localization.locale, meter.maximumTokens, { notation: "compact" }));
  }

  function contextPercentage(value: number): string {
    return formatNumber(localization.locale, value * 100, { maximumFractionDigits: 0 });
  }
</script>

<section class:hero class:collapsible class:collapsed={restingCollapsed} class="chat-composer" data-chat-composer-container role="group" ondragover={(event) => event.preventDefault()} ondrop={handleDrop}>
  {@render above?.()}
  {#if removedHiddenCharacters > 0}
    <Notice class="composer-notice" tone="warning" role="status" message={t("chat.composer.paste.hiddenRemoved", removedHiddenCharacters)} onDismiss={() => { removedHiddenCharacters = 0; }} />
  {/if}
  {#if pasteRefusal}
    <Notice class="composer-notice" message={pasteRefusal} onDismiss={() => { pasteRefusal = null; }} />
  {/if}
  <div class="editor-shell">
    <!-- Taps on the padding around the editor, which is most of a collapsed row, still reach the editor. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="composer-editor-frame" onclick={(event) => { if (event.target === event.currentTarget && !disabled) editorRoot?.focus(); }}>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        bind:this={editorRoot}
        class="composer-editor"
        data-chat-composer
        contenteditable={disabled ? "false" : "true"}
        {role}
        aria-multiline={role === "textbox" ? "true" : undefined}
        aria-autocomplete={role === "combobox" ? "list" : undefined}
        aria-haspopup={role === "combobox" ? "listbox" : undefined}
        aria-expanded={role === "combobox" ? menuOpen : undefined}
        aria-disabled={disabled}
        aria-label={label}
        aria-controls={menuOpen ? menuId : undefined}
        aria-activedescendant={menuOpen ? activeOptionId : undefined}
        data-placeholder={placeholder}
        spellcheck="true"
        tabindex="0"
        onfocus={() => { onFocusChange?.(true); editor()?.handleSelectionChange(); }}
        onblur={(event) => { const target = event.currentTarget; queueMicrotask(() => { if (target.isConnected) onFocusChange?.(false); }); }}
        oninput={() => editor()?.handleInput()}
        onbeforeinput={(event) => editor()?.handleBeforeInput(event)}
        onkeydown={handleKeydown}
        onpaste={handlePaste}
        oncopy={onCopy}
        oncompositionstart={() => { setComposing(true); editor()?.handleCompositionStart(); }}
        oncompositionend={() => { setComposing(false); editor()?.handleCompositionEnd(); }}
      ></div>
    </div>
    <ChatFormattingToolbar bind:this={formattingToolbar} {editorRoot} {editor} {disabled} {menuOpen} {composing} />
    {@render menu?.()}
    <div class="composer-toolbar">
      <div class="toolbar-left">{@render leading?.()}</div>
      <div class="toolbar-right">
        {@render trailing?.()}
        {#if showMeter}
          <span bind:this={meterElement} class="context-meter">
            <button
              type="button"
              class="context-ring"
              class:warning={meter?.warning}
              aria-expanded={meterOpen}
              aria-label={contextLabel()}
              style={`--context-progress:${Math.max(0, Math.min(1, meter?.ratio ?? 0))}`}
              onclick={() => { meterOpen = !meterOpen; }}
            >
              <svg viewBox="0 0 20 20" aria-hidden="true"><circle class="context-track" cx="10" cy="10" r="7"></circle><circle class="context-fill" cx="10" cy="10" r="7"></circle></svg>
            </button>
            <span class="context-tooltip" class:open={meterOpen} role="tooltip"><strong>{t("chat.composer.contextWindow")}</strong>{#if meter && meter.ratio !== null}<span>{t("chat.composer.contextPercentage", contextPercentage(meter.ratio), contextPercentage(1 - meter.ratio))}</span>{/if}<span>{contextLabel()}</span>{#if compactionReported || meter?.warning}<small>{t("chat.composer.contextCompaction")}</small>{/if}{#if meterNote}<small>{meterNote}</small>{/if}</span>
          </span>
        {/if}
        {@render actions?.()}
      </div>
    </div>
  </div>
  {@render below?.()}
</section>

<style>
  .chat-composer { container-type: inline-size; container-name: chat-composer; position: relative; display: grid; width:min(100%,var(--chat-conversation-max-width,60rem)); margin: 0 auto; overflow: visible; border: 1px solid color-mix(in srgb, var(--border) 88%, transparent); border-radius: 1.3rem; background: var(--card); box-shadow: 0 8px 22px -18px rgb(0 0 0 / 0.24), 0 1px 4px -3px rgb(0 0 0 / 0.16); }
  .chat-composer.hero { width:min(100%,var(--chat-conversation-max-width,60rem)); text-align: left; }
  .chat-composer > :global(:not(.editor-shell)) { margin-inline: 0.75rem; }
  .editor-shell { position: relative; }
  .composer-editor-frame { padding: 1rem 1.25rem 0; }
  .composer-editor { position: relative; display: block; box-sizing: border-box; width: 100%; min-height: 1lh; max-height: 6lh; overflow-y: auto; background: transparent; padding: 0; color: var(--foreground); caret-color: var(--foreground); font-size: var(--chat-conversation-font-size, calc(0.875rem * var(--type-scale))); line-height: var(--chat-conversation-line-height, calc(1.3125rem * var(--type-scale))); outline: none; overflow-wrap: anywhere; white-space: pre-wrap; }
  .composer-editor:global([data-empty="true"])::before { position: absolute; color: color-mix(in srgb, var(--muted-foreground) 52%, transparent); content: attr(data-placeholder); pointer-events: none; }
  .composer-editor :global(.chat-reference-atom) { display: inline-block; border-radius: 0.3rem; background: color-mix(in srgb, var(--primary) 13%, transparent); padding-inline: 0.12rem; color: color-mix(in srgb, var(--primary) 78%, var(--foreground)); box-decoration-break: clone; cursor: default; user-select: all; }
  .composer-editor :global(code) { border-radius: 0.25rem; background: color-mix(in srgb, var(--muted) 70%, transparent); padding-inline: 0.15rem; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: 0.92em; }
  .composer-editor :global([data-chat-composer-line]) { display: block; min-height: var(--chat-conversation-line-height, calc(1.3125rem * var(--type-scale))); line-height: inherit; }
  .composer-toolbar { display: grid; min-height: 2.6rem; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 0.5rem; padding: 0 0.75rem 0.5rem; }
  /* Touch controls are taller than the toolbar row, so the expanded toolbar keeps a gap under the last line of text. */
  @media (pointer: coarse) { .chat-composer:not(.collapsed) .composer-toolbar { padding-top: 0.5rem; } }
  .toolbar-left, .toolbar-right { display: flex; min-width: 0; align-items: center; gap: 0.3rem; }
  .toolbar-left { overflow: hidden; }
  /* Open menus anchored in the toolbar must not be clipped by its overflow. */
  .toolbar-left:has(:global(details[open]), :global([aria-expanded="true"])) { overflow: visible; }
  .toolbar-right { flex: 0 1 auto; justify-content: flex-end; }
  /*
    A collapsed composer is one row as tall as a touch control plus its inset. The toolbar is pulled up over the
    editor row, so the first leading control and the actions sit at its ends while the editor fills the middle;
    padding and margin carry the change, which every WebView can animate.
  */
  .chat-composer.collapsible { --composer-row-inset: 0.25rem; --composer-row: calc(var(--composer-control-size, 2.75rem) + var(--composer-row-inset) * 2); --composer-line: var(--chat-conversation-line-height, calc(1.3125rem * var(--type-scale))); --composer-motion: 240ms cubic-bezier(0.2, 0.8, 0.2, 1); transition: border-radius var(--composer-motion); }
  .chat-composer.collapsible .composer-editor-frame, .chat-composer.collapsible .composer-toolbar { transition: padding var(--composer-motion), margin var(--composer-motion), min-height var(--composer-motion); }
  .chat-composer.collapsible .toolbar-left > :global(*), .chat-composer.collapsible .toolbar-right > :global(*) { transition: opacity 180ms ease 60ms, visibility 0s linear 0s; }
  .chat-composer.collapsed { border-radius: calc(var(--composer-row) / 2 + 1px); }
  .chat-composer.collapsed .composer-editor-frame { padding: calc((var(--composer-row) - var(--composer-line)) / 2) calc(var(--composer-row) + var(--composer-row-inset)); }
  .chat-composer.collapsed .composer-editor { max-height: var(--composer-line); }
  .chat-composer.collapsed .composer-editor:global([data-empty="true"])::before { inset-inline: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-composer.collapsed .composer-toolbar { min-height: var(--composer-row); margin-top: calc(var(--composer-row) * -1); padding: var(--composer-row-inset); pointer-events: none; }
  .chat-composer.collapsed .toolbar-left > :global(*), .chat-composer.collapsed .toolbar-right > :global(*) { pointer-events: auto; }
  .chat-composer.collapsed .toolbar-left > :global(:not(:first-child)), .chat-composer.collapsed .toolbar-right > :global(:not(:last-child)) { visibility: hidden; opacity: 0; transition: opacity 120ms ease, visibility 0s linear 120ms; }
  .context-meter { position: relative; display: inline-flex; flex: 0 0 auto; }
  .context-ring { display: grid; width: 1.45rem; height: 1.45rem; place-items: center; border-radius: 999px; color: var(--muted-foreground); outline: none; }
  .context-ring.warning { color: var(--status-tentative); }
  .context-ring svg { width: 1.2rem; height: 1.2rem; transform: rotate(-90deg); overflow: visible; }
  .context-ring circle { fill: none; stroke-width: 3; }
  .context-track { stroke: color-mix(in srgb, var(--muted-foreground) 22%, transparent); }
  .context-fill { stroke: currentColor; stroke-linecap: round; stroke-dasharray: 43.9823; stroke-dashoffset: calc(43.9823 * (1 - var(--context-progress))); transition: stroke-dashoffset 180ms ease; }
  .context-tooltip { position: absolute; right: 50%; bottom: calc(100% + 0.55rem); z-index: 55; display: none; width: max-content; max-width: min(18rem, 80vw); transform: translateX(50%); border: 1px solid var(--border); border-radius: var(--floating-radius); background: var(--popover); padding: 0.55rem 0.75rem; color: var(--popover-foreground); box-shadow: var(--floating-shadow); text-align: center; }
  .context-meter:hover .context-tooltip, .context-tooltip.open { display: grid; gap: 0.15rem; }
  .context-tooltip strong { color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); font-weight: 400; }
  .context-tooltip span { font-size: calc(0.733333rem * var(--type-scale)); }
  .context-tooltip small { max-width: 15rem; color: var(--muted-foreground); font-size: calc(0.633333rem * var(--type-scale)); line-height: calc(0.9rem * var(--type-scale)); }
  .chat-composer :global(.composer-notice) { margin: 0.75rem 1.25rem 0; font-size: calc(0.733333rem * var(--type-scale)); }
  .chat-composer :global(.composer-notice-end) { margin: 0 1.25rem 0.75rem; }
  @container chat-composer (max-width: 390px) { .toolbar-left { gap: 0.1rem; } .context-meter { display: none; } }
  @container chat-composer (max-width: 300px) { .composer-editor-frame { padding-inline: 0.8rem; } .composer-toolbar { padding-inline: 0.4rem; } }
  @media (prefers-reduced-motion: reduce) { .chat-composer { scroll-behavior: auto; } .chat-composer.collapsible { --composer-motion: 0ms linear; } .chat-composer.collapsible .toolbar-left > :global(*), .chat-composer.collapsible .toolbar-right > :global(*), .chat-composer.collapsed .toolbar-left > :global(:not(:first-child)), .chat-composer.collapsed .toolbar-right > :global(:not(:last-child)) { transition: none; } }
</style>
