<script lang="ts">
  import Clock from "@lucide/svelte/icons/clock";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { tick, untrack, type Snippet } from "svelte";
  import MiniDatePicker from "$lib/components/ui/MiniDatePicker.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import TimePicker from "$lib/components/ui/TimePicker.svelte";
  import { formatTimeLabel } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseDateBoundaryParts,
    notesDatabaseDateHasTime,
    notesDatabaseDateWithDay,
    notesDatabaseDateWithEnd,
    notesDatabaseDateWithoutTime,
    notesDatabaseDateWithTime,
    notesDatabaseDateWithTimeZone,
    notesDatabaseIsTimeZoneValid,
    type NotesDatabaseDateBoundaryKey,
    type NotesDatabaseDateValue,
  } from "$lib/notes/database/date";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import { anchoredPopover } from "$lib/utils/anchored-popover";

  /**
   * The floating date editor shared by database date values and date mentions: a calendar, a time, an optional end
   * date and time zone, and clearing. Every pick reports the whole next value at once; the owner saves it and passes
   * the value back.
   */
  let {
    anchor,
    id,
    label,
    value,
    formatBoundary,
    error = null,
    showTimeZone = true,
    clearLabel,
    extras,
    onCommit,
    onClose,
  }: {
    /** The control the editor opens from; the editor portals into its floating root. */
    anchor: HTMLElement | undefined;
    id: string;
    label: string;
    value: NotesDatabaseDateValue | null;
    /** The text of one boundary, spelled like the value it belongs to. */
    formatBoundary: (boundary: string) => string;
    error?: string | null;
    showTimeZone?: boolean;
    clearLabel?: string;
    /** Rows shown below the end date switch, such as a format or reminder choice. */
    extras?: Snippet;
    onCommit: (next: NotesDatabaseDateValue | null) => void;
    /** Close the editor; `refocus` is true when focus should return to the anchor's editing surface. */
    onClose: (refocus: boolean) => void;
  } = $props();

  /** The slot a time picker centers on before a boundary has a time. */
  const DEFAULT_TIME_ANCHOR = "12:00";

  const { t } = getLocalization();
  const preferences = getPreferences();
  let panel: HTMLDivElement | undefined = $state();
  let timeTrigger: HTMLButtonElement | undefined = $state();
  let timePanel: HTMLDivElement | undefined = $state();
  let timeOpen = $state(false);
  let active = $state<NotesDatabaseDateBoundaryKey>("start");
  let zone = $state("");
  let zoneInvalid = $state(false);

  const range = $derived(value?.end != null);
  const activeKey = $derived<NotesDatabaseDateBoundaryKey>(range ? active : "start");
  const activeBoundary = $derived(value ? value[activeKey] ?? value.start : null);
  const activeParts = $derived(activeBoundary ? notesDatabaseDateBoundaryParts(activeBoundary) : null);
  const timed = $derived(notesDatabaseDateHasTime(value));
  const floatingRoot = $derived(anchor?.closest<HTMLElement>("[data-floating-root]") ?? "body");
  const boundaries = $derived<{ key: NotesDatabaseDateBoundaryKey; label: string }[]>([
    { key: "start", label: t("notes.databaseDateStart") },
    { key: "end", label: t("notes.databaseDateEnd") },
  ]);

  /** Show the saved zone whenever the value's zone changes, including a restore after a failed save. */
  $effect(() => {
    const saved = value?.time_zone ?? "";
    untrack(() => {
      zone = saved;
      zoneInvalid = false;
    });
  });

  /** Move focus into the calendar on the day being edited once the editor opens. */
  $effect(() => {
    if (!panel) return;
    const surface = panel;
    const day = untrack(() => activeParts?.date);
    void tick().then(() => {
      (surface.querySelector<HTMLButtonElement>(day ? `[data-date="${day}"]` : "[data-date][tabindex='0']")
        ?? surface.querySelector<HTMLButtonElement>("[data-date]"))?.focus({ preventScroll: true });
    });
  });

  function pickDay(day: string): void {
    onCommit(notesDatabaseDateWithDay(value, activeKey, day));
    if (range && activeKey === "start") active = "end";
  }

  function closeTime(): void {
    timeOpen = false;
    timeTrigger?.focus({ preventScroll: true });
  }

  function pickTime(time: string): void {
    if (!value) return;
    onCommit(notesDatabaseDateWithTime(value, activeKey, time));
    closeTime();
  }

  function removeTime(): void {
    if (!value) return;
    onCommit(notesDatabaseDateWithoutTime(value));
    timeOpen = false;
  }

  function toggleEnd(enabled: boolean): void {
    if (!value) return;
    onCommit(notesDatabaseDateWithEnd(value, enabled));
    active = enabled ? "end" : "start";
  }

  /** Save a typed time zone once it names a real zone, or clear it when blank. */
  function saveZone(): void {
    if (!value) return;
    const typed = zone.trim();
    zoneInvalid = typed !== "" && !notesDatabaseIsTimeZoneValid(typed);
    if (zoneInvalid || typed === (value.time_zone ?? "")) return;
    onCommit(notesDatabaseDateWithTimeZone(value, typed));
  }

  function clear(): void {
    onCommit(null);
    onClose(true);
  }
