<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import GitCompareArrows from "@lucide/svelte/icons/git-compare-arrows";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import type { QuickNote, QuickNotesCollection, QuickNoteTag } from "$lib/quick-notes/types";
  import type { Theme } from "$lib/themes";
  import QuickNoteColorPicker from "./QuickNoteColorPicker.svelte";
  import QuickNoteRichText from "./QuickNoteRichText.svelte";
  import QuickNoteTagPicker from "./QuickNoteTagPicker.svelte";

  let {
    note,
    collection,
    theme,
    tags,
    reorderable = false,
    onOpen,
    onMove,
    onPin,
    onColor,
    onTag,
    onArchive,
    onUnarchive,
    onTrash,
    onRestore,
    onDelete,
    mobileLayout = false,
  }: {
    note: QuickNote;
    collection: QuickNotesCollection;
    theme: Theme;
    tags: readonly QuickNoteTag[];
    reorderable?: boolean;
    onOpen: () => void;
    onMove: (direction: -1 | 1) => void;
    onPin: (pinned: boolean) => void;
    onColor: (color: QuickNote["color"]) => void;
    onTag: (tagId: string | null) => void;
    onArchive: () => void;
    onUnarchive: () => void;
    onTrash: () => void;
    onRestore: () => void;
    onDelete: () => void;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const colors = $derived(getQuickNoteColor(note.color, theme));
  const tag = $derived(tags.find((candidate) => candidate.id === note.tagId) ?? null);
  const surfaceStyle = $derived(`--quick-card-bg: ${colors.bg}; --quick-card-fg: ${colors.text};`);
  const actionClass = "flex size-7 items-center justify-center rounded-md transition-colors hover:bg-black/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-current dark:hover:bg-white/10";
  /** Mobile cards are compact: their actions live in the editor and touch reorders with a long press. */
  const showHandle = $derived(reorderable && !mobileLayout);

  function handleReorderKeydown(event: KeyboardEvent): void {
    if (event.key !== "ArrowUp" && event.key !== "ArrowLeft" && event.key !== "ArrowDown" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation();
    onMove(event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 1);
  }
</script>

<article
  class="quick-note-card group relative overflow-hidden rounded-xl"
  style={surfaceStyle}
>
  {#if showHandle}
    <button
      type="button"
      class="quick-note-drag-handle absolute right-1 top-1 z-10 flex size-7 touch-none items-center justify-center rounded-md opacity-60 transition-opacity hover:bg-black/10 hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-current dark:hover:bg-white/10"
      data-quick-note-drag-handle
      aria-label={t("quickNotes.action.reorder")}
      title={t("quickNotes.action.reorderHint")}
      onkeydown={handleReorderKeydown}
    >
      <span class="grid grid-cols-2 gap-0.5" aria-hidden="true">
        {#each [1, 2, 3, 4, 5, 6] as _}<span class="size-0.5 rounded-full bg-current"></span>{/each}
      </span>
    </button>
  {/if}
  <button
    type="button"
    class={`block w-full px-3.5 pt-3 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-current ${mobileLayout ? "min-h-12 pb-3" : "pb-2"} ${showHandle ? "pr-9" : ""}`}
    onclick={onOpen}
    onkeydown={reorderable && !showHandle ? handleReorderKeydown : undefined}
  >
    {#if note.title}
      <h3 class="mb-1.5 wrap-break-word text-[0.92rem] font-semibold leading-snug">{note.title}</h3>
    {/if}
    {#if note.hasConflict || tag}
      <span class="mb-1.5 flex max-w-full flex-wrap items-center gap-1">
        {#if note.hasConflict}
          <span class="inline-flex items-center gap-1 rounded-md bg-black/8 px-1.5 py-0.5 text-[0.65rem] font-medium dark:bg-white/10" title={t("quickNotes.conflict.cardHint")}>
            <GitCompareArrows class="size-3" strokeWidth={1.75} aria-hidden="true" />{t("quickNotes.conflict.badge")}
          </span>
        {/if}
        {#if tag}<span class="inline-flex max-w-full truncate rounded-md bg-black/8 px-1.5 py-0.5 text-[0.65rem] font-medium dark:bg-white/10">{tag.name}</span>{/if}
      </span>
    {/if}
    {#if note.runs.length > 0}
      <div class="quick-note-preview relative max-h-65 overflow-hidden text-[0.82rem] leading-relaxed">
        <QuickNoteRichText runs={note.runs} />
        {#if note.previewTruncated || note.bodyPlainText.length > 1200}
          <span class="quick-note-fade pointer-events-none absolute inset-x-0 bottom-0 h-12"></span>
        {/if}
      </div>
    {/if}
  </button>
  {#if !mobileLayout}
    <div class="quick-note-actions flex min-h-9 cursor-default items-center gap-0.5 px-2 pb-1.5" role="toolbar" data-quick-note-no-drag>
      {#if collection === "active"}
        <button class={actionClass} type="button" aria-label={note.pinned ? t("quickNotes.action.unpin") : t("quickNotes.action.pin")} title={note.pinned ? t("quickNotes.action.unpin") : t("quickNotes.action.pin")} onclick={() => onPin(!note.pinned)}>
          {#if note.pinned}<PinOff class="size-3.5" strokeWidth={1.5} />{:else}<Pin class="size-3.5" strokeWidth={1.5} />{/if}
        </button>
      {/if}
      {#if collection !== "trash"}
        <QuickNoteColorPicker color={note.color} {theme} onSelect={onColor} buttonClass={actionClass} />
        <QuickNoteTagPicker tagId={note.tagId} {tags} onSelect={onTag} buttonClass={actionClass} />
        <button class={actionClass} type="button" aria-label={collection === "archive" ? t("quickNotes.action.unarchive") : t("quickNotes.action.archive")} title={collection === "archive" ? t("quickNotes.action.unarchive") : t("quickNotes.action.archive")} onclick={collection === "archive" ? onUnarchive : onArchive}>
          {#if collection === "archive"}<ArchiveRestore class="size-3.5" strokeWidth={1.5} />{:else}<Archive class="size-3.5" strokeWidth={1.5} />{/if}
        </button>
        <button class={actionClass} type="button" aria-label={t("quickNotes.action.trash")} title={t("quickNotes.action.trash")} onclick={onTrash}>
          <Trash2 class="size-3.5" strokeWidth={1.5} />
        </button>
      {:else}
        <button class={actionClass} type="button" aria-label={t("quickNotes.action.restore")} title={t("quickNotes.action.restore")} onclick={onRestore}>
          <RotateCcw class="size-3.5" strokeWidth={1.5} />
        </button>
        <button class={actionClass} type="button" aria-label={t("quickNotes.action.deletePermanently")} title={t("quickNotes.action.deletePermanently")} onclick={onDelete}>
          <Trash2 class="size-3.5" strokeWidth={1.5} />
        </button>
      {/if}
    </div>
  {/if}
</article>

<style>
  .quick-note-card {
    background: var(--quick-card-bg);
    color: var(--quick-card-fg);
  }

  .quick-note-fade {
    background: linear-gradient(to bottom, transparent, var(--quick-card-bg));
  }

  @media (hover: hover) and (pointer: fine) {
    .quick-note-actions {
      opacity: 0;
      transition: opacity 120ms ease;
    }

    .quick-note-card:hover .quick-note-actions,
    .quick-note-card:focus-within .quick-note-actions {
      opacity: 1;
    }
  }
</style>
