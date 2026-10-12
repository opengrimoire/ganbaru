<script lang="ts" generics="T extends NotesDateMentionValue">
  import DateReminderOptions from "$lib/components/ui/DateReminderOptions.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDateMentionReminderMinutes,
    notesDateMentionWithReminderMinutes,
  } from "$lib/notes/rich-text/date-mention-edit";
  import type { NotesDateMentionValue } from "$lib/notes/types";

  /** The reminder rows of a Notes date editor, shared by date mentions and database dates. */
  let { value, onChange }: {
    value: T;
    onChange: (next: T) => void;
  } = $props();

  const { t } = getLocalization();
  const minutes = $derived(value.ganbaru_reminder?.enabled === true ? notesDateMentionReminderMinutes(value) : null);

  function change(next: number | null): void {
    if (next === null) {
      onChange({ ...value, ganbaru_reminder: null });
      return;
    }
    onChange(notesDateMentionWithReminderMinutes({ ...value, ganbaru_reminder: { enabled: true } }, next));
  }
</script>

<DateReminderOptions {minutes} timed={value.start.includes("T")} onChange={change} labels={{
  remind: t("notes.dateMentionRemind"),
  remindWhen: t("notes.dateMentionRemindWhen"),
  remindOnDay: t("notes.dateMentionRemindOnDay"),
  remindAtTime: t("notes.dateMentionRemindAtTime"),
}} />
