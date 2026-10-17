<script lang="ts">
  import { onDestroy } from "svelte";
  import { notesPageCoverAssetUrl } from "$lib/api/notes/page-covers";
  import NotesCoverDesign from "./NotesCoverDesign.svelte";
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import {
    isNotesPageCoverAssetPath,
    notesPageCoverAssetPath,
    notesPageCoverUrl,
    notesCoverObjectPosition,
    notesCoverFocalPointFromDrag,
    NOTES_COVER_DEFAULT_FOCAL_POINT,
  } from "$lib/notes/pages/cover";
  import type { NotesCoverFocalPoint } from "$lib/notes/contracts/assets";
  import type { NotesPageCover } from "$lib/notes/types";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import ImageIcon from "@lucide/svelte/icons/image";

  let {
    cover,
    unavailableLabel,
    objectFit = "cover",
    previewUrl = null,
    focalLabel = "",
    focalPoint,
    onFocalPoint,
    onStatus,
    positioningDisabled = false,
  }: {
    cover: NotesPageCover | null;
    unavailableLabel: string;
    objectFit?: "cover" | "contain";
    previewUrl?: string | null;
    focalLabel?: string;
    focalPoint?: NotesCoverFocalPoint;
    onFocalPoint?: (point: NotesCoverFocalPoint) => void;
    positioningDisabled?: boolean;
    onStatus?: (status: "loading" | "ready" | "error") => void;
  } = $props();

  let assetUrl = $state<string | null>(null);
  let assetFailed = $state(false);
  let failedUrl = $state<string | null>(null);
  let loadedUrl = $state<string | null>(null);
  let width = $state(0);
  let height = $state(0);
  let imageSize = $state({ width: 0, height: 0 });
  const assetPath = $derived(notesPageCoverAssetPath(cover));
  const remoteImageUrlsAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "notes.external-image-references");
  const url = $derived(previewUrl ?? assetUrl ?? (remoteImageUrlsAvailable ? notesPageCoverUrl(cover) : null));
  const imageLoading = $derived(cover?.type !== "design" && !assetFailed && (
    url ? loadedUrl !== url && failedUrl !== url : !!assetPath && isNotesPageCoverAssetPath(assetPath)
  ));
  let dragPreview = $state<NotesCoverFocalPoint | null>(null);
  let pendingPoint: NotesCoverFocalPoint | null = null;
  let previewFrame: number | null = null;
  const focal = $derived(dragPreview ?? focalPoint ?? (cover && cover.type !== "design" ? cover.focal_point ?? NOTES_COVER_DEFAULT_FOCAL_POINT : NOTES_COVER_DEFAULT_FOCAL_POINT));
  const objectPosition = $derived(objectFit === "contain" ? "50% 50%" : notesCoverObjectPosition(focal, imageSize, { width, height }));
  // Move a composited image while editing instead of repainting its object-position.
  const editableImage = $derived.by(() => {
    if (!onFocalPoint || !imageSize.width || !imageSize.height || !width || !height) return null;
    const scale = Math.max(width / imageSize.width, height / imageSize.height);
    const scaledWidth = imageSize.width * scale;
    const scaledHeight = imageSize.height * scale;
    const x = -Math.max(0, Math.min(scaledWidth - width, focal.x * scaledWidth - width / 2));
    const y = -Math.max(0, Math.min(scaledHeight - height, focal.y * scaledHeight - height / 2));
    return { width: scaledWidth, height: scaledHeight, transform: `translate3d(${x}px, ${y}px, 0)` };
  });
  let drag: { pointerId: number; x: number; y: number; focal: NotesCoverFocalPoint } | null = null;
  let dragging = $state(false);

  /** Cancel scheduled rendering when editing ends or the cover is removed. */
  function clearDrag(): void {
    if (previewFrame !== null) cancelAnimationFrame(previewFrame);
    previewFrame = null;
    pendingPoint = null;
    dragPreview = null;
    drag = null;
    dragging = false;
  }

  onDestroy(clearDrag);
  $effect(() => { if (!onFocalPoint) clearDrag(); });

  $effect(() => {
    const path = assetPath;
    let active = true;
    assetUrl = null;
    assetFailed = false;
    if (path && isNotesPageCoverAssetPath(path)) {
      void notesPageCoverAssetUrl(path).then((value) => {
        if (active) assetUrl = value;
      }).catch(() => {
        if (active) assetFailed = true;
      });
    }
    return () => { active = false; };
  });

  $effect(() => {
    if (cover?.type === "design") onStatus?.("ready");
    else if (assetFailed || (!url && cover && (!assetPath || !isNotesPageCoverAssetPath(assetPath)))) onStatus?.("error");
    else if (url && failedUrl === url) onStatus?.("error");
    else if (url && loadedUrl === url) onStatus?.("ready");
    else onStatus?.("loading");
  });

  /** Capture one pointer so dragging continues outside the banner on mouse and touch. */
  function startDrag(event: PointerEvent): void {
    if (positioningDisabled || event.button !== 0 || drag || !url || loadedUrl !== url || failedUrl === url) return;
    if (!(event.currentTarget instanceof HTMLElement)) return;
    event.preventDefault();
    event.currentTarget.focus({ preventScroll: true });
    event.currentTarget.setPointerCapture(event.pointerId);
    drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, focal: { ...focal } };
    dragging = true;
  }

  /** Render the latest pointer position once per frame, within the cover only. */
  function moveDrag(event: PointerEvent): void {
    if (!drag || drag.pointerId !== event.pointerId || positioningDisabled) return;
    pendingPoint = notesCoverFocalPointFromDrag(drag.focal,
      { x: event.clientX - drag.x, y: event.clientY - drag.y }, imageSize, { width, height });
    if (previewFrame !== null) return;
    previewFrame = requestAnimationFrame(() => {
      previewFrame = null;
      dragPreview = pendingPoint;
    });
  }

  /** Transfer the last preview to the editor draft, including moves before the next frame. */
  function stopDrag(event: PointerEvent): void {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.type === "pointerup") moveDrag(event);
    const point = pendingPoint;
    if (point && !positioningDisabled) onFocalPoint?.(point);
    clearDrag();
    if (event.currentTarget instanceof HTMLElement && event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  /** Offer the same two-axis positioning through keyboard arrows. */
  function moveFocalPoint(event: KeyboardEvent): void {
    if (positioningDisabled) return;
    const step = (event.shiftKey ? 0.1 : 0.02) * Math.max(width, height);
    const offsets: Partial<Record<string, readonly [number, number]>> = {
      ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step],
    };
    const delta = offsets[event.key];
    if (!delta && event.key !== "Home") return;
    event.preventDefault();
    onFocalPoint?.(delta ? notesCoverFocalPointFromDrag(focal, { x: delta[0], y: delta[1] }, imageSize, { width, height }) : { ...NOTES_COVER_DEFAULT_FOCAL_POINT });
  }
