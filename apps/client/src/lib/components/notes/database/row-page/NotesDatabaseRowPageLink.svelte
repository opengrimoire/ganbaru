<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesPageIcon as NotesPageIconValue } from "$lib/notes/types";
  import { cn } from "$lib/utils";

  /** A linked row on a row page: its icon and title open it, and a remove button unlinks it when allowed. */
  let { title, icon = null, removeLabel, disabled = false, onOpen, onRemove }: {
    title: string;
    icon?: NotesPageIconValue | null;
    /** Accessible name of the remove button. */
    removeLabel: string;
    disabled?: boolean;
    onOpen: () => void;
    /** Unlinks the row; the remove button is hidden without it. */
    onRemove?: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<span class="group/link inline-flex max-w-full min-w-0 items-center rounded-sm hover:bg-accent/60">
  <button type="button" class="inline-flex h-7 min-w-0 cursor-pointer items-center gap-1.5 rounded-sm px-1.5 text-left outline-none focus-visible:bg-accent"
    aria-label={t("notes.databaseRowsOpen", title || t("notes.untitled"))} onclick={onOpen}>
    <NotesPageIcon {icon} size={16} class="shrink-0 text-muted-foreground" />
    <span class={cn("min-w-0 truncate underline decoration-border underline-offset-2", !title && "text-muted-foreground")}>{title || t("notes.untitled")}</span>
  </button>
  {#if onRemove}
    <button type="button"
      class="mr-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground opacity-100 hover:bg-accent hover:text-foreground focus-visible:opacity-100 disabled:pointer-events-none disabled:opacity-50 [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/link:opacity-100"
      {disabled} aria-label={removeLabel} data-app-tooltip={removeLabel} onclick={onRemove}>
      <X class="size-3.5" aria-hidden="true" />
    </button>
  {/if}
</span>
