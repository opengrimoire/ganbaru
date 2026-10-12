<script lang="ts" generics="T extends NotesDateMentionValue">
  import Bell from "@lucide/svelte/icons/bell";
  import Check from "@lucide/svelte/icons/check";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { formatNotificationOffset } from "$lib/calendar/event-notifications";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDateMentionReminderMinutes,
    notesDateMentionReminderOffsets,
    notesDateMentionWithReminderMinutes,
  } from "$lib/notes/rich-text/date-mention-edit";
  import type { NotesDateMentionValue } from "$lib/notes/types";
  import { cn } from "$lib/utils";

  /**
   * The reminder rows of a date editor: a switch, and while it is on, how long before the date it fires. Date mentions
   * and database dates share it, so both offer the same offsets.
   */
  let { value, onChange }: {
    value: T;
    onChange: (next: T) => void;
  } = $props();

  const { t } = getLocalization();
  const enabled = $derived(value.ganbaru_reminder?.enabled === true);
  const timed = $derived(value.start.includes("T"));
  const offsets = $derived(notesDateMentionReminderOffsets(value));
  const minutes = $derived(notesDateMentionReminderMinutes(value));

  /** A reminder offset in words; reminding at the date itself reads differently for timed and all-day dates. */
  function offsetLabel(offset: number): string {
    if (offset > 0) return formatNotificationOffset(offset, t);
    return timed ? t("notes.dateMentionRemindAtTime") : t("notes.dateMentionRemindOnDay");
  }

  function pick(offset: number): void {
    if (offset !== minutes) onChange(notesDateMentionWithReminderMinutes(value, offset));
  }
</script>

<label class="menu-item cursor-pointer justify-between">
  <span class="flex min-w-0 items-center gap-2">
    <Bell size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
    <span class="truncate">{t("notes.dateMentionRemind")}</span>
  </span>
  <Switch size="compact" checked={enabled} ariaLabel={t("notes.dateMentionRemind")}
    onChange={(next) => onChange({ ...value, ganbaru_reminder: next ? { enabled: true } : null })} />
</label>
{#if enabled}
  <div class="grid gap-0.5 pl-2" role="radiogroup" aria-label={t("notes.dateMentionRemindWhen")}>
    {#each offsets as offset (offset)}
      <button type="button" class="menu-item" role="radio" aria-checked={offset === minutes} onclick={() => pick(offset)}>
        <span class="min-w-0 flex-1 truncate text-left">{offsetLabel(offset)}</span>
        <Check size={13} strokeWidth={1.75} class={cn("shrink-0", offset !== minutes && "invisible")} />
      </button>
    {/each}
  </div>
{/if}
