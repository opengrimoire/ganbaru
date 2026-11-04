<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { untrack, type Snippet } from "svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import { COLLECTION_VIEW_SETTINGS_PANEL_WIDTH } from "$lib/components/collections/collection-panel-width";
  import { databaseResource, notesDatabaseSession, rememberDatabaseScroll } from "$lib/notes/database/session.svelte";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSelect from "$lib/components/collections/CollectionMenuSelect.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
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
    getNotesDataSourceTimelineView,
    trashNotesPage,
    updateNotesDataSourceTimelineView,
    updateNotesDataSourceRowProperty,
  } from "$lib/api/notes";
  import { formatDateTime, formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseTimelineColumns,
    notesDatabaseTimelineConfigurationFromView,
    notesDatabaseTimelineDateColumns,
    notesDatabaseTimelineDates,
    notesDatabaseTimelineResizedDateValue,
    notesDatabaseTimelineFiltersFromView,
    notesDatabaseTimelineGroupableColumns,
    notesDatabaseTimelineGroups,
    notesDatabaseTimelineItems,
    notesDatabaseTimelineMonthRange,
    notesDatabaseTimelineRowCreateProperties,
    notesDatabaseTimelineRowText,
    notesDatabaseTimelineRowTitle,
    notesDatabaseTimelineShiftMonth,
    notesDatabaseTimelineSortsFromView,
    notesDatabaseTimelineUpdate,
    notesDatabaseTimelineVisibleColumns,
  } from "$lib/notes/database/timeline";
  import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { mergeNotesDatabaseRows } from "$lib/notes/database/view-window";
  import NotesDatabaseWindowSentinel from "./NotesDatabaseWindowSentinel.svelte";
  import type {
    NotesDatabaseTableFilter,
    NotesDatabaseTableSort,
    NotesDatabaseTimelineConfiguration,
    NotesDatabaseTimelineRowOpenMode,
    NotesDatabaseViewScope,
    NotesDataSourceTimelineGroup,
    NotesDataSourceTimelineView,
    NotesPage,
  } from "$lib/notes/types";
  import { Temporal } from "@js-temporal/polyfill";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import FileText from "@lucide/svelte/icons/file-text";
  import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash";

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

  let timeline = $state<NotesDataSourceTimelineView | null>(untrack(() => notesDatabaseSession.read(databaseResource("timeline", dataSourceId, viewScope()))));
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

  const columns = $derived(timeline ? notesDatabaseTimelineColumns(timeline.data_source, timeline.view) : []);
  const configuration = $derived(
    timeline ? notesDatabaseTimelineConfigurationFromView(timeline.view) : defaultConfiguration(),
  );
  const dateColumns = $derived(notesDatabaseTimelineDateColumns(columns));
  const dateColumn = $derived(
    configuration.date_property_id
      ? dateColumns.find((column) => column.id === configuration.date_property_id) ?? null
      : null,
  );
  const groupableColumns = $derived(notesDatabaseTimelineGroupableColumns(columns));
  const groupColumn = $derived(
    configuration.group_property_id
      ? groupableColumns.find((column) => column.id === configuration.group_property_id) ?? null
      : null,
  );
  const visibleColumns = $derived(notesDatabaseTimelineVisibleColumns(columns, configuration));
  const filters = $derived(timeline ? notesDatabaseTimelineFiltersFromView(timeline.view) : []);
  const sorts = $derived(timeline ? notesDatabaseTimelineSortsFromView(timeline.view) : []);
  const dates = $derived(notesDatabaseTimelineDates(configuration));
  const groups = $derived(timeline ? notesDatabaseTimelineGroups(timeline.rows, columns, configuration) : []);
  const visibleGroups = $derived(groups.filter((group) => !group.hidden));
  const hiddenGroups = $derived(groups.filter((group) => group.hidden));
  const selectedPanelRow = $derived(timeline?.rows.find((row) => row.id === selectedPanelRowId) ?? null);
  const timelineGridStyle = $derived(`grid-template-columns: repeat(${dates.length}, minmax(5.5rem, 5.5rem));`);

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (mutating) return;
    const signature = `${dataSourceId}:${databaseId ?? ""}:${viewId ?? ""}:${reloadKey}:${revision}`;
    if (signature === lastLoadSignature) return;
    const force = reloadKey !== lastReloadKey;
    lastReloadKey = reloadKey;
    lastLoadSignature = signature;
    void loadTimeline(force);
  });

  function viewScope(): NotesDatabaseViewScope {
    return { databaseId, viewId };
  }

  $effect(() => { if (timeline || error) onReady(); });

  async function loadTimeline(force = true): Promise<NotesDataSourceTimelineView | null> {
    const currentRequest = ++requestId;
    loading = !timeline;
    loadingMore = false;
    error = null;
    try {
      const sourceId = dataSourceId;
      const scope = viewScope();
      const resource = databaseResource("timeline", sourceId, scope);
      const loaded = await notesDatabaseSession.load(resource, () => getNotesDataSourceTimelineView(sourceId, scope), force);
      if (currentRequest !== requestId) return null;
      timeline = loaded;
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
  async function retryTimeline(): Promise<void> {
    await loadTimeline();
    if (error) throw new Error(error);
  }

  async function loadMoreTimeline(): Promise<void> {
    const current = timeline;
    if (!current?.has_more || !current.next_cursor || loadingMore) return;
    const currentRequest = requestId;
    const revision = notesDatabaseSession.revision;
    loadingMore = true;
    try {
      const loaded = await getNotesDataSourceTimelineView(dataSourceId, viewScope(), {
        start_cursor: current.next_cursor,
        range_start: configuration.range_start,
        range_end: configuration.range_end,
      });
      if (currentRequest !== requestId || timeline !== current || revision !== notesDatabaseSession.revision) return;
      timeline = { ...loaded, rows: mergeNotesDatabaseRows(current.rows, loaded.rows) };
      notesDatabaseSession.write(databaseResource("timeline", dataSourceId, viewScope()), timeline);
    } catch (caught) {
      if (currentRequest === requestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (currentRequest === requestId) loadingMore = false;
    }
  }

  async function persistTimeline(
    nextConfiguration: NotesDatabaseTimelineConfiguration,
    nextVisibleColumns = visibleColumns,
    nextFilters = filters,
    nextSorts = sorts,
  ): Promise<void> {
    if (editingLocked || mutating || !timeline) return;
    const resource = databaseResource("timeline", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      timeline = await updateNotesDataSourceTimelineView(
        dataSourceId,
        notesDatabaseTimelineUpdate(nextConfiguration, nextVisibleColumns, nextFilters, nextSorts),
        viewScope(),
      );
      notesDatabaseSession.write(resource, timeline, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Persist filter edits without replacing the current layout configuration. */
  function saveFilters(nextFilters: NotesDatabaseTableFilter[]): void {
    void persistTimeline(configuration, visibleColumns, nextFilters, sorts);
  }

  /** Persist sort edits without replacing the current layout configuration. */
  function saveSorts(nextSorts: NotesDatabaseTableSort[]): void {
    void persistTimeline(configuration, visibleColumns, filters, nextSorts);
  }

  async function createRow(date: string): Promise<void> {
    if (!timeline || !configuration.date_property_id) return;
    const title = t("notes.untitled");
    mutating = true;
    error = null;
    try {
      const loaded = await createNotesDataSourceRowPage(dataSourceId, {
        id: crypto.randomUUID(),
        first_block_id: crypto.randomUUID(),
        title,
        properties: notesDatabaseTimelineRowCreateProperties(
          timeline.data_source,
          configuration.date_property_id,
          date,
        ),
      });
      selectedPanelRowId = loaded.page.id;
      await loadTimeline();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  async function resizeRow(row: NotesPage, start: string, end: string): Promise<void> {
    if (!configuration.date_property_id) return;
    mutating = true;
    error = null;
    try {
      await updateNotesDataSourceRowProperty(dataSourceId, row.id, {
        property_id: configuration.date_property_id,
        value: notesDatabaseTimelineResizedDateValue(row, configuration.date_property_id, start, end),
      });
      await loadTimeline();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
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
      await loadTimeline();
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
      await loadTimeline();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  function updateDateProperty(propertyId: string): void {
    const datePropertyId = propertyId || null;
    const nextConfiguration: NotesDatabaseTimelineConfiguration = {
      ...configuration,
      date_property_id: datePropertyId,
      visible_property_ids: configuration.visible_property_ids.filter((id) => id !== datePropertyId),
    };
    const nextVisibleColumns = visibleColumns.filter((column) => column.id !== datePropertyId);
    void persistTimeline(nextConfiguration, nextVisibleColumns);
  }

  function updateGroupProperty(propertyId: string): void {
    const groupPropertyId = propertyId || null;
    const nextConfiguration: NotesDatabaseTimelineConfiguration = {
      ...configuration,
      group_property_id: groupPropertyId,
      group_order: [],
      hidden_group_ids: [],
      visible_property_ids: configuration.visible_property_ids.filter((id) => id !== groupPropertyId),
    };
    const nextVisibleColumns = visibleColumns.filter((column) => column.id !== groupPropertyId);
    void persistTimeline(nextConfiguration, nextVisibleColumns);
  }

  function updateRowOpenMode(mode: NotesDatabaseTimelineRowOpenMode): void {
    void persistTimeline({ ...configuration, row_open_mode: mode });
  }

  function shiftMonth(months: number): void {
    void persistTimeline(notesDatabaseTimelineShiftMonth(configuration, months));
  }

  function goToday(): void {
    void persistTimeline({
      ...configuration,
      ...notesDatabaseTimelineMonthRange(Temporal.Now.plainDateISO().toString()),
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
        column !== undefined
        && column.type !== "title"
        && column.id !== configuration.date_property_id
        && column.id !== configuration.group_property_id
      );
    void persistTimeline({ ...configuration, visible_property_ids: nextIds }, nextVisibleColumns);
  }

  function setGroupHidden(groupId: string, hidden: boolean): void {
    const hiddenGroupIds = hidden
      ? [...configuration.hidden_group_ids, groupId]
      : configuration.hidden_group_ids.filter((id) => id !== groupId);
    void persistTimeline({ ...configuration, hidden_group_ids: uniqueStrings(hiddenGroupIds) });
  }

  function openRow(row: NotesPage): void {
    if (configuration.row_open_mode === "side_panel") {
      selectedPanelRowId = row.id;
      return;
    }
    onSelectPage(row.id);
  }

  function rowTitle(row: NotesPage): string {
    return notesDatabaseTimelineRowTitle(row, columns, t("notes.untitled"));
  }

  function groupItems(group: NotesDataSourceTimelineGroup) {
    return notesDatabaseTimelineItems(group.rows, dateColumn, configuration);
  }

  function monthLabel(): string {
    return formatDateTime(
      localization.locale,
      new Date(`${configuration.range_start}T00:00:00Z`),
      { month: "long", year: "numeric", timeZone: "UTC" },
    );
  }

  function dateLabel(date: string): string {
    return formatDateTime(
      localization.locale,
      new Date(`${date}T00:00:00Z`),
      { day: "numeric", weekday: "short", timeZone: "UTC" },
    );
  }

  function uniqueStrings(values: string[]): string[] {
    return Array.from(new Set(values.filter(Boolean)));
  }

  function defaultConfiguration(): NotesDatabaseTimelineConfiguration {
    return {
      date_property_id: null,
      group_property_id: null,
      group_order: [],
      hidden_group_ids: [],
      ...notesDatabaseTimelineMonthRange(Temporal.Now.plainDateISO().toString()),
      visible_property_ids: [],
      row_open_mode: "side_panel",
    };
  }
</script>

<section class="space-y-3 pt-2" aria-label={t("notes.databaseTimelineTitle")}>

  <NotesDatabaseQueryBar properties={columns} {filters} {sorts} pending={mutating || editingLocked} onFiltersChange={saveFilters} onSortsChange={saveSorts} />
  {#if error && timeline}<Notice class="text-[0.8rem]" message={error} />{/if}
  {#if settingsOpen}
    <CollectionSettings label={t("notes.databaseViewSettings")} anchor={settingsAnchor} preferredWidth={COLLECTION_VIEW_SETTINGS_PANEL_WIDTH} showHeader={false} onClose={onCloseSettings}>
      {@render settingsHeader?.()}
      <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseLayout")} icon={PanelsTopLeft} summary={t("notes.databaseViewTimeline")}>
        <CollectionMenuSelect label={t("notes.databaseTimelineDateProperty")} icon={CalendarDays} value={configuration.date_property_id ?? ""} disabled={editingLocked || loading || mutating || !timeline}
          options={[{ value: "", label: t("notes.databaseTimelineNoDateProperty") }, ...dateColumns.map((column) => ({ value: column.id, label: column.name, icon: COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)] }))]}
          onChange={updateDateProperty} />
        <CollectionMenuSelect label={t("notes.databaseTableOpenMode")} icon={ExternalLink} value={configuration.row_open_mode} disabled={editingLocked || loading || mutating || !timeline}
          options={[{ value: "side_panel", label: t("notes.databaseTableOpenSidePanel") }, { value: "full_page", label: t("notes.databaseTableOpenFullPage") }]}
          onChange={updateRowOpenMode} />
      </CollectionMenu>
      <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseGroup")} kind="group" summary={groupColumn?.name ?? t("common.none")}>
        <CollectionMenuItem label={t("notes.databaseTimelineNoGroupProperty")} checked={!configuration.group_property_id} disabled={editingLocked || loading || mutating || !timeline} onclick={() => updateGroupProperty("")} />
        {#each groupableColumns as column (column.id)}
          <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={configuration.group_property_id === column.id}
            disabled={editingLocked || loading || mutating || !timeline} onclick={() => updateGroupProperty(column.id)} />
        {/each}
      </CollectionMenu>
      {#if timeline}
        <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableColumns")} kind="properties" summary={formatNumber(localization.locale, visibleColumns.length)}>
          {#each columns.filter((column) => column.type !== "title" && column.id !== configuration.date_property_id && column.id !== configuration.group_property_id) as column (column.id)}
            {@const visible = visibleColumns.some((visibleColumn) => visibleColumn.id === column.id)}
            <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={visible} disabled={mutating || editingLocked}
              onclick={() => updateRowProperty(column.id, !visible)} />
          {/each}
        </CollectionMenu>
        {#if groupColumn}
          <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTimelineHiddenGroups")} icon={EyeOff} summary={hiddenGroups.length > 0 ? formatNumber(localization.locale, hiddenGroups.length) : ""}>
            {#each visibleGroups as group (group.id)}
              <CollectionMenuItem icon={EyeOff} label={t("notes.databaseTimelineHideGroup", group.name)} disabled={mutating || editingLocked} onclick={() => setGroupHidden(group.id, true)} />
            {/each}
            {#if visibleGroups.length > 0 && hiddenGroups.length > 0}<CollectionMenuSeparator />{/if}
            {#each hiddenGroups as group (group.id)}
              <CollectionMenuItem icon={Eye} label={t("notes.databaseTimelineShowGroup", group.name)} disabled={mutating || editingLocked} onclick={() => setGroupHidden(group.id, false)} />
            {/each}
          </CollectionMenu>
        {/if}
        <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableSorts")} kind="sort" activeCount={sorts.length} summary={formatList(localization.locale, sorts.map((sort) => columns.find((column) => column.id === sort.property_id)?.name ?? ""))}>
          <NotesDatabaseSortControls properties={columns} {sorts} pending={mutating || editingLocked} onChange={saveSorts} />
        </CollectionMenu>
        <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableFilters")} kind="filter" activeCount={notesDatabaseFilterCount(filters)} summary={notesDatabaseFilterCount(filters) ? formatNumber(localization.locale, notesDatabaseFilterCount(filters)) : ""}>
          <NotesDatabaseFilterControls properties={columns} {filters} pending={mutating || editingLocked} onChange={saveFilters} />
        </CollectionMenu>
      {/if}
      <NotesDatabaseSettingsFooter reloadLabel={t("notes.databaseTimelineReload")} {editingLocked} reloadDisabled={loading || mutating}
        onEditProperties={() => { onCloseSettings(); onEditProperties(); }} onReload={() => { void loadTimeline(); }} />
    </CollectionSettings>
  {/if}

  <div class="flex min-w-0 flex-wrap items-center gap-2">
    <button
      type="button"
      class="inline-flex size-8 items-center justify-center rounded-md hover:bg-accent disabled:pointer-events-none"
      disabled={editingLocked || loading || mutating || !timeline}
      aria-label={t("notes.databaseTimelinePreviousMonth")}
      title={t("notes.databaseTimelinePreviousMonth")}
      onclick={() => shiftMonth(-1)}
    >
      <ChevronLeft class="size-3.5" aria-hidden="true" />
    </button>
    <div class="min-w-36 text-[0.933333rem] font-medium text-foreground">{monthLabel()}</div>
    <button
      type="button"
      class="inline-flex size-8 items-center justify-center rounded-md hover:bg-accent disabled:pointer-events-none"
      disabled={editingLocked || loading || mutating || !timeline}
      aria-label={t("notes.databaseTimelineNextMonth")}
      title={t("notes.databaseTimelineNextMonth")}
      onclick={() => shiftMonth(1)}
    >
      <ChevronRight class="size-3.5" aria-hidden="true" />
    </button>
    <button
      type="button"
      class="inline-flex h-8 items-center rounded-md px-2 text-[0.8rem] hover:bg-accent disabled:pointer-events-none"
      disabled={editingLocked || loading || mutating || !timeline}
      onclick={goToday}
    >
      {t("notes.databaseTimelineToday")}
    </button>

  </div>

  {#if !timeline && error}
    <div class="min-h-32 text-[0.8rem]"><Notice layout="fill" message={t("notes.databaseTimelineFailed", error)} copy={error} onRetry={retryTimeline} /></div>
  {:else if !timeline}<NotesLoadingSkeleton kind="timeline" />{/if}
  {#if timeline}
    {#if dateColumns.length === 0}
      <div class="rounded-md border border-border p-3 text-[0.866667rem] text-muted-foreground">
        {t("notes.databaseTimelineNoDateProperties")}
      </div>
    {:else}
      <div class="grid gap-2 @container">
        <div use:rememberDatabaseScroll={databaseResource("timeline", dataSourceId, viewScope()).key} class="overflow-x-auto rounded-md border border-border">
          <div class="min-w-max">
            <div class="grid border-b border-border bg-muted text-[0.733333rem] text-muted-foreground" style={timelineGridStyle}>
              {#each dates as date (date)}
                <div class="flex min-w-0 items-center justify-between gap-1 border-r border-border px-2 py-1 last:border-r-0">
                  <span class="truncate">{dateLabel(date)}</span>
                  {#if configuration.date_property_id}
                    <button
                      type="button"
                      class="inline-flex size-6 shrink-0 items-center justify-center rounded-md hover:bg-accent disabled:pointer-events-none"
                      disabled={loading || mutating}
                      aria-label={t("notes.databaseTimelineCreateOnDate", date)}
                      title={t("notes.databaseTimelineCreateOnDate", date)}
                      onclick={() => createRow(date)}
                    >
                      <Plus class="size-3" aria-hidden="true" />
                    </button>
                  {/if}
                </div>
              {/each}
            </div>

            {#each visibleGroups as group (group.id)}
              <section class="border-b border-border last:border-b-0">
                <div class="flex min-w-0 items-center justify-between gap-2 bg-background px-2 py-2 text-[0.8rem] font-medium text-foreground">
                  <span class="min-w-0 truncate">{group.name}</span>
                  <span class="shrink-0 text-muted-foreground">{group.rows.length}</span>
                </div>
                <div class="grid min-h-24 gap-1 bg-muted/30 p-2" style={timelineGridStyle}>
                  {#each groupItems(group) as item (item.row.id)}
                    <div
                      class="group grid min-w-48 gap-1 rounded-md border border-border bg-background p-2 shadow-sm"
                      style={`grid-column: ${item.grid_column};`}
                    >
                      <button
                        type="button"
                        class="min-w-0 truncate text-left text-[0.866667rem] font-medium text-foreground hover:underline"
                        onclick={() => openRow(item.row)}
                      >
                        {rowTitle(item.row)}
                      </button>
                      {#if visibleColumns.length > 0}
                        <div class="grid gap-0.5">
                          {#each visibleColumns as column (column.id)}
                            {@const value = notesDatabaseTimelineRowText(item.row, column)}
                            {#if value}
                              <div class="min-w-0 truncate text-[0.733333rem] text-muted-foreground">
                                {column.name}: {value}
                              </div>
                            {/if}
                          {/each}
                        </div>
                      {/if}
                      <div class="grid min-w-0 grid-cols-2 gap-1 text-[0.733333rem]">
                        <label class="grid min-w-0 gap-0.5">
                          <span class="truncate text-muted-foreground">{t("notes.databaseTimelineStartDate")}</span>
                          <input
                            type="date"
                            class="field h-7 min-h-0 min-w-0 px-1 text-foreground"
                            value={item.start}
                            disabled={mutating}
                            onchange={(event) => {
                              void resizeRow(item.row, event.currentTarget.value, item.end);
                            }}
                            onkeydown={(event) => event.stopPropagation()}
                          />
                        </label>
                        <label class="grid min-w-0 gap-0.5">
                          <span class="truncate text-muted-foreground">{t("notes.databaseTimelineEndDate")}</span>
                          <input
                            type="date"
                            class="field h-7 min-h-0 min-w-0 px-1 text-foreground"
                            value={item.end}
                            disabled={mutating}
                            onchange={(event) => {
                              void resizeRow(item.row, item.start, event.currentTarget.value);
                            }}
                            onkeydown={(event) => event.stopPropagation()}
                          />
                        </label>
                      </div>
                      <div class="flex min-w-0 items-center gap-1 opacity-100 @lg:opacity-0 @lg:group-focus-within:opacity-100 @lg:group-hover:opacity-100">
                        <button
                          type="button"
                          class="inline-flex size-6 items-center justify-center rounded-md hover:bg-accent"
                          aria-label={t("notes.databaseRowsOpen", rowTitle(item.row))}
                          title={t("notes.databaseRowsOpen", rowTitle(item.row))}
                          onclick={() => onSelectPage(item.row.id)}
                        >
                          <ExternalLink class="size-3" aria-hidden="true" />
                        </button>
                        <button
                          type="button"
                          class="inline-flex size-6 items-center justify-center rounded-md hover:bg-accent disabled:pointer-events-none"
                          disabled={mutating}
                          aria-label={t("notes.databaseRowsDuplicate", rowTitle(item.row))}
                          title={t("notes.databaseRowsDuplicate", rowTitle(item.row))}
                          onclick={() => duplicateRow(item.row)}
                        >
                          <Copy class="size-3" aria-hidden="true" />
                        </button>
                        <button
                          type="button"
                          class="inline-flex size-6 items-center justify-center rounded-md text-destructive hover:bg-destructive/10 disabled:pointer-events-none"
                          disabled={mutating}
                          aria-label={t("notes.databaseRowsTrash", rowTitle(item.row))}
                          title={t("notes.databaseRowsTrash", rowTitle(item.row))}
                          onclick={() => trashRow(item.row)}
                        >
                          <Trash2 class="size-3" aria-hidden="true" />
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              </section>
            {/each}
          </div>
        </div>
      </div>
    {/if}
  {/if}

  {#if timeline}
    <NotesDatabaseWindowSentinel hasMore={timeline.has_more} loading={loadingMore} onLoad={loadMoreTimeline} />
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
          {@const value = notesDatabaseTimelineRowText(selectedPanelRow, column)}
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
