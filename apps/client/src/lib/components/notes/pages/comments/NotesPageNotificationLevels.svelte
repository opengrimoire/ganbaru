<script lang="ts">
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    NOTES_PAGE_NOTIFICATION_LEVELS,
    type NotesPageNotificationLevel,
  } from "$lib/notes/contracts/collaboration";
  import Check from "@lucide/svelte/icons/check";

  /**
   * The selected page's notification choices as menu rows with a short description each. A page without a stored
   * choice notifies at the broadest level, so the rows show that level while the setting loads.
   */
  let {
    showHeading = true,
  }: {
    /** Off when the surrounding surface already titles the choices. */
    showHeading?: boolean;
  } = $props();

  const notes = getNotesEditor();
  const { t } = getLocalization();

  let errorMessage = $state<string | null>(null);
  let pending = $state(false);

  const current = $derived<NotesPageNotificationLevel>(notes.pageNotificationLevel ?? "all");

  function levelLabel(level: NotesPageNotificationLevel): string {
    switch (level) {
      case "all": return t("notes.pageNotificationLevelAll");
      case "mentions": return t("notes.pageNotificationLevelMentions");
      case "none": return t("notes.pageNotificationLevelNone");
    }
  }

  function levelDetail(level: NotesPageNotificationLevel): string {
    switch (level) {
      case "all": return t("notes.pageNotificationLevelAllDetail");
      case "mentions": return t("notes.pageNotificationLevelMentionsDetail");
      case "none": return t("notes.pageNotificationLevelNoneDetail");
    }
  }

  async function choose(level: NotesPageNotificationLevel): Promise<void> {
    if (pending || level === current) return;
    errorMessage = null;
    pending = true;
    try {
      await notes.setPageNotificationLevel(level);
    } catch (error) {
      errorMessage = t("notes.pageNotificationsFailed", error instanceof Error ? error.message : String(error));
    } finally {
      pending = false;
    }
  }
</script>

{#if showHeading}
  <p class="menu-label">{t("notes.pageNotifications")}</p>
{/if}
{#each NOTES_PAGE_NOTIFICATION_LEVELS as level (level)}
  {@const selected = level === current}
  <button
    type="button"
    class="menu-item min-w-0 items-start"
    aria-pressed={selected}
    disabled={pending}
    data-collection-row-two-line
    data-collection-menu-keep-open
    onclick={() => void choose(level)}
  >
    <span class="grid min-w-0 flex-1">
      <span class="truncate">{levelLabel(level)}</span>
      <span class="whitespace-normal text-panel-detail text-muted-foreground">{levelDetail(level)}</span>
    </span>
    <Check class={selected ? "mt-0.5 size-3.5 shrink-0" : "invisible mt-0.5 size-3.5 shrink-0"} aria-hidden="true" />
  </button>
{/each}
{#if errorMessage}
  <Notice class="px-2 py-1 text-panel-detail" message={errorMessage} />
{/if}
