<script lang="ts">
  import Clock from "@lucide/svelte/icons/clock";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { tick, untrack } from "svelte";
  import MiniDatePicker from "$lib/components/ui/MiniDatePicker.svelte";
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
  import { notesDatabaseDateDisplay } from "$lib/notes/database/property-display";
  import { notesDatabaseTableDateValue, type NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import { anchoredPopover } from "$lib/utils/anchored-popover";

  /**
   * Edits a date property with the calendar, time, and range controls Projects uses for task dates.
   *
   * Every pick saves at once. A range picks its start and then its end, and a failed save restores the saved value.
   */
  let { row, column, rowIndex, columnIndex, mutating, onSave, onNavigate, onEditingChange = () => {}, variant = "cell" }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    mutating: boolean;
    onSave: (value: NotesDatabaseDateValue | null) => Promise<boolean>;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
    onEditingChange?: (editing: boolean) => void;
    /** Cells fill a table column; page values align their text with the page and point like links. */
    variant?: "cell" | "page";
  } = $props();

  /** The slot a time picker centers on before a boundary has a time. */
  const DEFAULT_TIME_ANCHOR = "12:00";

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const pickerId = $props.id();
  const saved = $derived(notesDatabaseTableDateValue(row, column));
  let trigger: HTMLButtonElement | undefined = $state();
  let panel: HTMLDivElement | undefined = $state();
  let timeTrigger: HTMLButtonElement | undefined = $state();
  let timePanel: HTMLDivElement | undefined = $state();
  let open = $state(false);
  let timeOpen = $state(false);
  let active = $state<NotesDatabaseDateBoundaryKey>("start");
  let draft = $state.raw<NotesDatabaseDateValue | null>(null);
  let zone = $state("");
  let zoneInvalid = $state(false);
  let error = $state<string | null>(null);
  let pendingSaves = $state(0);
  let latestSave = 0;
  let saveChain: Promise<void> = Promise.resolve();

  /** The value on screen: the draft while the editor is open or saving, otherwise the saved value. */
  const shown = $derived(open || pendingSaves > 0 ? draft : saved);
  const text = $derived(notesDatabaseDateDisplay(shown, localization, column.displayFormat));
  const range = $derived(shown?.end != null);
  const activeKey = $derived<NotesDatabaseDateBoundaryKey>(range ? active : "start");
  const activeBoundary = $derived(shown ? shown[activeKey] ?? shown.start : null);
  const activeParts = $derived(activeBoundary ? notesDatabaseDateBoundaryParts(activeBoundary) : null);
  const timed = $derived(notesDatabaseDateHasTime(shown));
  const boundaries = $derived<{ key: NotesDatabaseDateBoundaryKey; label: string }[]>([
    { key: "start", label: t("notes.databaseDateStart") },
    { key: "end", label: t("notes.databaseDateEnd") },
  ]);

  /** Move focus into the calendar on the day being edited once the editor opens. */
  $effect(() => {
    if (!open || !panel) return;
    const surface = panel;
    const day = untrack(() => activeParts?.date);
    void tick().then(() => {
      if (!open) return;
      (surface.querySelector<HTMLButtonElement>(day ? `[data-date="${day}"]` : "[data-date][tabindex='0']")
        ?? surface.querySelector<HTMLButtonElement>("[data-date]"))?.focus({ preventScroll: true });
    });
  });

  function show(): void {
    if (open || mutating) return;
    if (pendingSaves === 0) draft = saved;
    active = "start";
    zone = draft?.time_zone ?? "";
    zoneInvalid = false;
    error = null;
    open = true;
    onEditingChange(true);
  }

  /** Close the editor, returning focus to the value when the close came from the keyboard. */
  function close(refocus: boolean): void {
    if (!open) return;
    open = false;
    timeOpen = false;
    onEditingChange(false);
    if (refocus) trigger?.focus({ preventScroll: true });
  }

  /**
   * Show a new value and save it after any earlier save of this cell, so saves land in the order they were picked.
   * Only the newest save reports a failure and restores the saved value.
   */
  function commit(next: NotesDatabaseDateValue | null): void {
    draft = next;
    error = null;
    const sequence = ++latestSave;
    pendingSaves += 1;
    saveChain = saveChain.then(async () => {
      let succeeded = false;
      try {
        succeeded = await onSave(next);
      } catch (caught: unknown) {
        console.warn("Save database date failed", caught);
      }
      if (!succeeded && sequence === latestSave) {
        error = t("notes.databaseDateSaveFailed");
        draft = saved;
        zone = saved?.time_zone ?? "";
      }
    }).finally(() => { pendingSaves -= 1; });
  }

  function pickDay(day: string): void {
    commit(notesDatabaseDateWithDay(draft, activeKey, day));
    if (range && activeKey === "start") active = "end";
  }

  function closeTime(): void {
    timeOpen = false;
    timeTrigger?.focus({ preventScroll: true });
  }

  function pickTime(time: string): void {
    if (!draft) return;
    commit(notesDatabaseDateWithTime(draft, activeKey, time));
    closeTime();
  }

  function removeTime(): void {
    if (!draft) return;
    commit(notesDatabaseDateWithoutTime(draft));
    zone = "";
    zoneInvalid = false;
    timeOpen = false;
  }

  function toggleEnd(enabled: boolean): void {
    if (!draft) return;
    commit(notesDatabaseDateWithEnd(draft, enabled));
    active = enabled ? "end" : "start";
  }

  /** Save a typed time zone once it names a real zone, or clear it when blank. */
  function saveZone(): void {
    if (!draft) return;
    const typed = zone.trim();
    zoneInvalid = typed !== "" && !notesDatabaseIsTimeZoneValid(typed);
    if (zoneInvalid || typed === (draft.time_zone ?? "")) return;
    commit(notesDatabaseDateWithTimeZone(draft, typed));
  }

  function clear(): void {
    commit(null);
    close(true);
  }

  /** The text of one boundary, formatted like the value itself. */
  function boundaryText(boundary: string): string {
    return notesDatabaseDateDisplay({ start: boundary, end: null, time_zone: shown?.time_zone ?? null }, localization, column.displayFormat);
  }
