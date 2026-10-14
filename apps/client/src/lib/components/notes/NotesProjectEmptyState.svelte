<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { shortcutParts } from "$lib/keyboard-shortcuts";

  /** Centered canvas state for a project without notes, offering the first note and its shortcut. */
  let {
    projectName,
    onCreate,
  }: {
    projectName: string;
    onCreate: () => void;
  } = $props();

  const { t } = getLocalization();
  const newPageShortcutParts = shortcutParts("Mod + N");
  const titleId = $props.id();
</script>

<section
  class="flex min-h-0 min-w-0 flex-1 overflow-y-auto p-4"
  aria-labelledby={titleId}
  data-notes-project-empty
>
  <div class="m-auto flex max-w-sm flex-col items-center text-center">
    <h2 id={titleId} class="text-sm text-pretty text-muted-foreground">{t("notes.emptyProjectTitle", projectName)}</h2>
    <button
      type="button"
      class="mt-4 inline-flex h-9 shrink-0 items-center justify-center gap-1.5 rounded-md bg-primary px-3.5 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90"
      aria-keyshortcuts="Control+N Meta+N"
      onclick={onCreate}
    >
      <Plus size={16} strokeWidth={2.25} />
      {t("notes.newPage")}
    </button>
    <p class="mt-3 flex flex-wrap items-center justify-center gap-1 text-xs text-muted-foreground">
      <span>{t("notes.emptyProjectShortcutHint")}</span>
      {#each newPageShortcutParts as part, partIndex}
        {#if partIndex > 0}<span aria-hidden="true">+</span>{/if}
        <kbd class="inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-background px-1 font-mono text-[0.6875rem] leading-none text-foreground shadow-sm">{part}</kbd>
      {/each}
    </p>
  </div>
</section>
