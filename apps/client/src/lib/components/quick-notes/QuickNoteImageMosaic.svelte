<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    QUICK_NOTE_CARD_IMAGE_ROWS,
    QUICK_NOTE_EDITOR_IMAGE_ROWS,
    quickNoteCardImagePreview,
    quickNoteImageMosaic,
  } from "$lib/quick-notes/image-layout";
  import type { QuickNoteImage } from "$lib/quick-notes/types";
  import QuickNoteImageTile from "./QuickNoteImageTile.svelte";

  let {
    images,
    variant,
    onOpen,
    onRemove,
  }: {
    images: readonly QuickNoteImage[];
    /**
     * `preview` shows a few static tiles and counts the rest, for cards and conflict versions. `editor` shows every
     * image with open and remove buttons.
     */
    variant: "preview" | "editor";
    onOpen?: (index: number) => void;
    /** Omitted when the images cannot be removed. */
    onRemove?: (index: number) => void;
  } = $props();

  const { t } = getLocalization();
  const preview = $derived(
    variant === "preview" ? quickNoteCardImagePreview(images) : { shown: [...images], hidden: 0 },
  );
  const rows = $derived(
    quickNoteImageMosaic(preview.shown, variant === "preview" ? QUICK_NOTE_CARD_IMAGE_ROWS : QUICK_NOTE_EDITOR_IMAGE_ROWS),
  );
  const lastShown = $derived(preview.shown.length - 1);
</script>

<div class="flex flex-col gap-0.5" data-quick-note-images>
  {#each rows as row, rowIndex (rowIndex)}
    <div class="mx-auto flex gap-0.5" style:width={`${row.width * 100}%`}>
      {#each row.tiles as tile (preview.shown[tile.index]?.sha256 ?? tile.index)}
        {@const image = preview.shown[tile.index]}
        {#if image}
          <div class="quick-note-mosaic-tile relative min-w-0" style:flex={`${tile.share} ${tile.share} 0%`} style:aspect-ratio={tile.aspect}>
            {#if variant === "editor"}
              <button
                type="button"
                class="block size-full cursor-zoom-in outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-current"
                aria-label={t("quickNotes.images.open", tile.index + 1, images.length)}
                data-quick-note-image-index={tile.index}
                onclick={() => onOpen?.(tile.index)}
              >
                <QuickNoteImageTile {image} />
              </button>
              {#if onRemove}
                <button
                  type="button"
                  class="quick-note-image-remove absolute top-1.5 right-1.5 flex size-7 items-center justify-center rounded-full bg-black/55 text-white transition-opacity hover:bg-black/75 focus-visible:outline-2 focus-visible:outline-white"
                  aria-label={t("quickNotes.images.remove", tile.index + 1, images.length)}
                  title={t("quickNotes.images.viewer.remove")}
                  onclick={() => onRemove?.(tile.index)}
                >
                  <X class="size-4" strokeWidth={2} aria-hidden="true" />
                </button>
              {/if}
            {:else}
              <QuickNoteImageTile {image} />
              {#if preview.hidden > 0 && tile.index === lastShown}
                <span
                  class="absolute inset-0 flex items-center justify-center bg-black/45 text-lg font-semibold text-white"
                  title={t("quickNotes.images.moreLabel", preview.hidden)}
                >
                  <span aria-hidden="true">{t("quickNotes.images.more", preview.hidden)}</span>
                  <span class="sr-only">{t("quickNotes.images.moreLabel", preview.hidden)}</span>
                </span>
              {/if}
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/each}
</div>

<style>
  @media (hover: hover) and (pointer: fine) {
    .quick-note-image-remove {
      opacity: 0;
    }

    .quick-note-mosaic-tile:hover .quick-note-image-remove,
    .quick-note-image-remove:focus-visible {
      opacity: 1;
    }
  }
</style>
