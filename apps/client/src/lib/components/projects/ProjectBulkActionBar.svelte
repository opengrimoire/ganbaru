<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Check from "@lucide/svelte/icons/check";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import X from "@lucide/svelte/icons/x";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import CollectionSelectionAction from "$lib/components/collections/CollectionSelectionAction.svelte";
  import CollectionSelectionBar from "$lib/components/collections/CollectionSelectionBar.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectPriority, ProjectPriorityConfig, ProjectStatus } from "$lib/projects/types";
  import { getTheme } from "$lib/stores/theme.svelte";
  import PriorityFlagIcon from "./PriorityFlagIcon.svelte";
  import ProjectStatusBadge from "./ProjectStatusBadge.svelte";

  /**
   * The floating toolbar for selected Project tasks: one segment per bulk property, the archive or restore action, and a
   * menu of selection actions. A property segment marks the value every selected task shares.
   */
  let {
    selectedTaskCount,
    selectableTaskCount,
    selectedActiveTaskCount,
    selectedArchivedTaskCount,
    bulkSchedulableCount,
    bulkTaskActionPending,
    bulkScheduleDate,
    bulkScheduleStartTime,
    bulkScheduleDurationMinutes,
    bulkTaskError,
    statuses,
    priorities,
    sharedStatusId,
    sharedPriority,
    onSelectFiltered,
    onSetStatus,
    onScheduleOpenChange,
    onSetPriority,
    onArchive,
    onRestore,
    onClear,
    onBulkScheduleDateChange,
    onBulkScheduleStartTimeChange,
    onBulkScheduleDurationMinutesChange,
    onBulkScheduleSubmit,
  }: {
    selectedTaskCount: number;
    selectableTaskCount: number;
    selectedActiveTaskCount: number;
    selectedArchivedTaskCount: number;
    bulkSchedulableCount: number;
    bulkTaskActionPending: boolean;
    bulkScheduleDate: string;
    bulkScheduleStartTime: string;
    bulkScheduleDurationMinutes: number;
    bulkTaskError: string | null;
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    /** The status every selected task shares, or null when they differ. */
    sharedStatusId: string | null;
    /** The priority every selected task shares, or null when they differ. */
    sharedPriority: ProjectPriority | null;
    onSelectFiltered: () => void;
    onSetStatus: (status: ProjectStatus) => void;
    /** Called when the schedule form opens or closes, so the owner can seed its draft. */
    onScheduleOpenChange: (open: boolean) => void;
    onSetPriority: (priority: ProjectPriority) => void;
    onArchive: () => void;
    onRestore: () => void;
    onClear: () => void;
    onBulkScheduleDateChange: (value: string) => void;
    onBulkScheduleStartTimeChange: (value: string) => void;
    onBulkScheduleDurationMinutesChange: (value: number) => void;
    onBulkScheduleSubmit: () => void;
  } = $props();

  const { t } = getLocalization();
  const theme = getTheme();
  const segmentClass = "h-8 rounded-none px-2.5 hover:bg-accent/60";
</script>

