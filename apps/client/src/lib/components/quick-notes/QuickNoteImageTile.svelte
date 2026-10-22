<script lang="ts">
  import { untrack } from "svelte";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import ImageOff from "@lucide/svelte/icons/image-off";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    quickNoteImageCache,
    type QuickNoteImageState,
    type QuickNoteImageVariant,
  } from "$lib/quick-notes/image-cache";
  import type { QuickNoteImage } from "$lib/quick-notes/types";

  let {
    image,
    variant = "thumbnail",
    lazy = true,
  }: {
    image: QuickNoteImage;
    variant?: QuickNoteImageVariant;
    /** Waits until the tile is near the viewport before reading the file. */
    lazy?: boolean;
  } = $props();

  /** Starts reading images a little before they scroll into view. */
  const LAZY_MARGIN = "600px";
  const { t } = getLocalization();
  let host = $state<HTMLSpanElement | null>(null);
  let imageState = $state.raw<QuickNoteImageState>({ status: "loading" });
  let decoded = $state(false);
  const sha256 = $derived(image.sha256);
  const url = $derived(imageState.status === "ready" ? imageState.url : null);

  function show(next: QuickNoteImageState): void {
    untrack(() => {
      if (next.status !== "ready" || imageState.status !== "ready" || next.url !== imageState.url) decoded = false;
      imageState = next;
    });
  }

  // Keyed on the file, since a refreshed note list hands over equal images as new objects.
  $effect(() => {
    void sha256;
    const node = host;
    if (!node) return;
    const current = untrack(() => image);
    const shownVariant = variant;
    let stop: (() => void) | null = null;
    const start = (): void => {
      stop ??= quickNoteImageCache().subscribe(current, shownVariant, show);
    };
    if (!lazy || typeof IntersectionObserver === "undefined") {
      start();
      return () => stop?.();
    }
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      start();
    }, { rootMargin: LAZY_MARGIN });
    observer.observe(node);
    return () => {
      observer.disconnect();
      stop?.();
    };
  });
</script>

<span bind:this={host} class="quick-note-image-tile relative block size-full overflow-hidden">
  {#if url}
    <img
      src={url}
      alt=""
      draggable="false"
      decoding="async"
      class="block size-full object-cover transition-opacity duration-150 motion-reduce:transition-none"
      class:opacity-0={!decoded}
      onload={() => { decoded = true; }}
    />
  {/if}
  {#if imageState.status === "failed"}
    <span
      class="absolute inset-0 flex items-center justify-center opacity-60"
      title={imageState.code === "unavailable" ? t("quickNotes.images.unavailable") : t("quickNotes.images.loadFailed")}
    >
      {#if imageState.code === "unavailable"}
        <CloudDownload class="size-5" strokeWidth={1.5} aria-hidden="true" />
      {:else}
        <ImageOff class="size-5" strokeWidth={1.5} aria-hidden="true" />
      {/if}
    </span>
  {/if}
</span>

<style>
  .quick-note-image-tile {
    background: color-mix(in srgb, currentColor 9%, transparent);
  }
</style>