</script>

<div bind:this={panel} use:portal={floatingRoot}
  use:anchoredPopover={{ anchor, inside: [timePanel], onOutside: () => onClose(false) }}
  {id} role="dialog" aria-label={label} tabindex="-1" data-app-floating-surface data-app-shortcuts="ignore"
  class="surface-floating fixed z-90 w-floating-sm overflow-auto p-2"
  onkeydowncapture={(event) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onClose(true);
  }}>
  {#if value && range}
    <div class="mb-2 grid grid-cols-2 gap-1" role="group" aria-label={label}>
      {#each boundaries as { key, label: boundaryLabel } (key)}
        {@const boundary = value[key]}
        <button type="button" class={cn("field flex min-w-0 flex-col items-start text-left", activeKey === key && "border-primary")}
          aria-pressed={activeKey === key} onclick={() => { active = key; }}>
          <span class="text-panel-detail text-muted-foreground">{boundaryLabel}</span>
          <span class="w-full truncate">{boundary ? formatBoundary(boundary) : ""}</span>
        </button>
      {/each}
    </div>
  {/if}
  <MiniDatePicker selectedDate={activeParts?.date ?? ""} rangeStartDate={range ? notesDatabaseDateBoundaryParts(value?.start ?? "")?.date : undefined}
    rangeEndDate={range ? notesDatabaseDateBoundaryParts(value?.end ?? "")?.date : undefined}
    small highlightToday={false} activeHighlight="primary" onSelect={pickDay} onCancel={() => onClose(true)} />
  {#if value}
    <div class="mt-2 grid gap-0.5 border-t border-border/70 pt-2">
      <div class="flex items-center gap-1">
        <button bind:this={timeTrigger} type="button" class="menu-item min-w-0 flex-1 text-muted-foreground hover:text-foreground"
          aria-haspopup="dialog" aria-expanded={timeOpen} onclick={() => { timeOpen = !timeOpen; }}>
          <Clock size={13} strokeWidth={1.75} class="shrink-0" />
          <span class="truncate">{activeParts?.time ? formatTimeLabel(activeParts.time, preferences.calendarTimeFormat) : t("notes.databaseDateAddTime")}</span>
        </button>
        {#if timed}
          <button type="button" class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
            aria-label={t("notes.databaseDateRemoveTime")} data-app-tooltip-disabled="true" onclick={removeTime}>
            <Eraser size={13} strokeWidth={1.75} />
          </button>
        {/if}
        <button type="button" class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
          aria-label={clearLabel ?? t("notes.databaseDateClear")} data-app-tooltip-disabled="true" onclick={clear}>
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
      <label class="menu-item cursor-pointer justify-between">
        <span class="min-w-0 truncate">{t("notes.databaseDateEndDate")}</span>
        <Switch size="compact" checked={range} ariaLabel={t("notes.databaseDateEndDate")} onChange={toggleEnd} />
      </label>
      {@render extras?.()}
      {#if timed && showTimeZone}
        <label class="grid gap-1 px-2 pt-1">
          <span class="text-panel-detail text-muted-foreground">{t("notes.databaseDateTimeZone")}</span>
          <input class="field min-w-0" bind:value={zone} placeholder={t("notes.databaseDateZoneHint")} aria-invalid={zoneInvalid}
            maxlength={100} spellcheck="false" autocomplete="off" onchange={saveZone}
            onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); saveZone(); } }} />
        </label>
        {#if zoneInvalid}<p class="px-2 text-panel-detail text-destructive" role="alert">{t("notes.databaseDateInvalidZone")}</p>{/if}
      {/if}
    </div>
  {/if}
  {#if error}<Notice class="px-2 pt-2 text-panel-detail" message={error} />{/if}
</div>

{#if timeOpen && value}
  <div bind:this={timePanel} use:portal={floatingRoot}
    role="dialog" aria-label={t("notes.databaseDateAddTime")} data-app-floating-surface data-app-shortcuts="ignore"
    use:anchoredPopover={{ anchor: timeTrigger, onOutside: () => { timeOpen = false; } }}
    class="surface-floating fixed z-90 w-40 overflow-hidden"
    onkeydowncapture={(event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeTime();
    }}>
    <TimePicker currentTime={activeParts?.time ?? ""} activeTime={activeParts?.time ?? DEFAULT_TIME_ANCHOR}
      scrollTime={activeParts?.time ?? DEFAULT_TIME_ANCHOR} emphasizedTime={activeParts?.time ? undefined : null} focusOnOpen
      onSelect={pickTime} onCancel={closeTime} />
  </div>
{/if}
