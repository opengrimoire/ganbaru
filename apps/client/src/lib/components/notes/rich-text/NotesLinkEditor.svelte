<script lang="ts">
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesFloatingPanelPlacement, type NotesFloatingPanelRect } from "$lib/notes/editor/floating-panel";
  import { notesLinkTarget } from "$lib/notes/links/navigation";
  import type { NotesPageMentionTarget } from "$lib/notes/rich-text/core";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import Copy from "@lucide/svelte/icons/copy";
  import Database from "@lucide/svelte/icons/database";
  import FileText from "@lucide/svelte/icons/file-text";
  import Link from "@lucide/svelte/icons/link";
  import Trash2 from "@lucide/svelte/icons/trash-2";

  const EDITOR_WIDTH_PX = 360;
  const PREVIEW_WIDTH_PX = 320;
  const PAGE_RESULT_LIMIT = 6;
  const PANEL_TEXT_SCALE = 0.8;

  let {
    value, title, mode, destinationTitle, pageTargets, error, busy, canRemove, editor, readAnchor,
    onInput, onTitleInput, onApply, onRemove, onCancel, onOpen, onCopy, onEdit, onPointerEnter, onPointerLeave,
  }: {
    value: string;
    title: string;
    mode: "preview" | "edit";
    destinationTitle: string;
    pageTargets: readonly NotesPageMentionTarget[];
    error: string | null;
    busy: boolean;
    canRemove: boolean;
    editor: HTMLElement | null;
    readAnchor: () => NotesFloatingPanelRect | null;
    onInput: (value: string) => void;
    onTitleInput: (value: string) => void;
    onApply: () => void;
    onRemove: () => void;
    onCancel: (restoreFocus?: boolean) => void;
    onOpen: () => void;
    onCopy: () => void;
    onEdit: () => void;
    onPointerEnter: () => void;
    onPointerLeave: () => void;
  } = $props();

  const { t } = getLocalization();
  let root = $state<HTMLDivElement | null>(null);
  let body = $state<HTMLDivElement | null>(null);
  let destinationInput = $state<HTMLInputElement | null>(null);
  let titleInput = $state<HTMLInputElement | null>(null);
  let isEditingDestination = $state(false);
  let anchor = $state<NotesFloatingPanelRect>({ top: 0, bottom: 0, left: 0, right: 0 });
  let panelHeight = $state(0);
  let panelFontSize = $state<string | undefined>(undefined);
  let viewport = $state({ width: 0, height: 0 });
  const localTarget = $derived(notesLinkTarget(value, typeof window === "undefined" ? undefined : window.location.href));
  const matches = $derived(isEditingDestination && value.trim() && !localTarget
    ? pageTargets.filter((page) => page.title.toLocaleLowerCase().includes(value.trim().toLocaleLowerCase())).slice(0, PAGE_RESULT_LIMIT) : []);
  const position = $derived(notesFloatingPanelPlacement(anchor, viewport, {
    width: mode === "preview" ? PREVIEW_WIDTH_PX : EDITOR_WIDTH_PX,
    height: panelHeight, align: "start",
  }));

  function reposition(): void {
    const rect = readAnchor();
    if (!rect || !root) return;
    const paragraph = editor?.classList.contains("notes-editor-body-text") ? editor
      : editor?.closest(".notes-block-list")?.querySelector<HTMLElement>("[contenteditable].notes-editor-body-text");
    const paragraphFontSize = paragraph ? Number.parseFloat(window.getComputedStyle(paragraph).fontSize) : Number.NaN;
    panelFontSize = Number.isFinite(paragraphFontSize) ? `${paragraphFontSize * PANEL_TEXT_SCALE}px` : undefined;
    anchor = { top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right };
    const visibleViewport = window.visualViewport;
    viewport = { width: visibleViewport?.width ?? window.innerWidth, height: visibleViewport?.height ?? window.innerHeight };
    const styles = window.getComputedStyle(root);
    const borderHeight = (Number.parseFloat(styles.borderTopWidth) || 0) + (Number.parseFloat(styles.borderBottomWidth) || 0);
    panelHeight = Math.ceil((body ?? root).scrollHeight + borderHeight);
  }

  /** Mount the open popover outside the note layout and release its observers on close. */
  function floatingPanel(node: HTMLDivElement) {
    const portaled = portal(node, editor?.closest<HTMLElement>("[data-floating-root]") ?? document.body);
    reposition();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(reposition);
    observer?.observe(node);
    document.addEventListener("scroll", reposition, true);
    window.addEventListener("resize", reposition);
    window.visualViewport?.addEventListener("resize", reposition);
    window.visualViewport?.addEventListener("scroll", reposition);
    return { destroy() {
      observer?.disconnect();
      document.removeEventListener("scroll", reposition, true);
      window.removeEventListener("resize", reposition);
      window.visualViewport?.removeEventListener("resize", reposition);
      window.visualViewport?.removeEventListener("scroll", reposition);
      portaled.destroy();
    } };
  }

  $effect(() => {
    void mode; void value; void error;
    void tick().then(reposition);
  });

  $effect(() => {
    if (mode !== "edit") return;
    let cancelled = false;
    void tick().then(() => {
      if (cancelled) return;
      const active = document.activeElement;
      if (active && active !== document.body && active !== editor && !root?.contains(active)) {
        onCancel();
        return;
      }
      if (localTarget) titleInput?.focus({ preventScroll: true });
      else { destinationInput?.focus({ preventScroll: true }); destinationInput?.select(); }
    });
    return () => { cancelled = true; };
  });

  function editDestination(): void {
    isEditingDestination = true;
    void tick().then(() => { destinationInput?.focus({ preventScroll: true }); destinationInput?.select(); });
  }

  function selectPage(page: NotesPageMentionTarget): void {
    onInput(`#notes?page=${encodeURIComponent(page.id)}`);
    isEditingDestination = false;
    void tick().then(() => titleInput?.focus({ preventScroll: true }));
  }
