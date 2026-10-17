<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getThemeEditor } from "$lib/stores/theme-editor.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import {
    THEME_EDITOR_EDGE_MARGIN,
    clampPanelRect,
    pickThemeEditorGeometry,
  } from "$lib/utils/responsive";
  import ThemeEditor from "./ThemeEditor.svelte";

  let { onBackToList }: { onBackToList: () => void } = $props();

  const themeStore = getTheme();
  const themeEditor = getThemeEditor();
  const viewport = getViewport();
  const { t } = getLocalization();

  const editing = $derived(
    themeEditor.editingId
      ? themeStore.registry[themeEditor.editingId]
      : undefined,
  );

  const isBuiltin = $derived(
    themeEditor.editingId
      ? themeStore.isBuiltin(themeEditor.editingId)
      : false,
  );

  const canResetToSeed = $derived(
    editing?.kind === "user"
      ? themeStore.canResetThemeToSeed(editing.id)
      : false,
  );

  const editorGeometry = $derived(
    pickThemeEditorGeometry({
      viewport: { width: viewport.width, height: viewport.height },
    }),
  );

  let posX = $state(0);
  let posY = $state(0);
  let positionInitialized = $state(false);
  let dragging = $state(false);
  let collapsed = $state(false);
  let panelEl = $state<HTMLDivElement | undefined>(undefined);
  let renderedPanelHeight = $state(0);
  let dragOffsetX = 0;
  let dragOffsetY = 0;
  const THEME_EDITOR_COLLAPSED_FALLBACK_HEIGHT = 96;

  const floatingClampHeight = $derived.by(() => {
    if (!collapsed) return editorGeometry.rect.height;
    if (renderedPanelHeight > 0 && renderedPanelHeight < editorGeometry.rect.height) {
      return renderedPanelHeight;
    }
    return THEME_EDITOR_COLLAPSED_FALLBACK_HEIGHT;
  });

  function getMeasuredPanelHeight(): number {
    const domHeight = panelEl?.getBoundingClientRect().height;
    const measured = domHeight ?? renderedPanelHeight;
    return Number.isFinite(measured) ? Math.max(0, measured) : 0;
  }

  function getFloatingClampHeight(): number {
    if (!collapsed) return editorGeometry.rect.height;
    const measured = getMeasuredPanelHeight();
    if (measured > 0 && measured < editorGeometry.rect.height) return measured;
    return THEME_EDITOR_COLLAPSED_FALLBACK_HEIGHT;
  }

  function clampFloatingPosition(
    x: number,
    y: number,
    height = getFloatingClampHeight(),
  ): { x: number; y: number } {
    const clamped = clampPanelRect(
      {
        x,
        y,
        width: editorGeometry.rect.width,
        height: Math.min(Math.max(height, 0), editorGeometry.rect.height),
      },
      { width: viewport.width, height: viewport.height },
      THEME_EDITOR_EDGE_MARGIN,
    );
    return { x: clamped.x, y: clamped.y };
  }

  function onHeaderPointerDown(e: PointerEvent) {
    if (!editorGeometry.canDrag) return;
    if (e.button !== 0) return;
    // Do not steal drags from buttons inside the header row.
    const target = e.target as HTMLElement | null;
    if (target && target.closest("button")) return;
    dragging = true;
    dragOffsetX = e.clientX - posX;
    dragOffsetY = e.clientY - posY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onHeaderPointerMove(e: PointerEvent) {
    if (!dragging) return;
    const next = clampFloatingPosition(
      e.clientX - dragOffsetX,
      e.clientY - dragOffsetY,
    );
    posX = next.x;
    posY = next.y;
  }

  function onHeaderPointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
  }

  $effect(() => {
    const geometry = editorGeometry;
    if (!geometry.canDrag) {
      dragging = false;
      posX = geometry.rect.x;
      posY = geometry.rect.y;
      positionInitialized = true;
      return;
    }
    if (!positionInitialized) {
      posX = geometry.rect.x;
      posY = geometry.rect.y;
      positionInitialized = true;
      return;
    }
    const next = clampFloatingPosition(posX, posY, floatingClampHeight);
    posX = next.x;
    posY = next.y;
  });

  // Track the rendered height so collapsed drag bounds match the visible panel.
  $effect(() => {
    if (!panelEl) return;
    const measure = () => {
      renderedPanelHeight = panelEl?.getBoundingClientRect().height ?? 0;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panelEl);
    return () => observer.disconnect();
  });

  function toggleCollapsed() {
    const nextCollapsed = !collapsed;
    if (!nextCollapsed && editorGeometry.canDrag) {
      const next = clampFloatingPosition(posX, posY, editorGeometry.rect.height);
      posX = next.x;
      posY = next.y;
    }
    collapsed = nextCollapsed;
  }

  // Cancel rolls the session back to its pre-edit state and returns to the
  // Settings list. Save keeps the edits as the committed state. Both close
  // the panel through `onBackToList`.
  async function onCancel() {
    await themeEditor.cancel();
    onBackToList();
  }

  async function onCommit() {
    await themeEditor.commit();
    onBackToList();
  }

  function onResetAllToSeed() {
    if (editing?.kind !== "user") return;
    if (!canResetToSeed) return;
    themeStore.resetThemeToSeed(editing.id);
  }

  const panelStyle = $derived.by(() => {
    const geometry = editorGeometry;
    const height = collapsed ? "auto" : `${geometry.rect.height}px`;
    const maxHeight = `${geometry.rect.height}px`;
    const clamped = geometry.canDrag
      ? clampFloatingPosition(posX, posY)
      : { x: geometry.rect.x, y: geometry.rect.y };
    const x = clamped.x;

    if (!geometry.canDrag && collapsed) {
      const bottom = geometry.layout === "fullscreen" ? 0 : THEME_EDITOR_EDGE_MARGIN;
      return [
        `left: ${x}px`,
        `bottom: ${bottom}px`,
        `width: ${geometry.rect.width}px`,
        "height: auto",
        `max-height: ${maxHeight}`,
      ].join("; ");
    }

    const y = clamped.y;
    return [
      `left: ${x}px`,
      `top: ${y}px`,
      `width: ${geometry.rect.width}px`,
      `height: ${height}`,
      `max-height: ${maxHeight}`,
    ].join("; ");
  });
