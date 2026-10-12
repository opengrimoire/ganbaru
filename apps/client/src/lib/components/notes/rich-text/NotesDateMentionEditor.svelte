<script lang="ts">
  import Bell from "@lucide/svelte/icons/bell";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import NotesDateEditorPanel from "$lib/components/notes/database/NotesDateEditorPanel.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { formatNotificationOffset } from "$lib/calendar/event-notifications";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesDatabaseDateValue } from "$lib/notes/database/date";
  import {
    notesDateMentionEditorValue,
    notesDateMentionFormat,
    notesDateMentionReminderMinutes,
    notesDateMentionReminderOffsets,
    notesDateMentionText,
    notesDateMentionValueWithDates,
    notesDateMentionWithReminderMinutes,
    notesDateMentionWithValue,
    type NotesDateMentionTextOptions,
  } from "$lib/notes/rich-text/date-mention-edit";
  import { NOTES_DATE_MENTION_FORMATS, type NotesDateMentionFormat, type NotesDateMentionRichText, type NotesDateMentionValue } from "$lib/notes/types";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";

  /**
   * Edits a date mention in place: its dates and time, an end date, how its date is spelled, and its reminder.
   * Every change reports the whole mention with its chip text spelled again; removing reports null.
   */
  let { anchor, mention, onChange, onClose }: {
    anchor: HTMLElement | undefined;
    mention: NotesDateMentionRichText;
    onChange: (next: NotesDateMentionRichText | null) => void;
    onClose: (refocus: boolean) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const editorId = $props.id();
  let formatsOpen = $state(false);

  const value = $derived(mention.mention.date);
  const format = $derived(notesDateMentionFormat(value));
  const reminder = $derived(value.ganbaru_reminder?.enabled === true);
  const timed = $derived(value.start.includes("T"));
  const reminderOffsets = $derived(notesDateMentionReminderOffsets(value));
  const reminderMinutes = $derived(notesDateMentionReminderMinutes(value));
  const textOptions = $derived<NotesDateMentionTextOptions>({
    locale: localization.locale,
    timeFormat: preferences.calendarTimeFormat,
    labels: {
      remindTitle: (date) => t("notes.remindOnDate", date),
      range: (start, end) => t("notes.dateMentionRange", start, end),
    },
  });
  const formatLabels = $derived<Record<NotesDateMentionFormat, string>>({
    full: t("notes.dateMentionFormatFull"),
    short: t("notes.dateMentionFormatShort"),
    numeric: t("notes.dateMentionFormatNumeric"),
    iso: t("notes.dateMentionFormatIso"),
  });

  function change(next: NotesDateMentionValue): void {
    onChange(notesDateMentionWithValue(mention, next, textOptions));
  }

  function commitDates(dates: NotesDatabaseDateValue | null): void {
    if (dates === null) onChange(null);
    else change(notesDateMentionValueWithDates(value, dates));
  }

  /** A boundary alone, spelled in the mention's format without its reminder wording. */
  function boundaryText(boundary: string): string {
    return spelled({ start: boundary, ganbaru_format: format });
  }

  /** The dates of the mention spelled in a format, for previews beside each format choice. */
  function spelled(next: NotesDateMentionValue): string {
    try {
      return notesDateMentionText({ ...next, ganbaru_reminder: null }, textOptions);
    } catch {
      return next.start;
    }
  }

  function pickFormat(next: NotesDateMentionFormat): void {
    formatsOpen = false;
    if (next !== format) change({ ...value, ganbaru_format: next });
  }

  /** A reminder offset in words; reminding at the date itself reads differently for timed and all-day mentions. */
  function reminderOffsetLabel(minutes: number): string {
    if (minutes > 0) return formatNotificationOffset(minutes, t);
    return timed ? t("notes.dateMentionRemindAtTime") : t("notes.dateMentionRemindOnDay");
  }

  function pickReminderMinutes(minutes: number): void {
    if (minutes !== reminderMinutes) change(notesDateMentionWithReminderMinutes(value, minutes));
  }
</script>

<NotesDateEditorPanel {anchor} id={editorId} label={t("notes.dateMentionEditor")} value={notesDateMentionEditorValue(value)}
  formatBoundary={boundaryText} showTimeZone={false} clearLabel={t("notes.dateMentionRemove")} onCommit={commitDates} {onClose}>
  {#snippet extras()}
    <button type="button" class="menu-item justify-between" aria-expanded={formatsOpen} aria-controls={`${editorId}-formats`}
      onclick={() => { formatsOpen = !formatsOpen; }}>
      <span class="min-w-0 truncate">{t("notes.dateMentionFormat")}</span>
      <span class="flex min-w-0 items-center gap-1 text-muted-foreground">
        <span class="truncate">{formatLabels[format]}</span>
        <ChevronDown size={13} strokeWidth={1.75} class={cn("shrink-0 transition-transform", formatsOpen && "rotate-180")} />
      </span>
    </button>
    {#if formatsOpen}
      <div id={`${editorId}-formats`} class="grid gap-0.5 pl-2" role="radiogroup" aria-label={t("notes.dateMentionFormat")}>
        {#each NOTES_DATE_MENTION_FORMATS as option (option)}
          {@const preview = spelled({ ...value, ganbaru_format: option })}
          <button type="button" class="menu-item" role="radio" aria-checked={option === format} title={preview}
            onclick={() => pickFormat(option)}>
            <span class="min-w-0 flex-1 truncate text-left">{formatLabels[option]}</span>
            <span class="min-w-0 truncate text-muted-foreground">{preview}</span>
            <Check size={13} strokeWidth={1.75} class={cn("shrink-0", option !== format && "invisible")} />
          </button>
        {/each}
      </div>
    {/if}
    <label class="menu-item cursor-pointer justify-between">
      <span class="flex min-w-0 items-center gap-2">
        <Bell size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
        <span class="truncate">{t("notes.dateMentionRemind")}</span>
      </span>
      <Switch size="compact" checked={reminder} ariaLabel={t("notes.dateMentionRemind")}
        onChange={(enabled) => change({ ...value, ganbaru_reminder: enabled ? { enabled: true } : null })} />
    </label>
    {#if reminder}
      <div class="grid gap-0.5 pl-2" role="radiogroup" aria-label={t("notes.dateMentionRemindWhen")}>
        {#each reminderOffsets as minutes (minutes)}
          <button type="button" class="menu-item" role="radio" aria-checked={minutes === reminderMinutes}
            onclick={() => pickReminderMinutes(minutes)}>
            <span class="min-w-0 flex-1 truncate text-left">{reminderOffsetLabel(minutes)}</span>
            <Check size={13} strokeWidth={1.75} class={cn("shrink-0", minutes !== reminderMinutes && "invisible")} />
          </button>
        {/each}
      </div>
    {/if}
  {/snippet}
</NotesDateEditorPanel>
