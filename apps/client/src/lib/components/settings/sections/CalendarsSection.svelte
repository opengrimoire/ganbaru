<script lang="ts">
  import { onMount } from "svelte";
  import Upload from "@lucide/svelte/icons/upload";
  import Download from "@lucide/svelte/icons/download";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { invoke } from "@tauri-apps/api/core";
  import { getCalendars } from "$lib/stores/calendars.svelte";
  import { getCalendar } from "$lib/stores/calendar.svelte";
  import type { Calendar } from "$lib/calendar/types";
  import { calendarDisplayName, calendarImportDate } from "$lib/calendar/display";
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import {
    CALENDAR_ZOOM_PERCENT_LEVELS,
    calendarZoomGridMinutesForPercent,
    getCalendarZoom,
  } from "$lib/stores/calendar-zoom.svelte";
  import { DEFAULT_CALENDAR_TIME_FORMAT, isCalendarTimeFormat } from "$lib/stores/preference-options";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { cn } from "$lib/utils";

  let {
    fileTransfersAvailable = true,
  }: {
    fileTransfersAvailable?: boolean;
  } = $props();

  const calendarsStore = getCalendars();
  const calendarStore = getCalendar();
  const preferences = getPreferences();
  const calendarZoom = getCalendarZoom();
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const timeFormatOptions = $derived([
    { value: "24h", label: t("settings.appearance.timeFormat24h") },
    { value: "12h", label: t("settings.appearance.timeFormat12h") },
  ]);
  const calendarZoomOptions = $derived(CALENDAR_ZOOM_PERCENT_LEVELS.map((percent) => ({
    value: String(percent),
    label: t("settings.appearance.calendarZoomOption", percent, calendarZoomGridMinutesForPercent(percent)),
  })));
  const iconButtonClass = $derived(mobileShell
    ? "flex size-8 items-center justify-center rounded-lg border border-border bg-card text-foreground transition-colors active:bg-accent disabled:cursor-not-allowed disabled:opacity-40 disabled:active:bg-transparent dark:bg-transparent"
    : "flex h-7 w-7 items-center justify-center rounded-md border border-border bg-card text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-card dark:bg-transparent dark:disabled:hover:bg-transparent");
  const MAX_VISIBLE_IMPORT_WARNINGS = 8;
  const TOAST_TIMEOUT_MS = 5_000;

  /**
   * One `.ics` entry as returned by the Rust import command. `name` is
   * always a basename (no directory components) and `contents` is the
   * UTF-8 decoded entry body.
   */
  type IcsZipEntry = { name: string; contents: string };

  /** Aggregate counters used to build the summary toast after an import. */
  type ImportTotals = {
    added: number;
    updated: number;
    skippedOlder: number;
    calendars: number;
    warnings: string[];
  };

  let toast = $state<string | undefined>(undefined);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingDelete = $state<Calendar | undefined>(undefined);
  let counts = $state<Record<string, number>>({});
  let importWarnings = $state<string[]>([]);
  let isImporting = $state(false);

  /**
   * Live import progress. `total` is set once the picked entry list
   * resolves (one entry for a plain `.ics`, every `.ics` entry for a
   * `.zip`); `current` increments before each entry begins. It stays
   * undefined while the picker is open, leaving the spinner-only state.
   */
  let importProgress = $state<
    { current: number; total: number; label: string } | undefined
  >(undefined);
  let loadingIcsParser: Promise<typeof import("$lib/calendar/ics/parser")> | null = null;

  function loadIcsParser(): Promise<typeof import("$lib/calendar/ics/parser")> {
    loadingIcsParser ??= import("$lib/calendar/ics/parser");
    return loadingIcsParser;
  }

  function flashToast(message: string) {
    toast = message;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast = undefined;
    }, TOAST_TIMEOUT_MS);
  }

  function dismissToast(): void {
    if (toastTimer) {
      clearTimeout(toastTimer);
      toastTimer = undefined;
    }
    toast = undefined;
  }

  function errorMessage(err: unknown, fallback: string): string {
    if (err instanceof Error && err.message.trim()) return err.message;
    if (typeof err === "string" && err.trim()) return err;
    return fallback;
  }

  function visibleImportWarnings(warnings: string[]): string[] {
    const uniqueWarnings = Array.from(new Set(warnings));
    const visibleWarnings = uniqueWarnings.slice(0, MAX_VISIBLE_IMPORT_WARNINGS);
    const hiddenCount = uniqueWarnings.length - visibleWarnings.length;
    if (hiddenCount > 0) {
      visibleWarnings.push(t("settings.calendars.moreWarnings", hiddenCount));
    }
    return visibleWarnings;
  }

  function importButtonLabel(): string {
    if (!isImporting) return t("settings.calendars.importCalendar");
    if (!importProgress) return t("settings.calendars.importingCalendar");
    return t(
      "settings.calendars.importingEntry",
      importProgress.label,
      importProgress.current,
      importProgress.total,
    );
  }

  async function refreshCounts() {
    const next: Record<string, number> = {};
    for (const calendar of calendarsStore.list) {
      next[calendar.id] = await calendarsStore.countEvents(calendar.id);
    }
    counts = next;
  }

  onMount(() => {
    void refreshCounts();
  });

  $effect(() => {
    // Re-count whenever the calendars list changes (add/remove).
    calendarsStore.list.length;
    void refreshCounts();
  });

  /**
   * Parse a single `.ics` payload, upsert into a calendar grouping keyed by
   * `groupingFilename`, and fold the result into `totals`. Same code path
   * for plain `.ics` files and individual entries inside `.zip` bundles.
   */
  async function importIcsText(
    text: string,
    groupingFilename: string,
    totals: ImportTotals,
    sourceKind: "import-file" | "import-zip-entry",
  ): Promise<void> {
    const { parseIcs } = await loadIcsParser();
    const parsed = parseIcs(text);
    const hasPreservation = (parsed.preservation?.objects.length ?? 0) > 0;
    if (parsed.events.length === 0 && !hasPreservation) {
      if (parsed.warnings.length > 0) totals.warnings.push(...parsed.warnings);
      return;
    }
    const targetCalendar = await calendarsStore.findOrCreateImported(groupingFilename);
    const summary = await calendarStore.bulkImport(parsed.events, targetCalendar.id, {
      preservation: parsed.preservation,
      sourceName: groupingFilename,
      sourceKind,
    });
    totals.added += summary.added;
    totals.updated += summary.updated;
    totals.skippedOlder += summary.skippedOlder;
    totals.calendars += 1;
    if (summary.warnings.length > 0) totals.warnings.push(...summary.warnings);
    if (parsed.warnings.length > 0) totals.warnings.push(...parsed.warnings);
  }

  function summarizeTotals(totals: ImportTotals): string {
    const parts: string[] = [];
    if (totals.added) parts.push(t("settings.calendars.newEvents", totals.added));
    if (totals.updated) parts.push(t("settings.calendars.updatedEvents", totals.updated));
    if (totals.skippedOlder) parts.push(t("settings.calendars.olderSkipped", totals.skippedOlder));
    const summaryLine = parts.length > 0 ? parts.join(", ") : t("settings.calendars.noChanges");
    if (totals.calendars > 1) {
      return t("settings.calendars.importedCalendars", totals.calendars, summaryLine);
    }
    return t("settings.calendars.imported", summaryLine);
  }

  async function handleImport() {
    if (isImporting) return;

    isImporting = true;
    importWarnings = [];
    try {
      const entries = await invoke<IcsZipEntry[] | null>("vault_pick_and_read_ics_import");
      if (!entries) return;

      const totals: ImportTotals = {
        added: 0,
        updated: 0,
        skippedOlder: 0,
        calendars: 0,
        warnings: [],
      };

      if (entries.length === 0) {
        flashToast(t("settings.calendars.noIcsFiles"));
        return;
      }
      importProgress = { current: 0, total: entries.length, label: entries[0].name };
      const sourceKind = entries.length > 1 ? "import-zip-entry" : "import-file";
      for (let i = 0; i < entries.length; i++) {
        importProgress = {
          current: i + 1,
          total: entries.length,
          label: entries[i].name,
        };
        await importIcsText(entries[i].contents, entries[i].name, totals, sourceKind);
      }

      if (totals.calendars === 0) {
        importWarnings = visibleImportWarnings(totals.warnings);
        flashToast(totals.warnings[0] ?? t("settings.calendars.noEventsFound"));
        return;
      }

      const summaryLine = summarizeTotals(totals);
      if (totals.warnings.length > 0) {
        for (const w of totals.warnings) console.warn("[ics import]", w);
        importWarnings = visibleImportWarnings(totals.warnings);
        flashToast(t("settings.calendars.withWarnings", summaryLine, totals.warnings.length));
      } else {
        importWarnings = [];
        flashToast(summaryLine);
      }
      await refreshCounts();
    } catch (err) {
      console.error("ics import failed", err);
      importWarnings = [];
      flashToast(errorMessage(err, t("settings.calendars.importFailed")));
    } finally {
      isImporting = false;
      importProgress = undefined;
    }
  }

  async function handleExport(calendar: Calendar) {
    try {
      const ics = await calendarStore.exportCalendarAsIcs(calendar);
      const displayName = calendarDisplayName(calendar);
      const saved = await invoke<boolean>("vault_pick_and_write_ics_export", {
        defaultName: `${displayName.replace(/[^\w.-]+/g, "_")}.ics`,
        contents: ics,
      });
      if (saved) flashToast(t("settings.calendars.exportedToFile"));
    } catch (err) {
      console.error("ics export failed", err);
      flashToast(errorMessage(err, t("settings.calendars.exportFailed")));
    }
  }

  function handleDelete(calendar: Calendar) {
    pendingDelete = calendar;
  }

  async function confirmDelete() {
    if (!pendingDelete) return;
    const target = pendingDelete;
    pendingDelete = undefined;
    try {
      await calendarsStore.remove(target.id);
      await calendarStore.load();
      await refreshCounts();
      flashToast(t("settings.calendars.deleted", calendarDisplayName(target)));
    } catch (err) {
      console.error("delete calendar failed", err);
      flashToast(errorMessage(err, t("settings.calendars.deleteFailed")));
    }
  }

  function cancelDelete() {
    pendingDelete = undefined;
  }

  function handleCalendarZoomChange(value: string): void {
    const percent = Number(value);
    if (Number.isFinite(percent)) calendarZoom.setZoomPercent(percent);
  }

  function handleTimeFormatChange(value: string): void {
    if (isCalendarTimeFormat(value)) preferences.setCalendarTimeFormat(value);
  }
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.calendars.generalHeading")}</h2>
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.appearance.calendarZoom")}
        descriptionShortcuts={mobileShell ? [] : ["Shift + +", "Shift + -", "Shift + 0"]}
        value={String(calendarZoom.zoomPercent)}
        options={calendarZoomOptions}
        onChange={handleCalendarZoomChange}
        canReset={!calendarZoom.isDefault}
        onReset={() => calendarZoom.reset()}
      />
      <Select
        label={t("settings.appearance.timeFormat")}
        description={t("settings.appearance.timeFormatDescription")}
        value={preferences.calendarTimeFormat}
        options={timeFormatOptions}
        onChange={handleTimeFormatChange}
        canReset={preferences.calendarTimeFormat !== DEFAULT_CALENDAR_TIME_FORMAT}
        onReset={() => preferences.resetCalendarTimeFormat()}
      />
      <SwitchField
        label={t("settings.appearance.dimPastEventColors")}
        description={t("settings.appearance.dimPastEventColorsDescription")}
        checked={preferences.calendarDimPastEvents}
        onChange={(checked) => preferences.setCalendarDimPastEvents(checked)}
      />
    </div>
  </section>

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.calendars.heading")}</h2>

    {#if importWarnings.length > 0}
      <section
        class="mx-1 rounded-md border border-border bg-muted/20 px-3 py-2 text-[0.766667rem] text-foreground"
      >
        <h3 class="font-medium">{t("settings.calendars.importWarnings")}</h3>
        <ul class="mt-1 list-disc space-y-1 pl-4 text-muted-foreground">
          {#each importWarnings as warning}
            <li>{warning}</li>
          {/each}
        </ul>
      </section>
    {/if}

    <div class="flex flex-col">
      {#each calendarsStore.list as calendar (calendar.id)}
        {@const displayName = calendarDisplayName(calendar)}
        {@const importDate = calendarImportDate(calendar, locale)}
        <div
          class={cn(
            "flex items-center justify-between gap-1 rounded-md px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch",
            mobileShell && "min-h-14 gap-2 rounded-xl py-2",
          )}
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-[0.866667rem] text-foreground">{displayName}</span>
              <span
                class="shrink-0 text-[0.733333rem] font-medium uppercase tracking-wide text-muted-foreground"
              >
                {calendar.source}
              </span>
            </div>
            <div class="mt-0.5 flex items-center gap-2 text-[0.733333rem] text-muted-foreground">
              <span>{t("settings.calendars.eventCount", counts[calendar.id] ?? 0)}</span>
              {#if importDate}
                <span>{t("settings.calendars.importedOn", importDate)}</span>
              {/if}
            </div>
          </div>
          <div class="flex shrink-0 items-center justify-end gap-1">
            {#if fileTransfersAvailable}
              <button
                type="button"
                onclick={() => handleExport(calendar)}
                disabled={(counts[calendar.id] ?? 0) === 0}
                aria-label={t("settings.calendars.exportCalendar", displayName)}
                data-app-tooltip-disabled="true"
                class={iconButtonClass}
              >
                <Download size={13} strokeWidth={2} />
              </button>
            {/if}
            {#if calendar.id === "local"}
              <button
                type="button"
                disabled
                aria-label={t("settings.calendars.localCannotDelete")}
                data-app-tooltip-disabled="true"
                class={iconButtonClass}
              >
                <Trash2 size={13} strokeWidth={2} />
              </button>
            {:else}
              <button
                type="button"
                onclick={() => handleDelete(calendar)}
                aria-label={t("settings.calendars.deleteCalendar", displayName)}
                data-app-tooltip-disabled="true"
                class={iconButtonClass}
              >
                <Trash2 size={13} strokeWidth={2} />
              </button>
            {/if}
          </div>
        </div>
      {/each}
      {#if fileTransfersAvailable}
        <button
          type="button"
          onclick={handleImport}
          disabled={isImporting}
          class={cn(
            mobileShell
              ? "flex min-h-12 w-full min-w-0 items-center gap-2 rounded-xl px-3 text-sm text-foreground active:bg-accent/40 focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              : "flex w-full min-w-0 items-center gap-2 rounded-md px-1 py-1 text-[0.866667rem] text-foreground transition-colors hover:bg-accent/25 focus:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            "disabled:cursor-not-allowed disabled:opacity-60",
          )}
        >
          {#if isImporting}
            <LoaderCircle
              size={13}
              strokeWidth={1.75}
              class="shrink-0 animate-spin text-muted-foreground motion-reduce:animate-none"
            />
          {:else}
            <Upload size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
          {/if}
          <span class="truncate">{importButtonLabel()}</span>
        </button>
      {/if}
    </div>
  </section>

  {#if toast}
    <ActionToast message={toast} onDismiss={dismissToast} />
  {/if}
</div>

{#if pendingDelete}
  <ConfirmDialog
    title={t("settings.calendars.deleteTitle")}
    message={t(
      "settings.calendars.deleteMessage",
      calendarDisplayName(pendingDelete),
      counts[pendingDelete.id] ?? 0,
    )}
    confirmLabel={t("settings.calendars.deleteConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmDelete}
    onCancel={cancelDelete}
  />
{/if}