</script>

<button bind:this={trigger} data-table-cell="true" data-collection-cell-primary data-row-index={rowIndex} data-column-index={columnIndex}
  type="button" class={cn("flex w-full min-w-0 items-center rounded-sm text-left outline-none disabled:cursor-not-allowed",
    variant === "page" ? "min-h-8 cursor-pointer px-1.5" : "h-8 px-1", text ? "text-foreground" : "text-muted-foreground")}
  disabled={mutating && !open} aria-label={column.name} aria-haspopup="dialog" aria-expanded={open}
  aria-controls={open ? pickerId : undefined} title={text}
  onclick={() => { if (open) close(false); else show(); }}
  onkeydown={(event) => { if (event.key !== "Enter" && event.key !== " ") onNavigate(event, rowIndex, columnIndex); }}>
  <span class="min-w-0 truncate">{text || t("notes.databaseTableEmptyCell")}</span>
</button>

{#if open}
  <div bind:this={panel} use:portal={trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
    use:anchoredPopover={{ anchor: trigger, inside: [timePanel], onOutside: () => close(false) }}
    id={pickerId} role="dialog" aria-label={column.name} tabindex="-1" data-app-floating-surface data-app-shortcuts="ignore"
    class="surface-floating fixed z-90 w-floating-sm overflow-auto p-2"
    onkeydowncapture={(event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }}>
    {#if shown && range}
      <div class="mb-2 grid grid-cols-2 gap-1" role="group" aria-label={column.name}>
        {#each boundaries as { key, label } (key)}
          {@const boundary = shown[key]}
          <button type="button" class={cn("field flex min-w-0 flex-col items-start text-left", activeKey === key && "border-primary")}
            aria-pressed={activeKey === key} onclick={() => { active = key; }}>
            <span class="text-panel-detail text-muted-foreground">{label}</span>
            <span class="w-full truncate">{boundary ? boundaryText(boundary) : ""}</span>
          </button>
        {/each}
      </div>
    {/if}
    <MiniDatePicker selectedDate={activeParts?.date ?? ""} rangeStartDate={range ? notesDatabaseDateBoundaryParts(shown?.start ?? "")?.date : undefined}
      rangeEndDate={range ? notesDatabaseDateBoundaryParts(shown?.end ?? "")?.date : undefined}
      small highlightToday={false} activeHighlight="primary" onSelect={pickDay} onCancel={() => close(true)} />
    {#if shown}
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
            aria-label={t("notes.databaseDateClear")} data-app-tooltip-disabled="true" onclick={clear}>
            <Trash2 size={13} strokeWidth={1.75} />
          </button>
        </div>
        <label class="menu-item cursor-pointer justify-between">
          <span class="min-w-0 truncate">{t("notes.databaseDateEndDate")}</span>
          <Switch size="compact" checked={range} ariaLabel={t("notes.databaseDateEndDate")} onChange={toggleEnd} />
        </label>
        {#if timed}
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
    {#if error}<p class="px-2 pt-2 text-panel-detail text-destructive" role="alert">{error}</p>{/if}
  </div>
{/if}

{#if open && timeOpen && shown}
  <div bind:this={timePanel} use:portal={trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
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