</script>

{#snippet surface()}
  <NotesLoadingSkeleton kind="cover" ready={!imageLoading}>
  {#snippet children()}
  {#if url && failedUrl !== url}
    {#key url}
      <img
        class={editableImage ? "absolute left-0 top-0 max-w-none" : `size-full ${objectFit === "contain" ? "object-contain" : "object-cover"}`}
        style:object-position={editableImage ? undefined : objectPosition}
        style:width={editableImage ? `${editableImage.width}px` : undefined}
        style:height={editableImage ? `${editableImage.height}px` : undefined}
        style:transform={editableImage?.transform}
        style:will-change={editableImage ? "transform" : undefined}
        src={url}
        alt=""
        draggable="false"
        onload={(event) => {
          if (!(event.currentTarget instanceof HTMLImageElement)) return;
          imageSize = { width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight };
          loadedUrl = event.currentTarget.getAttribute("src");
        }}
        onerror={(event) => { failedUrl = event.currentTarget.getAttribute("src"); }}
      />
    {/key}

  {:else if !imageLoading}
    <div class="flex size-full items-center justify-center gap-2 bg-muted text-[0.8rem] text-muted-foreground">
      <ImageIcon class="size-4" />
      <span class="max-w-full truncate px-2">{unavailableLabel}</span>
    </div>
  {/if}
  {/snippet}
  </NotesLoadingSkeleton>
{/snippet}

{#if cover?.type === "design"}
  <NotesCoverDesign pattern={cover.design.pattern} color={cover.design.color} />
{:else if onFocalPoint}
  <button type="button" class="relative block size-full touch-none select-none overflow-hidden focus-visible:ring-2 focus-visible:ring-ring" bind:clientWidth={width} bind:clientHeight={height} data-cover-drag data-app-tooltip-disabled="true" aria-label={focalLabel} aria-disabled={positioningDisabled} style:cursor={dragging ? "grabbing" : "grab"} onpointerdown={startDrag} onpointermove={moveDrag} onpointerup={stopDrag} onpointercancel={stopDrag} onlostpointercapture={stopDrag} onkeydown={moveFocalPoint}>
    {@render surface()}
  </button>
{:else if objectFit === "contain"}
  <div class="relative size-full overflow-hidden">{@render surface()}</div>
{:else}
  <div class="relative size-full overflow-hidden" bind:clientWidth={width} bind:clientHeight={height}>
    {@render surface()}
  </div>
{/if}
