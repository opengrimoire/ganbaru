<script lang="ts">
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import {
    moveMusicPlaylistOrder,
    musicPlaylistGridInsertion,
    musicPlaylistGridLayout,
    MUSIC_PLAYLIST_GRID_CARD_HEIGHT,
    type MusicPlaylistGridPosition,
  } from "$lib/music/playlists/order";
  import { isSystemMusicPlaylistId, orderMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import MusicPlaylistIcon from "./MusicPlaylistIcon.svelte";

  let {
    playlists,
    onEdit,
    onDelete,
    onReorder,
    onDone,
    showHeader = true,
    headerTitle,
    headerDescription,
    actionsDisabled = $bindable(false),
  }: {
    playlists: MusicPlaylistSummary[];
    onEdit: (playlistId: string) => void;
    onDelete: (playlistId: string) => void;
    onReorder: (playlistIds: string[]) => Promise<boolean>;
    onDone: () => void;
    showHeader?: boolean;
    headerTitle?: string;
    headerDescription?: string | null;
    actionsDisabled?: boolean;
  } = $props();

  interface PendingPointer {
    playlistId: string;
    pointerId: number;
    captureNode: HTMLElement;
    node: HTMLDivElement;
    startX: number;
    startY: number;
    grabX: number;
    grabY: number;
    width: number;
    height: number;
  }

  interface ActiveDrag extends PendingPointer {
    clientX: number;
    clientY: number;
    originIds: string[];
  }

  interface DropSettle {
    playlistId: string;
    left: number;
    top: number;
    width: number;
    height: number;
  }

  const { t } = getLocalization();
  let managerRoot = $state<HTMLDivElement | null>(null);
  let managerWidth = $state(0);
  let visualPlaylists = $state<MusicPlaylistSummary[]>(untrack(() => orderMusicPlaylists(playlists)));
  let initialIds = $state<string[]>(untrack(() => orderMusicPlaylists(playlists).map((playlist) => playlist.id)));
  let positions = $state<Record<string, MusicPlaylistGridPosition>>({});
  let layoutHeight = $state(0);
  let drag = $state.raw<ActiveDrag | null>(null);
  let dragRenderIds = $state<string[] | null>(null);
  let settling = $state<DropSettle | null>(null);
  let handoffId = $state<string | null>(null);
  let keyboardPlaylistId = $state<string | null>(null);
  let keyboardOriginIds = $state<string[]>([]);
  let saving = $state(false);
  let reorderError = $state(false);
  let announcement = $state("");
  let pendingPointer = $state.raw<PendingPointer | null>(null);
  let dragFrame: number | null = null;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;
  let handoffFrame: number | null = null;
  let previousBodyUserSelect = "";
  let previousDocumentCursor = "";
  const renderedPlaylists = $derived.by(() => {
    if (!dragRenderIds) return visualPlaylists;
    const byId = new Map(visualPlaylists.map((playlist) => [playlist.id, playlist]));
    return dragRenderIds.flatMap((id) => {
      const playlist = byId.get(id);
      return playlist ? [playlist] : [];
    });
  });

  function ids(): string[] {
    return visualPlaylists.map((playlist) => playlist.id);
  }

  function applyLayout(): void {
    const layout = musicPlaylistGridLayout(
      managerWidth || managerRoot?.clientWidth || 272,
      visualPlaylists.map(() => MUSIC_PLAYLIST_GRID_CARD_HEIGHT),
    );
    positions = Object.fromEntries(visualPlaylists.map((playlist, index) => [playlist.id, layout.positions[index]]));
    layoutHeight = layout.height;
  }

  function movePlaylist(playlistId: string, targetIndex: number): void {
    const sourceIndex = visualPlaylists.findIndex((playlist) => playlist.id === playlistId);
    if (sourceIndex < 0) return;
    const boundedTarget = Math.max(0, Math.min(targetIndex, visualPlaylists.length - 1));
    if (sourceIndex === boundedTarget) return;
    const moved = visualPlaylists[sourceIndex];
    if (!moved) return;
    visualPlaylists = moveMusicPlaylistOrder(visualPlaylists, playlistId, boundedTarget);
    applyLayout();
    announcement = t("music.builder.playlistMoved", systemMusicPlaylistName(moved.id, moved.name, t), boundedTarget + 1, visualPlaylists.length);
  }

  function restoreOrder(originIds: readonly string[]): void {
    const byId = new Map(visualPlaylists.map((playlist) => [playlist.id, playlist]));
    visualPlaylists = originIds.flatMap((id) => {
      const playlist = byId.get(id);
      return playlist ? [playlist] : [];
    });
    applyLayout();
  }

  async function saveOrder(originIds: string[]): Promise<boolean> {
    const nextIds = ids();
    if (nextIds.every((id, index) => id === originIds[index])) return true;
    saving = true;
    reorderError = false;
    const saved = await onReorder(nextIds);
    if (!saved) {
      restoreOrder(originIds);
      reorderError = true;
      announcement = t("music.builder.playlistReorderFailed");
    }
    saving = false;
    return saved;
  }

  export async function finishManaging(): Promise<void> {
    if (saving || drag || pendingPointer) return;
    if (keyboardPlaylistId) {
      const origin = keyboardOriginIds;
      keyboardPlaylistId = null;
      keyboardOriginIds = [];
      if (!await saveOrder(origin)) return;
    }
    onDone();
  }

  export async function cancelManaging(): Promise<void> {
    if (saving || drag || pendingPointer) return;
    keyboardPlaylistId = null;
    keyboardOriginIds = [];
    const currentIds = ids();
    restoreOrder(initialIds);
    if (!await saveOrder(currentIds)) return;
    onDone();
  }

  function dragScrollVelocity(clientY: number): number {
    const scrollable = managerRoot?.closest<HTMLElement>("[data-music-scrollable='true']");
    if (!scrollable) return 0;
    const rect = scrollable.getBoundingClientRect();
    const edge = Math.min(72, rect.height * 0.22);
    if (clientY < rect.top + edge) return -16 * (1 - Math.max(0, clientY - rect.top) / edge);
    if (clientY > rect.bottom - edge) return 16 * (1 - Math.max(0, rect.bottom - clientY) / edge);
    return 0;
  }

  function updateDragOrder(active: ActiveDrag): void {
    if (!managerRoot) return;
    const draggedIndex = visualPlaylists.findIndex((playlist) => playlist.id === active.playlistId);
    if (draggedIndex < 0) return;
    const rect = managerRoot.getBoundingClientRect();
    const targetLeft = active.clientX - active.grabX - rect.left;
    const targetTop = active.clientY - active.grabY - rect.top;
    const slots = visualPlaylists.flatMap((playlist, index) => {
      const position = positions[playlist.id];
      return position ? [{ index, left: position.left, top: position.top }] : [];
    });
    const insertion = musicPlaylistGridInsertion(slots, targetLeft, targetTop, draggedIndex);
    const current = positions[active.playlistId];
    if (!current || insertion.index === draggedIndex) return;
    const currentDistance = (current.left - targetLeft) ** 2 + (current.top - targetTop) ** 2;
    if (insertion.distanceSquared + 100 < currentDistance) movePlaylist(active.playlistId, insertion.index);
  }

  function runDragFrame(): void {
    dragFrame = null;
    const active = drag;
    if (!active) return;
    active.node.style.left = `${active.clientX - active.grabX}px`;
    active.node.style.top = `${active.clientY - active.grabY}px`;
    const scrollable = managerRoot?.closest<HTMLElement>("[data-music-scrollable='true']");
    const velocity = dragScrollVelocity(active.clientY);
    if (scrollable && velocity !== 0) scrollable.scrollTop += velocity;
    updateDragOrder(active);
    if (velocity !== 0) scheduleDragFrame();
  }

  function scheduleDragFrame(): void {
    if (dragFrame === null) dragFrame = requestAnimationFrame(runDragFrame);
  }

  function activateDrag(event: PointerEvent, pending: PendingPointer): void {
    previousBodyUserSelect = document.body.style.userSelect;
    previousDocumentCursor = document.documentElement.style.cursor;
    document.body.style.userSelect = "none";
    document.documentElement.style.cursor = "grabbing";
    const originIds = ids();
    dragRenderIds = originIds;
    drag = { ...pending, clientX: event.clientX, clientY: event.clientY, originIds };
    scheduleDragFrame();
  }

  function startPointerDrag(event: PointerEvent, playlistId: string): void {
    if (saving || pendingPointer || event.button !== 0 || !event.isPrimary) return;
    if (!(event.currentTarget instanceof HTMLElement)) return;
    const node = event.currentTarget.closest<HTMLDivElement>("[data-playlist-manager-id]");
    if (!node) return;
    completeDropSettle();
    const rect = node.getBoundingClientRect();
    pendingPointer = {
      playlistId,
      pointerId: event.pointerId,
      captureNode: event.currentTarget,
      node,
      startX: event.clientX,
      startY: event.clientY,
      grabX: event.clientX - rect.left,
      grabY: event.clientY - rect.top,
      width: rect.width,
      height: rect.height,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function movePointerDrag(event: PointerEvent): void {
    const pending = pendingPointer;
    if (!pending || pending.pointerId !== event.pointerId) return;
    if (!drag) {
      if (Math.hypot(event.clientX - pending.startX, event.clientY - pending.startY) < 4) return;
      activateDrag(event, pending);
    } else {
      drag.clientX = event.clientX;
      drag.clientY = event.clientY;
    }
    event.preventDefault();
    scheduleDragFrame();
  }

  function restoreDocumentDragState(): void {
    document.body.style.userSelect = previousBodyUserSelect;
    document.documentElement.style.cursor = previousDocumentCursor;
  }

  function releasePendingPointer(): PendingPointer | null {
    const pending = pendingPointer;
    pendingPointer = null;
    if (pending?.captureNode.hasPointerCapture(pending.pointerId)) pending.captureNode.releasePointerCapture(pending.pointerId);
    return pending;
  }

  function finishPointerDrag(event: PointerEvent, cancelled: boolean): void {
    const pending = pendingPointer;
    if (!pending || pending.pointerId !== event.pointerId) return;
    if (drag) {
      drag.clientX = event.clientX;
      drag.clientY = event.clientY;
      drag.node.style.left = `${drag.clientX - drag.grabX}px`;
      drag.node.style.top = `${drag.clientY - drag.grabY}px`;
    }
    const active = drag;
    if (active) updateDragOrder(active);
    releasePendingPointer();
    if (!active) return;
    event.preventDefault();
    if (dragFrame !== null) cancelAnimationFrame(dragFrame);
    dragFrame = null;
    restoreDocumentDragState();
    if (cancelled) {
      drag = null;
      restoreOrder(active.originIds);
      dragRenderIds = null;
      suppressLayoutTransition(active.playlistId);
      return;
    }
    void saveOrder(active.originIds);
    void settleDrop(active);
  }

  async function settleDrop(active: ActiveDrag): Promise<void> {
    const position = positions[active.playlistId];
    const rect = managerRoot?.getBoundingClientRect();
    if (!position || !rect) {
      drag = null;
      dragRenderIds = null;
      suppressLayoutTransition(active.playlistId);
      return;
    }
    settling = {
      playlistId: active.playlistId,
      left: active.clientX - active.grabX,
      top: active.clientY - active.grabY,
      width: active.width,
      height: active.height,
    };
    drag = null;
    await tick();
    if (settling?.playlistId !== active.playlistId) return;
    active.node.getBoundingClientRect();
    settling = { ...settling, left: rect.left + position.left, top: rect.top + position.top, width: position.width };
    if (settleTimer) clearTimeout(settleTimer);
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    settleTimer = setTimeout(completeDropSettle, reducedMotion ? 0 : 160);
  }

  function suppressLayoutTransition(playlistId: string): void {
    handoffId = playlistId;
    if (handoffFrame !== null) cancelAnimationFrame(handoffFrame);
    handoffFrame = requestAnimationFrame(() => {
      handoffFrame = null;
      if (handoffId === playlistId) handoffId = null;
    });
  }

  function completeDropSettle(): void {
    const playlistId = settling?.playlistId;
    if (settleTimer) clearTimeout(settleTimer);
    settleTimer = null;
    settling = null;
    dragRenderIds = null;
    if (playlistId) suppressLayoutTransition(playlistId);
  }

  function cancelActiveReorder(): void {
    const active = drag;
    releasePendingPointer();
    if (dragFrame !== null) cancelAnimationFrame(dragFrame);
    dragFrame = null;
    if (active) {
      drag = null;
      restoreDocumentDragState();
      restoreOrder(active.originIds);
      dragRenderIds = null;
      suppressLayoutTransition(active.playlistId);
      announcement = t("music.builder.playlistReorderCancelled");
    }
    if (keyboardPlaylistId) {
      restoreOrder(keyboardOriginIds);
      keyboardPlaylistId = null;
      keyboardOriginIds = [];
      announcement = t("music.builder.playlistReorderCancelled");
    }
  }

  function handleKeyboard(event: KeyboardEvent, playlist: MusicPlaylistSummary): void {
    if (saving) return;
    const active = keyboardPlaylistId === playlist.id;
    if (event.key === " " || event.key === "Enter") {
      event.preventDefault();
      if (!keyboardPlaylistId) {
        keyboardPlaylistId = playlist.id;
        keyboardOriginIds = ids();
        announcement = t("music.builder.playlistReorderPickedUp", systemMusicPlaylistName(playlist.id, playlist.name, t));
      } else if (active) {
        const origin = keyboardOriginIds;
        keyboardPlaylistId = null;
        keyboardOriginIds = [];
        void saveOrder(origin);
      }
      return;
    }
    if (!active) return;
    const index = visualPlaylists.findIndex((entry) => entry.id === playlist.id);
    if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
      event.preventDefault();
      movePlaylist(playlist.id, index - 1);
    } else if (event.key === "ArrowDown" || event.key === "ArrowRight") {
      event.preventDefault();
      movePlaylist(playlist.id, index + 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      movePlaylist(playlist.id, 0);
    } else if (event.key === "End") {
      event.preventDefault();
      movePlaylist(playlist.id, visualPlaylists.length - 1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancelActiveReorder();
    }
  }

  function wrapperStyle(playlistId: string, position: MusicPlaylistGridPosition | undefined): string {
    if (drag?.playlistId === playlistId) {
      return `position: fixed; left: ${drag.clientX - drag.grabX}px; top: ${drag.clientY - drag.grabY}px; width: ${drag.width}px; height: ${drag.height}px; transform: none; z-index: 70;`;
    }
    if (settling?.playlistId === playlistId) {
      return `position: fixed; left: ${settling.left}px; top: ${settling.top}px; width: ${settling.width}px; height: ${settling.height}px; transform: none; z-index: 70;`;
    }
    return position ? `width: ${position.width}px; transform: translate(${position.left}px, ${position.top}px);` : "width: 272px;";
  }

  onMount(() => {
    if (!managerRoot) return;
    const observer = new ResizeObserver(([entry]) => {
      const nextWidth = entry?.contentRect.width ?? managerRoot?.clientWidth ?? 0;
      if (Math.abs(nextWidth - managerWidth) < 1) return;
      managerWidth = nextWidth;
      applyLayout();
    });
    observer.observe(managerRoot);
    managerWidth = managerRoot.clientWidth;
    applyLayout();
    return () => observer.disconnect();
  });

  onDestroy(() => {
    releasePendingPointer();
    if (dragFrame !== null) cancelAnimationFrame(dragFrame);
    if (settleTimer) clearTimeout(settleTimer);
    if (handoffFrame !== null) cancelAnimationFrame(handoffFrame);
    if (drag) restoreDocumentDragState();
  });

  $effect(() => {
    const ordered = orderMusicPlaylists(playlists);
    if (!drag && !settling && !keyboardPlaylistId && !saving
      && ordered.map((playlist) => playlist.id).join("|") !== ids().join("|")) {
      visualPlaylists = ordered;
    }
  });

  $effect(() => {
    const sourceIds = orderMusicPlaylists(playlists).map((playlist) => playlist.id);
    const sourceIdSet = new Set(sourceIds);
    const initialIdSet = new Set(initialIds);
    if (sourceIds.length !== initialIds.length || sourceIds.some((id) => !initialIdSet.has(id))) {
      initialIds = [
        ...initialIds.filter((id) => sourceIdSet.has(id)),
        ...sourceIds.filter((id) => !initialIdSet.has(id)),
      ];
    }
  });

  $effect(() => {
    void visualPlaylists.map((playlist) => playlist.id).join("|");
    void managerWidth;
    applyLayout();
  });

  $effect(() => {
    actionsDisabled = saving || Boolean(drag) || Boolean(pendingPointer);
  });
</script>

<svelte:window
  onkeydown={(event) => { if (event.key === "Escape") cancelActiveReorder(); }}
  onpointermove={movePointerDrag}
  onpointerup={(event) => finishPointerDrag(event, false)}
  onpointercancel={(event) => finishPointerDrag(event, true)}
/>

{#if showHeader}
  <div class="sticky top-0 z-20 -mx-1 mb-3 flex min-w-0 items-center justify-between gap-3 px-1 pb-2" style="background-color: var(--cal-bg);">
    <div class="min-w-0">
      <h2 class="text-sm font-semibold">{headerTitle ?? t("music.builder.managePlaylists")}</h2>
      {#if headerDescription !== null}<p class="mt-0.5 text-[0.68rem] leading-relaxed text-muted-foreground">{headerDescription ?? t("music.builder.managePlaylistsHint")}</p>{/if}
    </div>
    <div class="flex shrink-0 items-center gap-2">
      <button type="button" onclick={() => { void cancelManaging(); }} disabled={actionsDisabled} class="h-8 rounded-lg px-3 text-xs font-medium text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50">{t("music.builder.cancel")}</button>
      <button type="button" onclick={() => { void finishManaging(); }} disabled={actionsDisabled} class="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50">{t("music.builder.doneManaging")}</button>
    </div>
  </div>
{/if}
<div bind:this={managerRoot} class="relative w-full" style={`height: ${layoutHeight}px;`} aria-busy={saving} role="list">
  {#each renderedPlaylists as playlist (playlist.id)}
    {@const protectedPlaylist = isSystemMusicPlaylistId(playlist.id)}
    {@const playlistName = systemMusicPlaylistName(playlist.id, playlist.name, t)}
    {@const visualIndex = visualPlaylists.findIndex((entry) => entry.id === playlist.id)}
    {@const position = positions[playlist.id]}
    <div
      class={`absolute left-0 top-0 min-w-0 will-change-transform ${drag?.playlistId === playlist.id ? "cursor-grabbing opacity-95 shadow-floating" : settling?.playlistId === playlist.id ? "playlist-drop-settling opacity-95 shadow-floating" : ""} ${(drag || keyboardPlaylistId) && drag?.playlistId !== playlist.id && settling?.playlistId !== playlist.id && handoffId !== playlist.id ? "motion-safe:transition-transform motion-safe:duration-150 motion-safe:ease-out" : ""}`}
      style={wrapperStyle(playlist.id, position)}
      data-playlist-manager-id={playlist.id}
      role="listitem"
    >
      <div class={`playlist-manager-row flex h-13.5 w-full min-w-0 items-center gap-2 rounded-xl px-2 py-2 ${drag?.playlistId === playlist.id || keyboardPlaylistId === playlist.id ? "is-reordering" : ""}`}>
        <button
          type="button"
          class={`grid h-9 w-7 shrink-0 touch-none place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60 ${drag?.playlistId === playlist.id ? "cursor-grabbing" : "cursor-grab"}`}
          aria-label={t("music.builder.reorderPlaylist", playlistName, visualIndex + 1, visualPlaylists.length)}
          aria-pressed={keyboardPlaylistId === playlist.id}
          data-app-tooltip-disabled="true"
          disabled={saving}
          onpointerdown={(event) => startPointerDrag(event, playlist.id)}
          onkeydown={(event) => handleKeyboard(event, playlist)}
        ><GripVertical size={16} /></button>
        <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-secondary text-foreground"><MusicPlaylistIcon icon={playlist.icon} size={17} /></span>
        <span class="min-w-0 flex-1">
          <strong class="block truncate text-xs font-semibold">{playlistName}</strong>
          <span class="mt-0.5 block text-[0.64rem] tabular-nums text-muted-foreground">{t("music.tracks", playlist.totalCount)}</span>
        </span>
        {#if protectedPlaylist}
          <button type="button" class="grid h-8 w-8 shrink-0 cursor-not-allowed place-items-center rounded-lg text-muted-foreground opacity-35" aria-disabled="true" aria-label={t("music.builder.defaultPlaylistEditProtected")} title={t("music.builder.defaultPlaylistEditProtected")}><Pencil size={14} /></button>
          <button type="button" class="grid h-8 w-8 shrink-0 cursor-not-allowed place-items-center rounded-lg text-muted-foreground opacity-35" aria-disabled="true" aria-label={t("music.builder.defaultPlaylistDeleteProtected")} title={t("music.builder.defaultPlaylistDeleteProtected")}><Trash2 size={14} /></button>
        {:else}
          <button type="button" class="grid h-8 w-8 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => onEdit(playlist.id)} aria-label={t("music.builder.editNamedPlaylist", playlistName)} title={t("music.builder.editNamedPlaylist", playlistName)} disabled={saving}><Pencil size={14} /></button>
          <button type="button" class="grid h-8 w-8 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-destructive/10 hover:text-destructive" onclick={() => onDelete(playlist.id)} aria-label={t("music.builder.deleteNamedPlaylist", playlistName)} title={t("music.builder.deleteNamedPlaylist", playlistName)} disabled={saving}><Trash2 size={14} /></button>
        {/if}
      </div>
    </div>
  {/each}
</div>
{#if reorderError}<Notice class="mt-2 text-xs" message={t("music.builder.playlistReorderFailed")} />{/if}
<p class="sr-only" aria-live="polite">{announcement}</p>

<style>
  .playlist-manager-row { border: 1px solid color-mix(in srgb, var(--border) 58%, transparent); background: color-mix(in srgb, var(--card) 68%, transparent); transition: background-color 140ms ease, border-color 140ms ease, box-shadow 140ms ease; }
  .playlist-manager-row.is-reordering { border-color: color-mix(in srgb, var(--primary) 42%, var(--border)); background: color-mix(in srgb, var(--primary) 9%, var(--card)); box-shadow: var(--floating-shadow); }
  .playlist-drop-settling { transition-property: left, top, width; transition-duration: 160ms; transition-timing-function: cubic-bezier(0.2, 0, 0, 1); }
  @media (prefers-reduced-motion: reduce) { .playlist-manager-row { transition: none; } .playlist-drop-settling { transition-duration: 0ms; } }
</style>