{#snippet statusIcon()}<CollectionPropertyIcon kind="status" class="size-3.5 shrink-0 text-muted-foreground" />{/snippet}
{#snippet priorityIcon()}<CollectionPropertyIcon kind="select" class="size-3.5 shrink-0 text-muted-foreground" />{/snippet}
{#snippet scheduleIcon()}<CalendarDays class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />{/snippet}

<CollectionSelectionBar count={selectedTaskCount} {onClear}>
  {#if statuses.length > 0}
    <CollectionMenu label={t("projects.list.status")} kind="property" leading={statusIcon} triggerClass={segmentClass}
      disabled={bulkTaskActionPending} dismissOnAction>
      <div class="grid gap-0">
        {#each statuses as status (status.id)}
          <button type="button" class="menu-item justify-between" aria-pressed={sharedStatusId === status.id} onclick={() => onSetStatus(status)}>
            <ProjectStatusBadge {status} theme={theme.current} label={status.name} class="text-[0.733333rem]" />
            {#if sharedStatusId === status.id}<Check class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />{/if}
          </button>
        {/each}
      </div>
    </CollectionMenu>
  {/if}
  {#if priorities.length > 0}
    <CollectionMenu label={t("projects.list.priority")} kind="property" leading={priorityIcon} triggerClass={segmentClass}
      disabled={bulkTaskActionPending} dismissOnAction>
      <div class="grid gap-0">
        {#each priorities as priority (priority.id)}
          <button type="button" class="menu-item justify-between" aria-pressed={sharedPriority === priority.id} onclick={() => onSetPriority(priority.id)}>
            <span class="flex min-w-0 items-center gap-1.5">
              <PriorityFlagIcon color={priority.color} theme={theme.current} size={12} class="shrink-0" />
              <span class="truncate">{priority.name}</span>
            </span>
            {#if sharedPriority === priority.id}<Check class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />{/if}
          </button>
        {/each}
      </div>
    </CollectionMenu>
  {/if}
  <CollectionMenu label={t("projects.bulk.schedule")} kind="property" leading={scheduleIcon} triggerClass={segmentClass}
    disabled={bulkTaskActionPending || bulkSchedulableCount === 0} onOpenChange={onScheduleOpenChange}>
    <form class="grid gap-2 p-1" onsubmit={(event) => { event.preventDefault(); onBulkScheduleSubmit(); }}>
      <p class="text-muted-foreground">{t("projects.bulk.scheduleHint", bulkSchedulableCount)}</p>
      <label class="grid gap-1 text-muted-foreground">
        <span>{t("projects.schedule.date")}</span>
        <input value={bulkScheduleDate} placeholder="YYYY-MM-DD" class="field min-w-0 text-foreground"
          oninput={(event) => onBulkScheduleDateChange(event.currentTarget.value)} />
      </label>
      <div class="grid grid-cols-2 gap-2">
        <label class="grid gap-1 text-muted-foreground">
          <span>{t("projects.schedule.start")}</span>
          <input value={bulkScheduleStartTime} placeholder="HH:MM" class="field min-w-0 text-foreground"
            oninput={(event) => onBulkScheduleStartTimeChange(event.currentTarget.value)} />
        </label>
        <label class="grid gap-1 text-muted-foreground">
          <span>{t("projects.schedule.duration")}</span>
          <input type="number" min="1" step="5" value={bulkScheduleDurationMinutes} class="field min-w-0 text-foreground"
            oninput={(event) => onBulkScheduleDurationMinutesChange(event.currentTarget.valueAsNumber)} />
        </label>
      </div>
      {#if bulkTaskError}
        <Notice class="px-1 py-0.5" message={bulkTaskError} />
      {/if}
      <button type="submit" data-collection-menu-keep-open
        class="rounded-md bg-primary px-2 py-1.5 font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-60"
        disabled={bulkTaskActionPending || bulkSchedulableCount === 0}>
        {t("projects.bulk.scheduleSelected")}
      </button>
    </form>
  </CollectionMenu>
  {#if selectedActiveTaskCount > 0}
    <CollectionSelectionAction icon={Archive} iconOnly label={t("projects.bulk.archive")} disabled={bulkTaskActionPending} onclick={onArchive} />
  {/if}
  {#if selectedArchivedTaskCount > 0}
    <CollectionSelectionAction icon={ArchiveRestore} iconOnly label={t("projects.bulk.restore")} disabled={bulkTaskActionPending} onclick={onRestore} />
  {/if}
  <CollectionMenu iconOnly kind="actions" label={t("collections.selection.more")} triggerClass={segmentClass} disabled={bulkTaskActionPending} dismissOnAction>
    <div class="grid gap-0">
      {#if selectableTaskCount > selectedTaskCount}
        <CollectionMenuItem icon={ListChecks} label={t("projects.bulk.selectFiltered", selectableTaskCount)} onclick={onSelectFiltered} />
      {/if}
      <CollectionMenuItem icon={X} label={t("collections.selection.clear")} onclick={onClear} />
    </div>
  </CollectionMenu>
</CollectionSelectionBar>
