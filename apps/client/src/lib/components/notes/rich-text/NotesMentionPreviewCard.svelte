<script lang="ts">
  import type { Snippet } from "svelte";
  import { tick } from "svelte";
  import NotesLocationPath from "$lib/components/notes/NotesLocationPath.svelte";
  import { notesFloatingPanelContentHeight, notesFloatingPanelPlacement, type NotesFloatingPanelPlacement } from "$lib/notes/editor/floating-panel";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  const PREVIEW_WIDTH_PX = 300;

  /**
   * A hover card for an inline mention: the muted route to the object on top, its name below, and optional detail
   * lines, status, and body. The card lives outside editable content and follows its chip while the page scrolls.
   */
  let { anchor, id, route = [], title, details = [], status = null, icon, children, onKeepOpen, onLeave, onDismiss }: {
    anchor: HTMLElement;
    id: string;
    route?: readonly string[];
    title: string;
    details?: readonly string[];
    status?: string | null;
    icon: Snippet;
    children?: Snippet;
    onKeepOpen: () => void;
    onLeave: () => void;
    onDismiss: () => void;
  } = $props();

  let card = $state<HTMLDivElement | null>(null);
  let cardBody = $state<HTMLDivElement | null>(null);
  let placement = $state<NotesFloatingPanelPlacement>({ left: 0, top: 0, width: PREVIEW_WIDTH_PX, maxHeight: 0 });
  let destroyed = false;

  function reposition(): void {
    if (!card || destroyed) return;
    const rect = anchor.getBoundingClientRect();
    const viewport = window.visualViewport;
    const offsetLeft = viewport?.offsetLeft ?? 0;
    const offsetTop = viewport?.offsetTop ?? 0;
    const viewportWidth = viewport?.width ?? window.innerWidth;
    const viewportHeight = viewport?.height ?? window.innerHeight;
    const position = notesFloatingPanelPlacement({
      left: rect.left - offsetLeft, right: rect.right - offsetLeft,
      top: Math.min(viewportHeight, Math.max(0, rect.top - offsetTop)),
      bottom: Math.min(viewportHeight, Math.max(0, rect.bottom - offsetTop)),
    }, { width: viewportWidth, height: viewportHeight }, {
      width: PREVIEW_WIDTH_PX, height: notesFloatingPanelContentHeight({
        scrollHeight: (cardBody ?? card).scrollHeight, clientHeight: card.clientHeight, offsetHeight: card.offsetHeight,
      }), align: "start",
    });
    placement = { ...position, left: position.left + offsetLeft, top: position.top + offsetTop };
  }

  /** Keep the card outside editable content and release all observers when it closes. */
  function floatingCard(node: HTMLDivElement) {
    const floatingRoot = anchor.closest<HTMLElement>("[data-floating-root]");
    const portaled = portal(node, floatingRoot && !floatingRoot.closest("[contenteditable='true']") ? floatingRoot : node.ownerDocument.body);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(reposition);
    observer?.observe(node);
    document.addEventListener("scroll", reposition, true);
    window.addEventListener("resize", reposition);
    window.visualViewport?.addEventListener("resize", reposition);
    window.visualViewport?.addEventListener("scroll", reposition);
    reposition();
    return { destroy() {
      destroyed = true;
      observer?.disconnect();
      document.removeEventListener("scroll", reposition, true);
      window.removeEventListener("resize", reposition);
      window.visualViewport?.removeEventListener("resize", reposition);
      window.visualViewport?.removeEventListener("scroll", reposition);
      portaled.destroy();
    } };
  }

  $effect(() => {
    void route;
    void title;
    void details;
    void status;
    void tick().then(reposition);
  });
</script>

<div
  bind:this={card}
  use:floatingCard
  use:dismissOnOutside={{ onDismiss: (reason, event) => {
    if (reason === "escape") { event.preventDefault(); event.stopPropagation(); }
    onDismiss();
  } }}
  {id}
  role="tooltip"
  contenteditable="false"
  data-notes-mention-preview
  class="notes-mention-preview surface-floating fixed z-60 flex flex-col overflow-hidden"
  style:left={`${placement.left}px`}
  style:top={`${placement.top}px`}
  style:width={`${placement.width}px`}
  style:max-height={`${placement.maxHeight}px`}
  onpointerenter={onKeepOpen}
  onpointerleave={onLeave}
>
  <div bind:this={cardBody} class="min-h-0 flex-1 overflow-auto" use:scrollEdgeFadeAction>
    <div class="flex items-start gap-3 p-3.5">
      <span class="flex size-9 shrink-0 items-center justify-center rounded-floating-item border border-border/60 bg-muted/40 text-muted-foreground" aria-hidden="true">{@render icon()}</span>
      <div class="min-w-0 flex-1">
        <NotesLocationPath segments={route} />
        <div class="wrap-anywhere leading-snug font-medium" data-notes-mention-preview-title>{title}</div>
        {#each details as detail, index (index)}<div class="mt-1 wrap-anywhere text-[0.85em] leading-snug text-muted-foreground">{detail}</div>{/each}
      </div>
    </div>
    {#if status}<div class="px-3.5 pb-3.5 text-[0.9em] text-muted-foreground" role="status">{status}</div>{/if}
    {@render children?.()}
  </div>
</div>

<style>
  .notes-mention-preview { font-size: calc(0.8rem * var(--font-scale, 1)); line-height: 1.5; }
</style>
