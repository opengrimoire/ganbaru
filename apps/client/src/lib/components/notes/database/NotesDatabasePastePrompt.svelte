<script lang="ts">
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { anchoredPanelStyle } from "$lib/utils/anchored-panel";
  import { portal } from "$lib/utils/portal";
  import type { NotesDatabasePasteController } from "$lib/stores/notes/database-paste.svelte";

  let { controller, anchor }: { controller: NotesDatabasePasteController; anchor: HTMLElement | null } = $props();
  const { t } = getLocalization();
  const TEXT_SCALE = 0.8;
  const current = $derived(controller.prompt);

  /** Keep paste choices attached to their row without taking space in the document. */
  function floatPanel(node: HTMLDivElement) {
    const portaled = portal(node, anchor?.closest<HTMLElement>("[data-floating-root]") ?? document.body);
    let previousStyle = "";
    // The panel's own height only informs placement; deriving max-height from it would resize the observed node and loop.
    const place = () => {
      if (!anchor?.isConnected) return;
      const paragraph = anchor.closest(".notes-block-list")?.querySelector<HTMLElement>(".notes-editor-body-text");
      const fontSize = paragraph ? Number.parseFloat(getComputedStyle(paragraph).fontSize) : Number.NaN;
      const placement = anchoredPanelStyle({
        triggerRect: anchor.getBoundingClientRect(), viewportWidth: window.innerWidth, viewportHeight: window.innerHeight,
        preferredWidth: FLOATING_WIDTH.sm, contentHeight: node.getBoundingClientRect().height,
      });
      const style = Number.isFinite(fontSize) ? `${placement}; font-size:${fontSize * TEXT_SCALE}px` : placement;
      if (style === previousStyle) return;
      node.style.cssText = style;
      previousStyle = style;
    };
    const outside = (event: Event) => {
      if (event.target instanceof Node && !node.contains(event.target) && !anchor?.contains(event.target) && !controller.busy) controller.dismiss();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || controller.busy) return;
      event.preventDefault();
      event.stopPropagation();
      controller.dismiss();
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(node);
    if (anchor) observer?.observe(anchor);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
    void tick().then(place);
    return { destroy() {
      observer?.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape, true);
      portaled.destroy();
    } };
  }
</script>

{#if current && anchor}
  <div use:floatPanel role="dialog" aria-label={t("notes.databasePasteAs")} aria-busy={controller.busy}
    data-app-floating-surface data-floating-root class="surface-floating surface-floating-body z-50 overflow-y-auto">
    <p class="menu-label">{t(current.kind === "copy" ? "notes.databasePastedCopy" : "notes.databasePasteAs")}</p>
    {#if controller.started}
      {#if controller.error}<button type="button" class="menu-item disabled:pointer-events-none" disabled={controller.busy} onclick={() => void controller.linkedView()}>{t("common.retry")}</button>{/if}
    {:else if current.kind === "link"}
      <button type="button" class="menu-item disabled:pointer-events-none" disabled={controller.busy} onclick={() => void controller.mention()}>{t("notes.databasePasteMention")}</button>
      <button type="button" class="menu-item disabled:pointer-events-none" disabled={controller.busy} onclick={() => void controller.linkedView()}>{t("notes.databasePasteLinkedView")}</button>
      <button type="button" class="menu-item disabled:pointer-events-none" disabled={controller.busy} onclick={controller.dismiss}>{t("notes.databasePasteUrl")}</button>
    {:else}
      <button type="button" class="menu-item disabled:pointer-events-none" disabled={controller.busy} onclick={() => void controller.linkedView()}>{t("notes.databasePasteAndSync")}</button>
      <button type="button" class="menu-item disabled:pointer-events-none" disabled={controller.busy} onclick={controller.dismiss}>{t("notes.databasePasteDismiss")}</button>
    {/if}
    {#if controller.error}<p role="alert" class="px-2 py-1.5 text-panel-detail text-destructive">{t("notes.databasePasteFailed", controller.error)}</p>{/if}
  </div>
{/if}
