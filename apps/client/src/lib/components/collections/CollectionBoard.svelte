<script lang="ts" generics="Item extends { id: string }, Group extends { id: string }">
  import type { Snippet } from "svelte";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { cn } from "$lib/utils";
  import { collectionWindow } from "./collection-window";

  let { groups, items, label, count, header, card, footer, emptyLabel, dragLabel, canMove, onMove, reorder = false, fill = false, disabled = false, virtualize = false }: {
    groups: readonly Group[];
    items: (group: Group) => readonly Item[];
    label: (group: Group) => string;
    count?: (group: Group) => number;
    header?: Snippet<[Group]>;
    card: Snippet<[Item, Group, Snippet]>;
    footer?: Snippet<[Group]>;
    emptyLabel: string;
    dragLabel: (item: Item) => string;
    canMove: (item: Item, group: Group) => boolean;
    onMove: (item: Item, group: Group, target: Item | null, position: "before" | "after") => Promise<void>;
    reorder?: boolean;
    fill?: boolean;
    disabled?: boolean;
    virtualize?: boolean;
  } = $props();

  let dragged: { item: Item; groupId: string } | null = $state(null);
  let target: { groupId: string; itemId: string | null; position: "before" | "after" } | null = $state(null);
  let pendingId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let heights = $state<Record<string, number>>({});
  let viewports = $state<Record<string, { top: number; height: number; gap: number }>>({});
  const ESTIMATED_CARD_HEIGHT = 144;
  const OVERSCAN = 3;

  function visibleWindow(group: Group) {
    const rows = items(group);
    if (!virtualize) return { items: rows, beforePx: 0, afterPx: 0 };
    const viewport = viewports[group.id] ?? { top: 0, height: ESTIMATED_CARD_HEIGHT, gap: 0 };
    const range = collectionWindow(rows.map((row) => heights[`${group.id}:${row.id}`] ?? ESTIMATED_CARD_HEIGHT), viewport.top, viewport.height, viewport.gap, OVERSCAN);
    return { items: rows.slice(range.start, range.end), beforePx: range.beforePx, afterPx: range.afterPx };
  }

  function measureCard(node: HTMLElement, key: string) {
    if (!virtualize || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      const height = node.getBoundingClientRect().height;
      if (height > 0 && heights[key] !== height) heights[key] = height;
    });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  function trackViewport(node: HTMLElement, groupId: string) {
    const update = () => {
      const previous = viewports[groupId];
      const gap = Number.parseFloat(getComputedStyle(node).rowGap) || 0;
      if (previous?.top !== node.scrollTop || previous?.height !== node.clientHeight || previous?.gap !== gap) viewports[groupId] = { top: node.scrollTop, height: node.clientHeight, gap };
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(node);
    node.addEventListener("scroll", update);
    update();
    return { destroy() { observer?.disconnect(); node.removeEventListener("scroll", update); } };
  }

  function accepts(group: Group): boolean {
    return !disabled && pendingId === null && dragged !== null && canMove(dragged.item, group)
      && (reorder || dragged.groupId !== group.id);
  }

  function dragOver(event: DragEvent, group: Group, item: Item | null = null): void {
    if (item?.id === dragged?.item.id) { event.stopPropagation(); target = null; return; }
    if (!accepts(group)) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    target = { groupId: group.id, itemId: reorder ? item?.id ?? null : null, position: event.clientY >= rect.top + rect.height / 2 ? "after" : "before" };
  }

  async function drop(event: DragEvent, group: Group): Promise<void> {
    event.preventDefault();
    event.stopPropagation();
    const current = dragged;
    if (!current || !accepts(group)) return;
    const over = target?.groupId === group.id ? target : null;
    const overItem = items(group).find((item) => item.id === over?.itemId) ?? null;
    pendingId = current.item.id;
    dragged = null;
    target = null;
    error = null;
    try {
      await onMove(current.item, group, overItem, over?.position ?? "after");
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      pendingId = null;
    }
  }
</script>

<div class={cn("flex min-h-0 min-w-0 flex-col", fill && "h-full")}>
  {#if error}<Notice class="px-3 py-2 text-[0.8rem]" message={error} />{/if}
  <div data-collection-scroll class={cn("collection-board flex min-h-0 min-w-0 gap-3 overflow-x-auto py-2", fill && "h-full scroll-px-3 px-3")}>
    {#each groups as group (group.id)}
      {@const rows = items(group)}
      {@const cardWindow = visibleWindow(group)}
      <section class={cn("collection-column flex min-h-56 shrink-0 flex-col rounded-lg border border-transparent p-1", target?.groupId === group.id ? "border-primary/40 bg-primary/5" : "bg-muted/20")} aria-label={label(group)} ondragover={(event) => dragOver(event, group)} ondrop={(event) => { void drop(event, group); }}>
        <div class="flex min-h-10 min-w-0 items-center gap-2 px-2">
          {#if header}{@render header(group)}{:else}<h3 class="min-w-0 flex-1 truncate text-[0.866667rem] font-medium">{label(group)}</h3><span class="text-[0.8rem] tabular-nums text-muted-foreground">{count?.(group) ?? rows.length}</span>{/if}
        </div>
        <div use:trackViewport={group.id} class="collection-cards flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-1 pb-1">
          {#if cardWindow.beforePx > 0}<div class="shrink-0" aria-hidden="true" style={`height: ${cardWindow.beforePx}px`}></div>{/if}
          {#each cardWindow.items as item (item.id)}
            {#snippet dragHandle()}
              {#if canMove(item, group)}
                <button type="button" class="collection-drag mt-1 flex size-6 shrink-0 cursor-grab items-center justify-center rounded text-muted-foreground hover:bg-accent active:cursor-grabbing" aria-label={dragLabel(item)} draggable={!disabled && pendingId === null} disabled={disabled || pendingId !== null} ondragstart={(event) => { dragged = { item, groupId: group.id }; error = null; event.dataTransfer?.setData("text/plain", item.id); if (event.dataTransfer) event.dataTransfer.effectAllowed = "move"; }} ondragend={() => { dragged = null; target = null; }}><GripVertical class="size-3.5" /></button>
              {/if}
            {/snippet}
            {#if target?.groupId === group.id && target.itemId === item.id && target.position === "before"}<div class="h-0.5 shrink-0 rounded-full bg-primary"></div>{/if}
            <div use:measureCard={`${group.id}:${item.id}`} class={cn("shrink-0", dragged?.item.id === item.id && "opacity-50")} role="group" aria-label={dragLabel(item)} ondragover={(event) => dragOver(event, group, item)} ondrop={(event) => { void drop(event, group); }}>
              {@render card(item, group, dragHandle)}
            </div>
            {#if target?.groupId === group.id && target.itemId === item.id && target.position === "after"}<div class="h-0.5 shrink-0 rounded-full bg-primary"></div>{/if}
          {/each}
          {#if cardWindow.afterPx > 0}<div class="shrink-0" aria-hidden="true" style={`height: ${cardWindow.afterPx}px`}></div>{/if}
          {#if target?.groupId === group.id && target.itemId === null}<div class="h-0.5 shrink-0 rounded-full bg-primary"></div>{/if}
          {#if rows.length === 0}<p class="rounded-md border border-dashed border-border px-3 py-5 text-[0.8rem] text-muted-foreground">{emptyLabel}</p>{/if}
          {#if footer}<div class="mt-1 shrink-0">{@render footer(group)}</div>{/if}
        </div>
      </section>
    {/each}
  </div>
</div>

<style>
  .collection-board { container-type: inline-size; }
  .collection-column { width: min(18rem, calc(100cqw - 0.5rem)); max-height: 36rem; }
  .h-full > .collection-board > .collection-column { max-height: none; }
  .collection-cards { scrollbar-gutter: stable; }
  /* Pointer scrolling stays free; touch swipes settle on a column, after the board padding. */
  @media (hover: none) and (pointer: coarse) {
    .collection-board { scroll-snap-type: x proximity; }
    .collection-column { scroll-snap-align: start; }
    .collection-drag { display: none; }
  }
</style>
