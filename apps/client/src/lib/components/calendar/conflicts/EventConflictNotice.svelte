<script lang="ts">
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { calendarConflicts } from "$lib/stores/calendar-conflicts.svelte";
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";

  let {
    eventId,
    blocked = false,
    onResolved,
  }: {
    /** Id of the event, or of the series an occurrence belongs to. */
    eventId: string;
    /** Whether the panel holds unsaved edits, which a resolution would leave out of date. */
    blocked?: boolean;
    /** Called after the dialog closes with at least one conflict resolved, so the panel can reload the event. */
    onResolved: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const conflicts = $derived(calendarConflicts.forEvent(eventId));
  const hasTiming = $derived(conflicts.some((conflict) => conflict.field === "timing"));
  const fields = $derived(formatList(
    localization.locale,
    conflicts.map((conflict) => {
      if (conflict.field === "timing") return t("calendar.conflict.fieldTiming");
      return conflict.field === "title"
        ? t("calendar.conflict.fieldTitle")
        : t("calendar.conflict.fieldDescription");
    }),
  ));
  let dialogOpen = $state(false);

  $effect(() => {
    void calendarConflicts.revision;
    void calendarConflicts.ensure();
  });

  function handleClose(resolved: boolean): void {
    dialogOpen = false;
    if (resolved) onResolved();
  }
</script>

{#if conflicts.length > 0}
  <SyncConflictCard
    title={t("calendar.conflict.noticeTitle")}
    hint={t("calendar.conflict.noticeFields", fields)}
    message={blocked ? t("calendar.conflict.saveFirst") : ""}
  >
    {#if hasTiming}
      <div class="text-muted-foreground">{t("calendar.pomodoro.timingConflictNote")}</div>
    {/if}
    <div>
      <button
        class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
        type="button"
        disabled={blocked}
        onclick={() => { dialogOpen = true; }}
      >{t("calendar.conflict.review")}</button>
    </div>
  </SyncConflictCard>
{/if}
{#if dialogOpen}
  {#await import("./EventConflictDialog.svelte") then module}
    {@const EventConflictDialog = module.default}
    <EventConflictDialog {eventId} onClose={handleClose} />
  {/await}
{/if}
