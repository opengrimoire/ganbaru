<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { portal } from "$lib/utils/portal";

  interface ChatImageGalleryItem {
    id: string;
    displayName: string;
    byteSize: number | null;
  }

  const {
    images,
    variant = "message",
    onRemove,
  } = $props<{
    images: ChatImageGalleryItem[];
    variant?: "composer" | "message";
    onRemove?: (id: string) => void;
  }>();

  const MIN_ZOOM = 0.5;
  const MAX_ZOOM = 4;
  const ZOOM_STEP = 0.25;
  const localization = getLocalization();
  const { t } = localization;
  let imageUrls = $state<Record<string, string>>({});
  let failedImageIds = $state<string[]>([]);
  let activeImageId = $state<string | null>(null);
  let zoom = $state(1);
  let previewDialog: HTMLDivElement | undefined = $state();
  let previewReturnFocus: HTMLElement | null = null;
  let destroyed = false;
  const imageRequests = new Map<string, Promise<string>>();
  const activeImage = $derived(images.find((image: ChatImageGalleryItem) => image.id === activeImageId) ?? null);
  const activeIndex = $derived(activeImage ? images.findIndex((image: ChatImageGalleryItem) => image.id === activeImage.id) : -1);

  $effect(() => {
    for (const image of images) {
      if (imageUrls[image.id] || failedImageIds.includes(image.id)) continue;
      void loadImage(image.id).catch(() => undefined);
    }
  });

  onDestroy(() => { destroyed = true; });

  function loadImage(imageId: string): Promise<string> {
    const loaded = imageUrls[imageId];
    if (loaded) return Promise.resolve(loaded);
    const existing = imageRequests.get(imageId);
    if (existing) return existing;
    const request = chatApi.chatAttachmentDataUrl(imageId).then((url) => {
      if (!destroyed) imageUrls = { ...imageUrls, [imageId]: url };
      return url;
    }).catch((cause: unknown) => {
      if (!destroyed && !failedImageIds.includes(imageId)) {
        failedImageIds = [...failedImageIds, imageId];
      }
      throw cause;
    });
    imageRequests.set(imageId, request);
    return request;
  }

  async function openPreview(imageId: string): Promise<void> {
    previewReturnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    try {
      await loadImage(imageId);
    } catch {
      return;
    }
    activeImageId = imageId;
    zoom = 1;
    await tick();
    previewDialog?.querySelector<HTMLElement>("button")?.focus();
  }

  function closePreview(): void {
    activeImageId = null;
    zoom = 1;
    const target = previewReturnFocus;
    queueMicrotask(() => target?.isConnected && target.focus());
  }

  function changeImage(offset: number): void {
    if (images.length < 2 || activeIndex < 0) return;
    const nextIndex = (activeIndex + offset + images.length) % images.length;
    activeImageId = images[nextIndex]?.id ?? activeImageId;
    zoom = 1;
  }

  function changeZoom(delta: number): void {
    zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom + delta));
  }

  function handlePreviewKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closePreview();
      return;
    }
    if (event.key === "ArrowLeft" && images.length > 1) {
      event.preventDefault();
      changeImage(-1);
      return;
    }
    if (event.key === "ArrowRight" && images.length > 1) {
      event.preventDefault();
      changeImage(1);
      return;
    }
    if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      changeZoom(ZOOM_STEP);
      return;
    }
    if (event.key === "-") {
      event.preventDefault();
      changeZoom(-ZOOM_STEP);
      return;
    }
    if (event.key === "0") {
      event.preventDefault();
      zoom = 1;
      return;
    }
    if (event.key !== "Tab" || !previewDialog) return;
    const controls = [...previewDialog.querySelectorAll<HTMLElement>("button:not(:disabled)")];
    if (controls.length === 0) return;
    const currentIndex = controls.indexOf(document.activeElement as HTMLElement);
    const nextIndex = event.shiftKey
      ? currentIndex <= 0 ? controls.length - 1 : currentIndex - 1
      : currentIndex >= controls.length - 1 ? 0 : currentIndex + 1;
    event.preventDefault();
    controls[nextIndex]?.focus();
  }
