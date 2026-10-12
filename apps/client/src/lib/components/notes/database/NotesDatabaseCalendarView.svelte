<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { untrack, type Snippet } from "svelte";
  import { databaseResource, notesDatabaseSession, rememberDatabaseScroll } from "$lib/notes/database/session.svelte";
  import NotesDatabasePropertyValue from "./NotesDatabasePropertyValue.svelte";
  import CollectionQuickAdd from "$lib/components/collections/CollectionQuickAdd.svelte";
  import CollectionCard from "$lib/components/collections/CollectionCard.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import { COLLECTION_VIEW_SETTINGS_PANEL_WIDTH } from "$lib/components/collections/collection-panel-width";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSelect from "$lib/components/collections/CollectionMenuSelect.svelte";
  import { COLLECTION_PROPERTY_ICONS } from "$lib/components/collections/property-icons";
  import { notesPropertyKind } from "./property-kinds";
  import NotesDatabaseSettingsFooter from "./NotesDatabaseSettingsFooter.svelte";
  import NotesDatabaseFilterControls from "./NotesDatabaseFilterControls.svelte";
  import { notesDatabaseFilterCount } from "$lib/notes/database/query-controls";
  import NotesDatabaseSortControls from "./NotesDatabaseSortControls.svelte";
  import NotesDatabaseQueryBar from "./NotesDatabaseQueryBar.svelte";
  import {
    createNotesDataSourceRowPage,
    duplicateNotesPage,
    getNotesDataSourceCalendarView,
    trashNotesPage,
    updateNotesDataSourceCalendarView,
  } from "$lib/api/notes";
  import { formatDateTime, formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseCalendarColumns,
    notesDatabaseCalendarConfigurationFromView,
    notesDatabaseCalendarDateColumns,
    notesDatabaseCalendarDays,
    notesDatabaseCalendarFiltersFromView,
    notesDatabaseCalendarRowCreateProperties,
    notesDatabaseCalendarRowText,
    notesDatabaseCalendarRowTitle,
    notesDatabaseCalendarShiftMonth,
    notesDatabaseCalendarSortsFromView,
    notesDatabaseCalendarUpdate,
    notesDatabaseCalendarVisibleColumns,
    notesDatabaseCalendarMonthRange,
  } from "$lib/notes/database/calendar";
  import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { mergeNotesDatabaseRows } from "$lib/notes/database/view-window";
  import NotesDatabaseWindowSentinel from "./NotesDatabaseWindowSentinel.svelte";
  import type {
    NotesDatabaseCalendarConfiguration,
    NotesDatabaseCalendarRowOpenMode,
    NotesDatabaseTableFilter,
    NotesDatabaseTableSort,
    NotesDatabaseViewScope,
    NotesDataSourceCalendarView,
    NotesPage,
  } from "$lib/notes/types";
  import { Temporal } from "@js-temporal/polyfill";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import FileText from "@lucide/svelte/icons/file-text";
  import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";

  let {
    dataSourceId,
    databaseId = null,
    viewId = null,
    onSelectPage,
    onSavingChange = () => {},
    onReady = () => {},
    reloadKey = 0,
    settingsOpen = false,
    editingLocked = false,
    settingsAnchor = null,
    settingsHeader,
    onCloseSettings,
    onEditProperties,
  }: {
    dataSourceId: string;
    databaseId?: string | null;
    viewId?: string | null;
    onSelectPage: (pageId: string) => void;
    onSavingChange?: (saving: boolean) => void;
    onReady?: () => void;
    reloadKey?: number;
    settingsOpen?: boolean;
    editingLocked?: boolean;
    settingsAnchor?: HTMLElement | null;
    settingsHeader?: Snippet;
    onCloseSettings: () => void;
    onEditProperties: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;

  let calendar = $state<NotesDataSourceCalendarView | null>(untrack(() => notesDatabaseSession.read(databaseResource("calendar", dataSourceId, viewScope()))));
  let loading = $state(false);
  let loadingMore = $state(false);
  let requestId = 0;
  let mutating = $state(false);

  $effect(() => {
    onSavingChange(mutating);
    return () => onSavingChange(false);
  });
  let error = $state<string | null>(null);
  let selectedPanelRowId = $state<string | null>(null);
  let lastLoadSignature = $state("");
  let lastReloadKey = untrack(() => reloadKey);

  const columns = $derived(calendar ? notesDatabaseCalendarColumns(calendar.data_source, calendar.view) : []);
  const configuration = $derived(
    calendar ? notesDatabaseCalendarConfigurationFromView(calendar.view) : defaultConfiguration(),
  );
  const dateColumns = $derived(notesDatabaseCalendarDateColumns(columns));
  const dateColumn = $derived(
    configuration.date_property_id
      ? dateColumns.find((column) => column.id === configuration.date_property_id) ?? null
      : null,
  );
  const visibleColumns = $derived(notesDatabaseCalendarVisibleColumns(columns, configuration));
  const filters = $derived(calendar ? notesDatabaseCalendarFiltersFromView(calendar.view) : []);
  const sorts = $derived(calendar ? notesDatabaseCalendarSortsFromView(calendar.view) : []);
  const days = $derived(calendar ? notesDatabaseCalendarDays(calendar.rows, dateColumn, configuration) : []);
  const selectedPanelRow = $derived(calendar?.rows.find((row) => row.id === selectedPanelRowId) ?? null);

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (mutating) return;
    const signature = `${dataSourceId}:${databaseId ?? ""}:${viewId ?? ""}:${reloadKey}:${revision}`;
    if (signature === lastLoadSignature) return;
    const force = reloadKey !== lastReloadKey;
    lastReloadKey = reloadKey;
    lastLoadSignature = signature;
    void loadCalendar(force);
  });

  function viewScope(): NotesDatabaseViewScope {
    return { databaseId, viewId };
  }

  $effect(() => { if (calendar || error) onReady(); });

  async function loadCalendar(force = true): Promise<NotesDataSourceCalendarView | null> {
    const currentRequest = ++requestId;
    loading = !calendar;
    loadingMore = false;
    error = null;
    try {
      const sourceId = dataSourceId;
      const scope = viewScope();
      const resource = databaseResource("calendar", sourceId, scope);
      const loaded = await notesDatabaseSession.load(resource, () => getNotesDataSourceCalendarView(sourceId, scope), force);
      if (currentRequest !== requestId) return null;
      calendar = loaded;
      if (selectedPanelRowId && !loaded.rows.some((row) => row.id === selectedPanelRowId)) {
        selectedPanelRowId = null;
      }
      return loaded;
    } catch (caught) {
      if (currentRequest !== requestId) return null;
      error = caught instanceof Error ? caught.message : String(caught);
      return null;
    } finally {
      if (currentRequest === requestId) loading = false;
    }
  }

  /** Reloads a view that failed to load, rejecting while it is still unavailable. */
  async function retryCalendar(): Promise<void> {
    await loadCalendar();
    if (error) throw new Error(error);
  }

  async function loadMoreCalendar(): Promise<void> {
    const current = calendar;
    if (!current?.has_more || !current.next_cursor || loadingMore) return;
    const currentRequest = requestId;
    const revision = notesDatabaseSession.revision;
    loadingMore = true;
    try {
      const loaded = await getNotesDataSourceCalendarView(dataSourceId, viewScope(), {
        start_cursor: current.next_cursor,
        range_start: configuration.range_start,
        range_end: configuration.range_end,
      });
      if (currentRequest !== requestId || calendar !== current || revision !== notesDatabaseSession.revision) return;
      calendar = { ...loaded, rows: mergeNotesDatabaseRows(current.rows, loaded.rows) };
      notesDatabaseSession.write(databaseResource("calendar", dataSourceId, viewScope()), calendar);
    } catch (caught) {
      if (currentRequest === requestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (currentRequest === requestId) loadingMore = false;
    }
  }

  async function persistCalendar(
    nextConfiguration: NotesDatabaseCalendarConfiguration,
    nextVisibleColumns = visibleColumns,
    nextFilters = filters,
    nextSorts = sorts,
  ): Promise<void> {
    if (editingLocked || mutating || !calendar) return;
    const resource = databaseResource("calendar", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      calendar = await updateNotesDataSourceCalendarView(
        dataSourceId,
        notesDatabaseCalendarUpdate(nextConfiguration, nextVisibleColumns, nextFilters, nextSorts),
        viewScope(),
      );
      notesDatabaseSession.write(resource, calendar, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Persist filter edits without replacing the current layout configuration. */
  function saveFilters(nextFilters: NotesDatabaseTableFilter[]): void {
    void persistCalendar(configuration, visibleColumns, nextFilters, sorts);
  }

  /** Persist sort edits without replacing the current layout configuration. */
  function saveSorts(nextSorts: NotesDatabaseTableSort[]): void {
    void persistCalendar(configuration, visibleColumns, filters, nextSorts);
  }

  async function createRow(date: string, title: string): Promise<boolean> {
    if (!calendar || !configuration.date_property_id || mutating) return false;
    mutating = true;
    error = null;
    try {
      const loaded = await createNotesDataSourceRowPage(dataSourceId, {
        id: crypto.randomUUID(),
        first_block_id: crypto.randomUUID(),
        title,
        properties: notesDatabaseCalendarRowCreateProperties(
          calendar.data_source,
          configuration.date_property_id,
          date,
        ),
      });
      selectedPanelRowId = loaded.page.id;
      await loadCalendar();
      return true;
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
      return false;
    } finally {
      mutating = false;
    }
  }

  async function duplicateRow(row: NotesPage): Promise<void> {
    mutating = true;
    error = null;
    try {
      const loaded = await duplicateNotesPage(row.id, {});
      selectedPanelRowId = loaded.page.id;
      await loadCalendar();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  async function trashRow(row: NotesPage): Promise<void> {
    mutating = true;
    error = null;
    try {
      await trashNotesPage(row.id, true);
      if (selectedPanelRowId === row.id) selectedPanelRowId = null;
      await loadCalendar();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  function updateDateProperty(propertyId: string): void {
    const datePropertyId = propertyId || null;
    const nextConfiguration: NotesDatabaseCalendarConfiguration = {
      ...configuration,
      date_property_id: datePropertyId,
      visible_property_ids: configuration.visible_property_ids.filter((id) => id !== datePropertyId),
    };
    const nextVisibleColumns = visibleColumns.filter((column) => column.id !== datePropertyId);
    void persistCalendar(nextConfiguration, nextVisibleColumns);
  }

  function updateRowOpenMode(mode: NotesDatabaseCalendarRowOpenMode): void {
    void persistCalendar({ ...configuration, row_open_mode: mode });
  }

  function shiftMonth(months: number): void {
    void persistCalendar(notesDatabaseCalendarShiftMonth(configuration, months));
  }

  function goToday(): void {
    void persistCalendar({
      ...configuration,
      ...notesDatabaseCalendarMonthRange(Temporal.Now.plainDateISO().toString()),
    });
  }

  function updateRowProperty(columnId: string, visible: boolean): void {
    const byId = new Map(columns.map((column) => [column.id, column]));
    const nextIds = visible
      ? [...configuration.visible_property_ids, columnId]
      : configuration.visible_property_ids.filter((id) => id !== columnId);
    const nextVisibleColumns = nextIds
      .map((id) => byId.get(id))
      .filter((column): column is NotesDatabaseTableColumn =>
        column !== undefined && column.type !== "title" && column.id !== configuration.date_property_id
      );
    void persistCalendar({ ...configuration, visible_property_ids: nextIds }, nextVisibleColumns);
  }

  function openRow(row: NotesPage): void {
    if (configuration.row_open_mode === "side_panel") {
      selectedPanelRowId = row.id;
      return;
    }
    onSelectPage(row.id);
  }

  function rowTitle(row: NotesPage): string {
    return notesDatabaseCalendarRowTitle(row, columns, t("notes.untitled"));
  }

  function monthLabel(): string {
    return formatDateTime(
      localization.locale,
      new Date(`${configuration.range_start}T00:00:00Z`),
      { month: "long", year: "numeric", timeZone: "UTC" },
    );
  }

  function dayNumber(date: string): string {
    return String(Number.parseInt(date.slice(8, 10), 10));
  }

  function weekdayLabels(): string[] {
    return ["2026-06-01", "2026-06-02", "2026-06-03", "2026-06-04", "2026-06-05", "2026-06-06", "2026-06-07"]
      .map((date) => formatDateTime(
        localization.locale,
        new Date(`${date}T00:00:00Z`),
        { weekday: "short", timeZone: "UTC" },
      ));
  }

  function defaultConfiguration(): NotesDatabaseCalendarConfiguration {
    return {
      date_property_id: null,
      ...notesDatabaseCalendarMonthRange(Temporal.Now.plainDateISO().toString()),
      visible_property_ids: [],
      row_open_mode: "side_panel",
    };
  }
</script>
{#snippet viewControls()}
  <span class="sr-only" role="status">
    {#if loading}
      {t("notes.databaseCalendarLoading")}
    {:else if error}
      {t("notes.databaseCalendarFailed", error)}
    {:else}
      {t("notes.databaseCalendarRowsCount", calendar?.rows.length ?? 0)}
    {/if}
  </span>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseLayout")} icon={PanelsTopLeft} summary={t("notes.databaseViewCalendar")}>
    <CollectionMenuSelect label={t("notes.databaseCalendarDateProperty")} icon={CalendarDays} value={configuration.date_property_id ?? ""} disabled={editingLocked || loading || mutating || !calendar}
      options={[{ value: "", label: t("notes.databaseCalendarNoDateProperty") }, ...dateColumns.map((column) => ({ value: column.id, label: column.name, icon: COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)] }))]}
      onChange={updateDateProperty} />
    <CollectionMenuSelect label={t("notes.databaseTableOpenMode")} icon={ExternalLink} value={configuration.row_open_mode} disabled={editingLocked || loading || mutating || !calendar}
      options={[{ value: "side_panel", label: t("notes.databaseTableOpenSidePanel") }, { value: "full_page", label: t("notes.databaseTableOpenFullPage") }]}
      onChange={updateRowOpenMode} />
  </CollectionMenu>
{/snippet}

{#snippet propertyControls()}
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableColumns")} kind="properties" summary={formatNumber(localization.locale, visibleColumns.length)}>
    {#each columns.filter((column) => column.type !== "title" && column.id !== configuration.date_property_id) as column (column.id)}
      {@const visible = visibleColumns.some((visibleColumn) => visibleColumn.id === column.id)}
      <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={visible} disabled={mutating || editingLocked}
        onclick={() => updateRowProperty(column.id, !visible)} />
    {/each}
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableSorts")} kind="sort" activeCount={sorts.length} summary={formatList(localization.locale, sorts.map((sort) => columns.find((column) => column.id === sort.property_id)?.name ?? ""))}>
    <NotesDatabaseSortControls properties={columns} {sorts} pending={mutating || editingLocked} onChange={saveSorts} />
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableFilters")} kind="filter" activeCount={notesDatabaseFilterCount(filters)} summary={notesDatabaseFilterCount(filters) ? formatNumber(localization.locale, notesDatabaseFilterCount(filters)) : ""}>
    <NotesDatabaseFilterControls properties={columns} {filters} pending={mutating || editingLocked} onChange={saveFilters} />
  </CollectionMenu>
{/snippet}

<section class="space-y-3 pt-2" aria-label={t("notes.databaseCalendarTitle")}>

  <NotesDatabaseQueryBar properties={columns} {filters} {sorts} pending={mutating || editingLocked} onFiltersChange={saveFilters} onSortsChange={saveSorts} />
  {#if error && calendar}<Notice class="text-[0.8rem]" message={error} />{/if}
  {#if settingsOpen}
    <CollectionSettings label={t("notes.databaseViewSettings")} anchor={settingsAnchor} preferredWidth={COLLECTION_VIEW_SETTINGS_PANEL_WIDTH} showHeader={false} onClose={onCloseSettings}>
      {@render settingsHeader?.()}
      {@render viewControls()}
      {#if calendar}{@render propertyControls()}{/if}
      <NotesDatabaseSettingsFooter reloadLabel={t("notes.databaseCalendarReload")} {editingLocked} reloadDisabled={loading || mutating}
        onEditProperties={() => { onCloseSettings(); onEditProperties(); }} onReload={() => { void loadCalendar(); }} />
    </CollectionSettings>
  {/if}

  <div class="flex min-w-0 flex-wrap items-center gap-2">
    <button
      type="button"
      class="inline-flex size-8 items-center justify-center rounded-md hover:bg-accent disabled:pointer-events-none"
      disabled={editingLocked || loading || mutating || !calendar}
      aria-label={t("notes.databaseCalendarPreviousMonth")}
      title={t("notes.databaseCalendarPreviousMonth")}
      onclick={() => shiftMonth(-1)}
    >
      <ChevronLeft class="size-3.5" aria-hidden="true" />
    </button>
    <div class="min-w-36 text-[0.933333rem] font-medium text-foreground">{monthLabel()}</div>
    <button
      type="button"
      class="inline-flex size-8 items-center justify-center rounded-md hover:bg-accent disabled:pointer-events-none"
      disabled={editingLocked || loading || mutating || !calendar}
      aria-label={t("notes.databaseCalendarNextMonth")}
      title={t("notes.databaseCalendarNextMonth")}
      onclick={() => shiftMonth(1)}
    >
      <ChevronRight class="size-3.5" aria-hidden="true" />
    </button>
    <button
      type="button"
      class="inline-flex h-8 items-center rounded-md px-2 text-[0.8rem] hover:bg-accent disabled:pointer-events-none"
      disabled={editingLocked || loading || mutating || !calendar}
      onclick={goToday}
    >
      {t("notes.databaseCalendarToday")}
    </button>
  </div>

  {#if !calendar && error}
    <div class="min-h-32 text-[0.8rem]"><Notice layout="fill" message={t("notes.databaseCalendarFailed", error)} copy={error} onRetry={retryCalendar} /></div>
  {:else if !calendar}<NotesLoadingSkeleton kind="calendar" />{/if}
  {#if calendar}
    {#if dateColumns.length === 0}
      <div class="rounded-md border border-border p-3 text-[0.866667rem] text-muted-foreground">
        {t("notes.databaseCalendarNoDateProperties")}
      </div>
    {:else}
      <div class="grid gap-2 @container">
        <!-- The bottom padding keeps the grid border inside the scroll clip when zoom gives rows fractional heights. -->
        <div use:rememberDatabaseScroll={databaseResource("calendar", dataSourceId, viewScope()).key} class="min-w-0 overflow-x-auto pb-px">
        <div class="grid min-w-2xl grid-cols-7 gap-px overflow-hidden rounded-md border border-border bg-border text-[0.8rem]">
          {#each weekdayLabels() as weekday}
            <div class="bg-muted px-2 py-1 text-center font-medium text-muted-foreground">{weekday}</div>
          {/each}
          {#each days as day (day.date)}
            <div class={`min-h-32 min-w-0 bg-background p-1 ${day.in_month ? "" : "opacity-45"}`}>
              <div class="mb-1 flex min-w-0 items-center justify-between gap-1">
                <span class="flex size-7 items-center justify-center rounded-full text-[0.8rem] font-medium" class:bg-primary={day.date === Temporal.Now.plainDateISO().toString()} class:text-primary-foreground={day.date === Temporal.Now.plainDateISO().toString()}>{dayNumber(day.date)}</span>
                {#if day.in_month && configuration.date_property_id}
                  <CollectionMenu kind="new" iconOnly label={t("notes.databaseCalendarCreateOnDate", day.date)}>
                    <CollectionQuickAdd active label={t("notes.databaseRowsNewPlaceholder")} disabled={loading || mutating} onSubmit={(title) => createRow(day.date, title)} />
                  </CollectionMenu>
                {/if}
              </div>
              <div class="grid gap-1">
                {#each day.rows as row (row.id)}
                  <CollectionCard title={rowTitle(row)} onOpen={() => openRow(row)} compact>
                    {#snippet actions()}
                      <CollectionMenu kind="actions" iconOnly showHeader={false} label={t("notes.databaseTableRowActions")}>
                        <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" onclick={() => onSelectPage(row.id)}><ExternalLink class="size-4" />{t("notes.databaseRowsOpen", rowTitle(row))}</button>
                        <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" disabled={mutating} onclick={() => { void duplicateRow(row); }}><Copy class="size-4" />{t("notes.duplicatePage")}</button>
                        <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left text-destructive hover:bg-destructive/10" disabled={mutating} onclick={() => { void trashRow(row); }}><Trash2 class="size-4" />{t("notes.databaseRowsTrash", rowTitle(row))}</button>
                      </CollectionMenu>
                    {/snippet}
                    {#each visibleColumns as column (column.id)}
                      {@const value = notesDatabaseCalendarRowText(row, column)}
                      {#if value}<div class="min-w-0 text-[0.733333rem] text-muted-foreground" title={column.name}><NotesDatabasePropertyValue {row} {column} /></div>{/if}
                    {/each}
                  </CollectionCard>
                {/each}
              </div>
            </div>
          {/each}
        </div>
        </div>
      </div>
    {/if}
  {/if}

  {#if calendar}
    <NotesDatabaseWindowSentinel hasMore={calendar.has_more} loading={loadingMore} onLoad={loadMoreCalendar} />
  {/if}

  {#if selectedPanelRow}
    <aside class="rounded-md border border-border bg-background p-3 shadow-sm">
      <div class="mb-2 flex min-w-0 items-center justify-between gap-2">
        <div class="flex min-w-0 items-center gap-2">
          <FileText class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <div class="min-w-0 truncate text-[0.933333rem] font-medium">{rowTitle(selectedPanelRow)}</div>
        </div>
        <button
          type="button"
          class="rounded-md px-2 py-1 text-[0.8rem] hover:bg-accent"
          onclick={() => {
            selectedPanelRowId = null;
          }}
        >
          {t("common.close")}
        </button>
      </div>
      <div class="grid gap-1 text-[0.8rem] text-muted-foreground">
        {#each columns.filter((column) => column.type !== "title") as column (column.id)}
          {@const value = notesDatabaseCalendarRowText(selectedPanelRow, column)}
          {#if value}
            <div class="grid grid-cols-[8rem_minmax(0,1fr)] gap-2">
              <span class="truncate">{column.name}</span>
              <span class="min-w-0 truncate text-foreground">{value}</span>
            </div>
          {/if}
        {/each}
      </div>
      <button
        type="button"
        class="mt-3 inline-flex h-8 items-center gap-1 rounded-md px-2 text-[0.8rem] hover:bg-accent"
        onclick={() => onSelectPage(selectedPanelRow.id)}
      >
        <ExternalLink class="size-3.5" aria-hidden="true" />
        <span>{t("notes.databaseRowsOpen", rowTitle(selectedPanelRow))}</span>
      </button>
    </aside>
  {/if}
</section>
