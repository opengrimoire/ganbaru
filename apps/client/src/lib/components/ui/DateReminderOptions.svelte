<script lang="ts" module>
  /** The wording of the reminder rows, which each feature owns in its catalog. */
  export interface DateReminderOptionsLabels {
    remind: string;
    remindWhen: string;
    remindOnDay: string;
    remindAtTime: string;
  }
</script>

<script lang="ts">
  import Bell from "@lucide/svelte/icons/bell";
  import Check from "@lucide/svelte/icons/check";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { formatNotificationOffset } from "$lib/calendar/event-notifications";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { dateReminderOffsets } from "$lib/utils/date-reminders";
  import { cn } from "$lib/utils";

  /**
   * The reminder rows of a date editor: a switch, and while it is on, how long before the date it fires. Notes dates
   * and Projects task dates share it, so both offer the same offsets.
   */
  let { minutes, timed, labels, onChange }: {
    /** Minutes before the date the reminder fires, or null for no reminder. */
    minutes: number | null;
    /** Whether the date has a time, which decides the offsets offered. */
    timed: boolean;
    labels: DateReminderOptionsLabels;
    onChange: (minutes: number | null) => void;
  } = $props();

  const { t } = getLocalization();
  const offsets = $derived(dateReminderOffsets(timed));

  /** A reminder offset in words; reminding at the date itself reads differently for timed and all-day dates. */
  function offsetLabel(offset: number): string {
    if (offset > 0) return formatNotificationOffset(offset, t);
    return timed ? labels.remindAtTime : labels.remindOnDay;
  }
</script>

<label class="menu-item cursor-pointer justify-between">
  <span class="flex min-w-0 items-center gap-2">
    <Bell size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
    <span class="truncate">{labels.remind}</span>
  </span>
  <Switch size="compact" checked={minutes !== null} ariaLabel={labels.remind}
    onChange={(next) => onChange(next ? 0 : null)} />
</label>
{#if minutes !== null}
  <div class="grid gap-0.5 pl-2" role="radiogroup" aria-label={labels.remindWhen}>
    {#each offsets as offset (offset)}
      <button type="button" class="menu-item" role="radio" aria-checked={offset === minutes}
        onclick={() => { if (offset !== minutes) onChange(offset); }}>
        <span class="min-w-0 flex-1 truncate text-left">{offsetLabel(offset)}</span>
        <Check size={13} strokeWidth={1.75} class={cn("shrink-0", offset !== minutes && "invisible")} />
      </button>
    {/each}
  </div>
{/if}