</script>

<div
  bind:this={root}
  use:floatingPanel
  use:dismissOnOutside={{ onDismiss: (reason, event) => {
    if (reason === "escape") { event.preventDefault(); event.stopPropagation(); }
    onCancel(reason === "escape");
  } }}
  role="dialog"
  aria-label={mode === "preview" ? t("notes.linkPreview") : t("notes.openLinkEditor")}
  tabindex="-1"
  data-notes-link-panel
  class="notes-editor-body-text surface-floating fixed z-60 flex flex-col overflow-hidden leading-normal"
  style:left={`${position.left}px`}
  style:top={`${position.top}px`}
  style:width={mode === "preview" ? "max-content" : `${position.width}px`}
  style:max-width={`${position.width}px`}
  style:max-height={panelHeight > position.maxHeight ? `${position.maxHeight}px` : undefined}
  style:font-size={panelFontSize ?? `calc(1rem * var(--font-scale) * ${PANEL_TEXT_SCALE})`}
  onpointerenter={onPointerEnter}
  onpointerleave={() => { if (!root?.contains(document.activeElement)) onPointerLeave(); }}
  onfocusin={onPointerEnter}
  onkeydown={(event) => {
    event.stopPropagation();
    if (event.key === "Enter" && !event.isComposing && event.target instanceof HTMLInputElement) {
      event.preventDefault(); onApply();
    }
  }}
  onfocusout={(event) => {
    if (event.relatedTarget instanceof Node && !root?.contains(event.relatedTarget)) onCancel();
  }}
>
  <div bind:this={body} class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto" use:scrollEdgeFadeAction>
    {#if mode === "preview"}
      <div class="flex min-h-11 items-center gap-1 p-1.5">
        <button type="button" disabled={busy} aria-label={t("notes.linkOpen")} class="menu-item min-w-0 flex-1 gap-2" onclick={onOpen}>
          {#if localTarget?.blockId}<Database class="size-4 shrink-0 text-muted-foreground" />{:else if localTarget}<FileText class="size-4 shrink-0 text-muted-foreground" />{:else}<Link class="size-4 shrink-0 text-muted-foreground" />{/if}
          <span class="truncate">{destinationTitle || value}</span>
        </button>
        <button type="button" disabled={busy} aria-label={t("notes.linkCopy")} class="menu-item size-8 shrink-0 justify-center p-0" onclick={onCopy}><Copy class="size-4" /></button>
        {#if canRemove}<button type="button" disabled={busy} class="menu-item w-auto shrink-0" onclick={onEdit}>{t("notes.linkEdit")}</button>{/if}
      </div>
    {:else}
      <div class="space-y-3 p-3">
        <div class="space-y-1.5">
          <label for="notes-link-destination" class="text-[0.875em] text-muted-foreground">{t("notes.linkDestination")}</label>
          {#if localTarget && !isEditingDestination}
            <button id="notes-link-destination" type="button" disabled={busy} class="field flex w-full items-center gap-2 text-left hover:bg-accent" onclick={editDestination}>
              {#if localTarget.blockId}<Database class="size-4 shrink-0 text-muted-foreground" />{:else}<FileText class="size-4 shrink-0 text-muted-foreground" />{/if}
              <span class="truncate">{destinationTitle || value}</span>
            </button>
          {:else}
            <input bind:this={destinationInput} id="notes-link-destination" class="field w-full" {value} disabled={busy} placeholder={t("notes.linkUrlPlaceholder")} onfocus={() => { isEditingDestination = true; }} oninput={(event) => onInput(event.currentTarget.value)} />
            {#if matches.length}
              <div class="space-y-0.5" aria-label={t("notes.linkDestination")}>
                {#each matches as page (page.id)}<button type="button" class="menu-item gap-2" onclick={() => selectPage(page)}><FileText class="size-4 shrink-0 text-muted-foreground" /><span class="truncate">{page.title}</span></button>{/each}
              </div>
            {/if}
          {/if}
        </div>
        <div class="space-y-1.5">
          <label for="notes-link-title" class="text-[0.875em] text-muted-foreground">{t("notes.linkTitle")}</label>
          <input bind:this={titleInput} id="notes-link-title" class="field w-full" value={title} disabled={busy} oninput={(event) => onTitleInput(event.currentTarget.value)} />
        </div>
      </div>
      <div role="separator" class="menu-separator mx-3"></div>
      <div class="flex items-center justify-between gap-2 p-1.5">
        <button type="button" disabled={busy} class="menu-item w-auto" onclick={onApply}>{t("notes.applyLink")}</button>
        {#if canRemove}<button type="button" disabled={busy} class="menu-item menu-item-destructive w-auto gap-2" onclick={onRemove}><Trash2 class="size-4" />{t("notes.removeLink")}</button>{/if}
      </div>
    {/if}
    {#if error}<p role="alert" class="px-3 pb-3 text-[0.875em] text-destructive">{error}</p>{/if}
  </div>
</div>

<style>
  [data-notes-link-panel] button,
  [data-notes-link-panel] input {
    font-size: inherit;
  }
</style>