</script>

<div class:composer={variant === "composer"} class:message={variant === "message"} class="chat-image-gallery">
  {#each images as image (image.id)}
    <article class="chat-image-tile">
      <button
        type="button"
        class="attachment-preview"
        aria-label={t("chat.composer.previewAttachment", image.displayName)}
        onclick={() => void openPreview(image.id)}
      >
        {#if imageUrls[image.id]}
          <img src={imageUrls[image.id]} alt={image.displayName} />
        {:else if failedImageIds.includes(image.id)}
          <span>{image.displayName}</span>
        {:else}
          <LoaderCircle size={16} class="animate-spin" />
        {/if}
      </button>
      {#if variant === "composer" && onRemove}
        <button
          type="button"
          class="chat-image-remove"
          aria-label={t("chat.composer.removeAttachment", image.displayName)}
          onclick={() => onRemove(image.id)}
        ><X size={13} /></button>
      {/if}
    </article>
  {/each}
</div>

{#if activeImage && imageUrls[activeImage.id]}
  <div use:portal class="chat-image-lightbox">
    <button type="button" class="chat-image-backdrop" aria-label={t("chat.cancel")} onclick={closePreview}></button>
    <div
      bind:this={previewDialog}
      class="chat-image-dialog"
      role="dialog"
      aria-modal="true"
      aria-label={t("chat.composer.previewAttachment", activeImage.displayName)}
      tabindex="-1"
      onkeydown={handlePreviewKeydown}
    >
      <header>
        <div class="min-w-0 flex-1">
          <strong>{activeImage.displayName}</strong>
          {#if activeImage.byteSize !== null}<span>{formatNumber(localization.locale, activeImage.byteSize)} B</span>{/if}
        </div>
        <div class="chat-image-controls">
          <button type="button" aria-label={t("chat.composer.zoomOut")} title={t("chat.composer.zoomOut")} disabled={zoom <= MIN_ZOOM} onclick={() => changeZoom(-ZOOM_STEP)}><Minus size={15} /></button>
          <button type="button" aria-label={t("chat.composer.resetZoom")} title={t("chat.composer.resetZoom")} onclick={() => { zoom = 1; }}><RotateCcw size={14} /><span>{formatNumber(localization.locale, zoom * 100, { maximumFractionDigits: 0 })}%</span></button>
          <button type="button" aria-label={t("chat.composer.zoomIn")} title={t("chat.composer.zoomIn")} disabled={zoom >= MAX_ZOOM} onclick={() => changeZoom(ZOOM_STEP)}><Plus size={15} /></button>
          <button type="button" aria-label={t("chat.cancel")} title={t("chat.cancel")} onclick={closePreview}><X size={16} /></button>
        </div>
      </header>
      <div class="chat-image-viewport">
        <img style={`width:${zoom * 100}%`} src={imageUrls[activeImage.id]} alt={activeImage.displayName} />
      </div>
      {#if images.length > 1}
        <button type="button" class="chat-image-previous" aria-label={t("chat.composer.previousImage")} onclick={() => changeImage(-1)}><ChevronLeft size={20} /></button>
        <button type="button" class="chat-image-next" aria-label={t("chat.composer.nextImage")} onclick={() => changeImage(1)}><ChevronRight size={20} /></button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .chat-image-gallery { display: flex; max-width: 100%; flex-wrap: wrap; gap: 0.45rem; }
  .chat-image-gallery.composer { margin-top: 0.65rem; }
  .chat-image-gallery.message { margin-top: 0.6rem; }
  .chat-image-tile { position: relative; overflow: hidden; border: 1px solid color-mix(in srgb, var(--border) 88%, transparent); border-radius: 0.75rem; background: var(--muted); }
  .composer .chat-image-tile { width: 5.5rem; height: 5.5rem; }
  .message .chat-image-tile { width: clamp(6.5rem, 18vw, 9rem); height: clamp(6.5rem, 18vw, 9rem); }
  .attachment-preview { display: grid; width: 100%; height: 100%; place-items: center; overflow: hidden; color: var(--muted-foreground); }
  .attachment-preview img { width: 100%; height: 100%; object-fit: cover; transition: transform 160ms ease, filter 160ms ease; }
  .attachment-preview:hover img { filter: brightness(1.04); transform: scale(1.025); }
  .attachment-preview > span { max-width: 100%; overflow: hidden; padding: 0.5rem; font-size: calc(0.666667rem * var(--type-scale)); overflow-wrap: anywhere; }
  .chat-image-remove { position: absolute; top: 0.3rem; right: 0.3rem; display: grid; width: 1.45rem; height: 1.45rem; place-items: center; border: 1px solid rgb(255 255 255 / 0.3); border-radius: 999px; background: rgb(10 10 10 / 0.82); color: white; box-shadow: 0 2px 8px rgb(0 0 0 / 0.25); }
  .chat-image-lightbox { position: fixed; inset: 0; z-index: 80; display: grid; place-items: center; padding: max(0.5rem, 2vmin); }
  .chat-image-lightbox :global(svg.lucide) { stroke-width: var(--icon-stroke-width); }
  .chat-image-backdrop { position: absolute; inset: 0; background: rgb(0 0 0 / 0.72); backdrop-filter: blur(8px); }
  .chat-image-dialog { position: relative; display: grid; width: min(94vw, 80rem); height: min(92vh, 60rem); min-height: 0; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; border: 1px solid color-mix(in srgb, var(--border) 80%, white 10%); border-radius: 1rem; background: color-mix(in srgb, var(--background) 96%, transparent); box-shadow: 0 24px 80px rgb(0 0 0 / 0.45); }
  .chat-image-dialog > header { display: flex; min-width: 0; flex-wrap: wrap; align-items: center; gap: 0.5rem; border-bottom: 1px solid var(--border); padding: 0.6rem 0.7rem; }
  .chat-image-dialog > header strong, .chat-image-dialog > header span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-image-dialog > header strong { font-size: calc(0.8rem * var(--type-scale)); }
  .chat-image-dialog > header span { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .chat-image-controls { display: flex; flex: 0 0 auto; align-items: center; gap: 0.2rem; }
  .chat-image-controls button, .chat-image-previous, .chat-image-next { display: inline-flex; min-width: 2rem; height: 2rem; align-items: center; justify-content: center; gap: 0.25rem; border-radius: 0.5rem; color: var(--muted-foreground); }
  .chat-image-controls button:hover:not(:disabled), .chat-image-previous:hover, .chat-image-next:hover { background: var(--accent); color: var(--foreground); }
  .chat-image-controls button:disabled { opacity: 0.35; }
  .chat-image-viewport { display: grid; min-height: 0; overflow: auto; place-items: center; overscroll-behavior: contain; background: color-mix(in srgb, black 38%, var(--background)); scrollbar-gutter: stable; }
  .chat-image-viewport img { height: auto; max-width: none; object-fit: contain; }
  .chat-image-previous, .chat-image-next { position: absolute; top: 50%; border: 1px solid rgb(255 255 255 / 0.16); background: rgb(0 0 0 / 0.58); color: white; transform: translateY(-50%); }
  .chat-image-previous { left: 0.6rem; }
  .chat-image-next { right: 0.6rem; }
  @media (max-width: 420px), (max-height: 320px) {
    .chat-image-dialog { width: calc(100vw - 0.5rem); height: calc(100vh - 0.5rem); border-radius: 0.65rem; }
    .chat-image-dialog > header { padding: 0.35rem; }
    .chat-image-dialog > header > div:first-child { flex-basis: 100%; }
    .chat-image-controls { width: 100%; justify-content: flex-end; }
  }
</style>