</script>

{#if editing}
  <div
    bind:this={panelEl}
    class={cn(
      "surface-dialog fixed z-75 flex flex-col overflow-hidden",
      editorGeometry.layout === "fullscreen" &&
        "rounded-none border-x-0 border-b-0 shadow-none",
    )}
    style={panelStyle}
    data-theme-editor-layout={editorGeometry.layout}
    data-theme-editor-density={editorGeometry.density}
    data-app-shortcuts="ignore"
    role="dialog"
    aria-label={t("settings.theme.editor.dialogLabel")}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class={cn(
        "flex shrink-0 items-center gap-2 border-b border-border/70 bg-sidebar px-3 py-2",
        editorGeometry.canDrag
          ? dragging
            ? "cursor-grabbing"
            : "cursor-grab"
          : "cursor-not-allowed",
      )}
      onpointerdown={onHeaderPointerDown}
      onpointermove={onHeaderPointerMove}
      onpointerup={onHeaderPointerUp}
      onpointercancel={onHeaderPointerUp}
    >
      <div class="min-w-0 flex-1" aria-hidden="true"></div>
      <button
        type="button"
        onclick={toggleCollapsed}
        aria-label={collapsed
          ? t("settings.theme.editor.expandEditor")
          : t("settings.theme.editor.collapseEditor")}
        aria-expanded={!collapsed}
        data-app-tooltip-disabled="true"
        class="flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        {#if collapsed}
          <ChevronDown size={13} strokeWidth={2} />
        {:else}
          <ChevronUp size={13} strokeWidth={2} />
        {/if}
      </button>
    </header>

    <!-- Keep the editor mounted while collapsed so scroll and expanded rows survive. -->
    <div
      class={cn(
        "min-h-0 flex-1 bg-background/40 dark:bg-black/20",
        collapsed && "hidden",
      )}
      aria-hidden={collapsed ? "true" : undefined}
    >
      <ThemeEditor theme={editing} />
    </div>

    <footer
      class="theme-editor-footer flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-border/70 bg-sidebar px-3 py-2"
    >
      <div class="flex min-w-0 items-center gap-2">
        <button
          type="button"
          onclick={onCancel}
          class="flex items-center gap-1.5 rounded-md border border-destructive bg-destructive px-2.5 py-1 text-[0.733333rem] font-medium text-destructive-foreground transition-colors hover:bg-destructive/90"
        >
          <ArrowLeft size={11} strokeWidth={2.25} />
          <span>{t("common.cancel")}</span>
        </button>
        {#if editing.kind === "user"}
          <button
            type="button"
            onclick={onResetAllToSeed}
            disabled={!canResetToSeed}
            aria-disabled={!canResetToSeed}
            aria-label={t("settings.theme.editor.resetAllToSeed")}
            title={canResetToSeed
              ? t("settings.theme.editor.restoreSeedTitle")
              : t("settings.theme.editor.noSeedChangesTitle")}
            class={cn(
              "theme-editor-reset-all flex items-center gap-1.5 rounded-md border border-destructive bg-destructive px-2.5 py-1 text-[0.733333rem] font-medium text-destructive-foreground transition-colors",
              canResetToSeed
                ? "hover:bg-destructive/90"
                : "cursor-not-allowed opacity-40",
            )}
          >
            <RotateCcw size={11} strokeWidth={2.25} />
            <span class="theme-editor-optional-label">
              {t("settings.theme.editor.resetAllToSeedButton")}
            </span>
          </button>
        {/if}
      </div>
      <button
        type="button"
        onclick={onCommit}
        class="flex items-center gap-1.5 rounded-md border border-primary bg-primary px-3 py-1 text-[0.733333rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
      >
        <Check size={11} strokeWidth={2.25} />
        <span>
          {isBuiltin
            ? t("settings.theme.editor.applyAndReturn")
            : t("settings.theme.editor.saveAndApply")}
        </span>
      </button>
    </footer>
  </div>
{/if}

<style>
  [data-theme-editor-density="micro"] .theme-editor-footer {
    gap: 0.375rem;
    padding-inline: 0.5rem;
  }

  [data-theme-editor-density="micro"] .theme-editor-footer button {
    min-height: 2rem;
  }

  [data-theme-editor-density="micro"] .theme-editor-reset-all {
    min-width: 2rem;
    width: 2rem;
    justify-content: center;
    padding-inline: 0;
  }

  [data-theme-editor-density="micro"] .theme-editor-optional-label {
    display: none;
  }
</style>
