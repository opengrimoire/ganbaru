<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Check from "@lucide/svelte/icons/check";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import CollectionSelectionAction from "$lib/components/collections/CollectionSelectionAction.svelte";
  import CollectionSelectionBar from "$lib/components/collections/CollectionSelectionBar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectPriority, ProjectPriorityConfig } from "$lib/projects/types";
  import { getTheme } from "$lib/stores/theme.svelte";
  import PriorityFlagIcon from "./PriorityFlagIcon.svelte";

  let {
    selectedTaskCount,
    selectableTaskCount,
    selectedActiveTaskCount,
    selectedArchivedTaskCount,
    bulkSchedulableCount,
    bulkTaskActionPending,
    bulkScheduleOpen,
    bulkScheduleDate,
    bulkScheduleStartTime,
    bulkScheduleDurationMinutes,
    bulkTaskError,
    priorities,
    canMarkDone,
    canReopen,
    onSelectFiltered,
    onMarkDone,
    onReopen,
    onToggleBulkSchedule,
    onSetPriority,
    onArchive,
    onRestore,
    onClear,
    onBulkScheduleDateChange,
    onBulkScheduleStartTimeChange,
    onBulkScheduleDurationMinutesChange,
    onBulkScheduleSubmit,
    onCloseBulkSchedule,
  }: {
    selectedTaskCount: number;
    selectableTaskCount: number;
    selectedActiveTaskCount: number;
    selectedArchivedTaskCount: number;
    bulkSchedulableCount: number;
    bulkTaskActionPending: boolean;
    bulkScheduleOpen: boolean;
    bulkScheduleDate: string;
    bulkScheduleStartTime: string;
    bulkScheduleDurationMinutes: number;
    bulkTaskError: string | null;
    priorities: ProjectPriorityConfig[];
    canMarkDone: boolean;
    canReopen: boolean;
    onSelectFiltered: () => void;
    onMarkDone: () => void;
    onReopen: () => void;
    onToggleBulkSchedule: () => void;
    onSetPriority: (priority: ProjectPriority) => void;
    onArchive: () => void;
    onRestore: () => void;
    onClear: () => void;
    onBulkScheduleDateChange: (value: string) => void;
    onBulkScheduleStartTimeChange: (value: string) => void;
    onBulkScheduleDurationMinutesChange: (value: number) => void;
    onBulkScheduleSubmit: () => void;
    onCloseBulkSchedule: () => void;
  } = $props();

  const { t } = getLocalization();
  const theme = getTheme();
</script>

<CollectionSelectionBar count={selectedTaskCount} {onClear}>
  <CollectionSelectionAction icon={Check} label={t("projects.bulk.selectFiltered", selectableTaskCount)}
    disabled={bulkTaskActionPending || selectableTaskCount === selectedTaskCount} onclick={onSelectFiltered} />
  <CollectionSelectionAction icon={Check} label={t("projects.bulk.markDone")}
    disabled={bulkTaskActionPending || !canMarkDone} onclick={onMarkDone} />
  <CollectionSelectionAction icon={RotateCcw} label={t("projects.bulk.reopen")}
    disabled={bulkTaskActionPending || !canReopen} onclick={onReopen} />
  <CollectionSelectionAction icon={CalendarDays} label={t("projects.bulk.schedule")}
    disabled={bulkTaskActionPending || bulkSchedulableCount === 0} onclick={onToggleBulkSchedule} />
  <div class="flex max-w-full items-center gap-1 overflow-x-auto rounded-md bg-muted/60 p-0.5">
    {#each priorities as priority (priority.id)}
      <button
        type="button"
        class="flex h-6 shrink-0 items-center gap-1.5 rounded px-2 font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
        disabled={bulkTaskActionPending}
        onclick={() => onSetPriority(priority.id)}
      >
        <PriorityFlagIcon color={priority.color} theme={theme.current} size={12} class="shrink-0" />
        <span>{priority.name}</span>
      </button>
    {/each}
  </div>
  {#if selectedActiveTaskCount > 0}
    <CollectionSelectionAction icon={Archive} label={t("projects.bulk.archive")} destructive
      disabled={bulkTaskActionPending} onclick={onArchive} />
  {/if}
  {#if selectedArchivedTaskCount > 0}
    <CollectionSelectionAction icon={ArchiveRestore} label={t("projects.bulk.restore")}
      disabled={bulkTaskActionPending} onclick={onRestore} />
  {/if}
  {#snippet footer()}
    {#if bulkScheduleOpen}
      <form
        class="basis-full grid gap-2 border-t border-border/70 pt-2 min-[860px]:grid-cols-[minmax(0,1fr)_8rem_7rem_6rem_auto_auto]"
        onsubmit={(event) => {
          event.preventDefault();
          onBulkScheduleSubmit();
        }}
      >
        <div class="self-end text-[0.733333rem] text-muted-foreground">
          {t("projects.bulk.scheduleHint", bulkSchedulableCount)}
        </div>
        <label class="grid gap-1 text-[0.733333rem] text-muted-foreground">
          <span>{t("projects.schedule.date")}</span>
          <input
            value={bulkScheduleDate}
            placeholder="YYYY-MM-DD"
            class="field min-w-0 text-[0.8rem] text-foreground"
            oninput={(event) => onBulkScheduleDateChange(event.currentTarget.value)}
          />
        </label>
        <label class="grid gap-1 text-[0.733333rem] text-muted-foreground">
          <span>{t("projects.schedule.start")}</span>
          <input
            value={bulkScheduleStartTime}
            placeholder="HH:MM"
            class="field min-w-0 text-[0.8rem] text-foreground"
            oninput={(event) => onBulkScheduleStartTimeChange(event.currentTarget.value)}
          />
        </label>
        <label class="grid gap-1 text-[0.733333rem] text-muted-foreground">
          <span>{t("projects.schedule.duration")}</span>
          <input
            type="number"
            min="1"
            step="5"
            value={bulkScheduleDurationMinutes}
            class="field min-w-0 text-[0.8rem] text-foreground"
            oninput={(event) => onBulkScheduleDurationMinutesChange(event.currentTarget.valueAsNumber)}
          />
        </label>
        <button
          type="submit"
          class="self-end rounded-md bg-primary px-2 py-1.5 text-[0.8rem] font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-60"
          disabled={bulkTaskActionPending || bulkSchedulableCount === 0}
        >
          {t("projects.bulk.scheduleSelected")}
        </button>
        <button
          type="button"
          class="self-end rounded-md border border-border bg-card px-2 py-1.5 text-[0.8rem] hover:bg-accent"
          disabled={bulkTaskActionPending}
          onclick={onCloseBulkSchedule}
        >
          {t("common.cancel")}
        </button>
      </form>
    {/if}
    {#if bulkTaskError}
      <div class="basis-full rounded border border-destructive/30 bg-destructive/10 px-2 py-1 text-destructive">
        {bulkTaskError}
      </div>
    {/if}
  {/snippet}
</CollectionSelectionBar>
