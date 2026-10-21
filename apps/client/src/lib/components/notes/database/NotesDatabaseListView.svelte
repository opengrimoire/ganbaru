<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { untrack, type Snippet } from "svelte";
  import { databaseResource, notesDatabaseSession } from "$lib/notes/database/session.svelte";
  import NotesDatabasePropertyValue from "./NotesDatabasePropertyValue.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import { COLLECTION_VIEW_SETTINGS_PANEL_WIDTH } from "$lib/components/collections/collection-panel-width";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
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
    duplicateNotesPage,
    getNotesDataSourceListView,
    trashNotesPage,
    updateNotesDataSourceListView,
  } from "$lib/api/notes";
  import { formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseListColumns,
    notesDatabaseListConfigurationFromView,
    notesDatabaseListFiltersFromView,
    notesDatabaseListGroupableColumns,
    notesDatabaseListGroups,
    notesDatabaseListRowText,
    notesDatabaseListRowTitle,
    notesDatabaseListSortsFromView,
    notesDatabaseListUpdate,
    notesDatabaseListVisibleColumns,
  } from "$lib/notes/database/list";
  import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { mergeNotesDatabaseRows } from "$lib/notes/database/view-window";
  import NotesDatabaseWindowSentinel from "./NotesDatabaseWindowSentinel.svelte";
  import type {
    NotesDatabaseListConfiguration,
    NotesDatabaseListRowOpenMode,
    NotesDatabaseTableFilter,
    NotesDatabaseTableSort,
    NotesDatabaseViewScope,
    NotesDataSourceListGroup,
    NotesDataSourceListView,
    NotesPage,
  } from "$lib/notes/types";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
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

  let list = $state<NotesDataSourceListView | null>(untrack(() => notesDatabaseSession.read(databaseResource("list", dataSourceId, viewScope()))));
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

  const columns = $derived(list ? notesDatabaseListColumns(list.data_source, list.view) : []);
  const configuration = $derived(
    list ? notesDatabaseListConfigurationFromView(list.view) : defaultConfiguration(),
  );
  const groupableColumns = $derived(notesDatabaseListGroupableColumns(columns));
  const groupColumn = $derived(
    configuration.group_property_id
      ? groupableColumns.find((column) => column.id === configuration.group_property_id) ?? null
      : null,
  );
  const visibleColumns = $derived(notesDatabaseListVisibleColumns(columns, configuration));
  const filters = $derived(list ? notesDatabaseListFiltersFromView(list.view) : []);
  const sorts = $derived(list ? notesDatabaseListSortsFromView(list.view) : []);
  const groups = $derived(list ? notesDatabaseListGroups(list.rows, columns, configuration) : []);
  const visibleGroups = $derived(groups.filter((group) => !group.hidden));
  const hiddenGroups = $derived(groups.filter((group) => group.hidden));
  const selectedPanelRow = $derived(list?.rows.find((row) => row.id === selectedPanelRowId) ?? null);

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (mutating) return;
    const signature = `${dataSourceId}:${databaseId ?? ""}:${viewId ?? ""}:${reloadKey}:${revision}`;
    if (signature === lastLoadSignature) return;
    const force = reloadKey !== lastReloadKey;
    lastReloadKey = reloadKey;
    lastLoadSignature = signature;
    void loadList(force);
  });

  function viewScope(): NotesDatabaseViewScope {
    return { databaseId, viewId };
  }

  $effect(() => { if (list || error) onReady(); });

  async function loadList(force = true): Promise<NotesDataSourceListView | null> {
    const currentRequest = ++requestId;
    loading = !list;
    loadingMore = false;
    error = null;
    try {
      const sourceId = dataSourceId;
      const scope = viewScope();
      const resource = databaseResource("list", sourceId, scope);
      const loaded = await notesDatabaseSession.load(resource, () => getNotesDataSourceListView(sourceId, scope), force);
      if (currentRequest !== requestId) return null;
      list = loaded;
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
  async function retryList(): Promise<void> {
    await loadList();
    if (error) throw new Error(error);
  }

  async function loadMoreList(): Promise<void> {
    const current = list;
    if (!current?.has_more || !current.next_cursor || loadingMore) return;
    const currentRequest = requestId;
    const revision = notesDatabaseSession.revision;
    loadingMore = true;
    try {
      const loaded = await getNotesDataSourceListView(dataSourceId, viewScope(), {
        start_cursor: current.next_cursor,
      });
      if (currentRequest !== requestId || list !== current || revision !== notesDatabaseSession.revision) return;
      list = { ...loaded, rows: mergeNotesDatabaseRows(current.rows, loaded.rows) };
      notesDatabaseSession.write(databaseResource("list", dataSourceId, viewScope()), list);
    } catch (caught) {
      if (currentRequest === requestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (currentRequest === requestId) loadingMore = false;
    }
  }

  async function persistList(
    nextConfiguration: NotesDatabaseListConfiguration,
    nextVisibleColumns = visibleColumns,
    nextFilters = filters,
    nextSorts = sorts,
  ): Promise<void> {
    if (editingLocked || mutating || !list) return;
    const resource = databaseResource("list", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      list = await updateNotesDataSourceListView(
        dataSourceId,
        notesDatabaseListUpdate(nextConfiguration, nextVisibleColumns, nextFilters, nextSorts),
        viewScope(),
      );
      notesDatabaseSession.write(resource, list, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Persist filter edits without replacing the current layout configuration. */
  function saveFilters(nextFilters: NotesDatabaseTableFilter[]): void {
    void persistList(configuration, visibleColumns, nextFilters, sorts);
  }

  /** Persist sort edits without replacing the current layout configuration. */
  function saveSorts(nextSorts: NotesDatabaseTableSort[]): void {
    void persistList(configuration, visibleColumns, filters, nextSorts);
  }

  async function duplicateRow(row: NotesPage): Promise<void> {
    mutating = true;
    error = null;
    try {
      const loaded = await duplicateNotesPage(row.id, {});
      selectedPanelRowId = loaded.page.id;
      await loadList();
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
      await loadList();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  function updateGroupProperty(propertyId: string): void {
    const groupPropertyId = propertyId || null;
    const nextConfiguration: NotesDatabaseListConfiguration = {
      ...configuration,
      group_property_id: groupPropertyId,
      group_order: [],
      hidden_group_ids: [],
      visible_property_ids: configuration.visible_property_ids.filter((id) => id !== groupPropertyId),
    };
    const nextVisibleColumns = visibleColumns.filter((column) => column.id !== groupPropertyId);
    void persistList(nextConfiguration, nextVisibleColumns);
  }

  function updateRowOpenMode(mode: NotesDatabaseListRowOpenMode): void {
    void persistList({ ...configuration, row_open_mode: mode });
  }

  function updateRowProperty(columnId: string, visible: boolean): void {
    const byId = new Map(columns.map((column) => [column.id, column]));
    const nextIds = visible
      ? [...configuration.visible_property_ids, columnId]
      : configuration.visible_property_ids.filter((id) => id !== columnId);
    const nextVisibleColumns = nextIds
      .map((id) => byId.get(id))
      .filter((column): column is NotesDatabaseTableColumn =>
        column !== undefined && column.type !== "title" && column.id !== configuration.group_property_id
      );
    void persistList({ ...configuration, visible_property_ids: nextIds }, nextVisibleColumns);
  }

  function setGroupHidden(groupId: string, hidden: boolean): void {
    const hiddenGroupIds = hidden
      ? [...configuration.hidden_group_ids, groupId]
      : configuration.hidden_group_ids.filter((id) => id !== groupId);
    void persistList({ ...configuration, hidden_group_ids: uniqueStrings(hiddenGroupIds) });
  }

  function openRow(row: NotesPage): void {
    if (configuration.row_open_mode === "side_panel") {
      selectedPanelRowId = row.id;
      return;
    }
    onSelectPage(row.id);
  }

  function rowTitle(row: NotesPage): string {
    return notesDatabaseListRowTitle(row, columns, t("notes.untitled"));
  }

  function uniqueStrings(values: string[]): string[] {
    return Array.from(new Set(values.filter(Boolean)));
  }

  function defaultConfiguration(): NotesDatabaseListConfiguration {
    return {
      group_property_id: null,
      group_order: [],
      hidden_group_ids: [],
      visible_property_ids: [],
      row_open_mode: "side_panel",
    };
  }
  const listTemplate = $derived(`minmax(12rem, 1fr) ${visibleColumns.map(() => "minmax(0, 1fr)").join(" ")} 2rem`);
</script>
{#snippet viewControls()}
  <span class="sr-only" role="status">
    {#if loading}
      {t("notes.databaseListLoading")}
    {:else if error}
      {t("notes.databaseListFailed", error)}
    {:else}
      {t("notes.databaseListRowsCount", list?.rows.length ?? 0)}
    {/if}
  </span>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseLayout")} icon={PanelsTopLeft} summary={t("notes.databaseViewList")}>
    <CollectionMenuSelect label={t("notes.databaseTableOpenMode")} icon={ExternalLink} value={configuration.row_open_mode} disabled={editingLocked || loading || mutating || !list}
      options={[{ value: "side_panel", label: t("notes.databaseTableOpenSidePanel") }, { value: "full_page", label: t("notes.databaseTableOpenFullPage") }]}
      onChange={updateRowOpenMode} />
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseGroup")} kind="group" summary={groupColumn?.name ?? t("common.none")}>
    <CollectionMenuItem label={t("notes.databaseListNoGroupProperty")} checked={!configuration.group_property_id} disabled={editingLocked || loading || mutating || !list} onclick={() => updateGroupProperty("")} />
    {#each groupableColumns as column (column.id)}
      <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={configuration.group_property_id === column.id}
        disabled={editingLocked || loading || mutating || !list} onclick={() => updateGroupProperty(column.id)} />
    {/each}
  </CollectionMenu>
{/snippet}

{#snippet propertyControls()}
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableColumns")} kind="properties" summary={formatNumber(localization.locale, visibleColumns.length)}>
    {#each columns.filter((column) => column.type !== "title" && column.id !== configuration.group_property_id) as column (column.id)}
      {@const visible = visibleColumns.some((visibleColumn) => visibleColumn.id === column.id)}
      <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={visible} disabled={mutating || editingLocked}
        onclick={() => updateRowProperty(column.id, !visible)} />
    {/each}
  </CollectionMenu>
  {#if groupColumn}
    <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseListHiddenGroups")} icon={EyeOff} summary={hiddenGroups.length > 0 ? formatNumber(localization.locale, hiddenGroups.length) : ""}>
      {#each visibleGroups as group (group.id)}
        <CollectionMenuItem icon={EyeOff} label={t("notes.databaseListHideGroup", group.name)} disabled={mutating || editingLocked} onclick={() => setGroupHidden(group.id, true)} />
      {/each}
      {#if visibleGroups.length > 0 && hiddenGroups.length > 0}<CollectionMenuSeparator />{/if}
      {#each hiddenGroups as group (group.id)}
        <CollectionMenuItem icon={Eye} label={t("notes.databaseListShowGroup", group.name)} disabled={mutating || editingLocked} onclick={() => setGroupHidden(group.id, false)} />
      {/each}
    </CollectionMenu>
  {/if}
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableSorts")} kind="sort" activeCount={sorts.length} summary={formatList(localization.locale, sorts.map((sort) => columns.find((column) => column.id === sort.property_id)?.name ?? ""))}>
    <NotesDatabaseSortControls properties={columns} {sorts} pending={mutating || editingLocked} onChange={saveSorts} />
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableFilters")} kind="filter" activeCount={notesDatabaseFilterCount(filters)} summary={notesDatabaseFilterCount(filters) ? formatNumber(localization.locale, notesDatabaseFilterCount(filters)) : ""}>
    <NotesDatabaseFilterControls properties={columns} {filters} pending={mutating || editingLocked} onChange={saveFilters} />
  </CollectionMenu>
{/snippet}

<section class="space-y-3 pt-2" aria-label={t("notes.databaseListTitle")}>

  <NotesDatabaseQueryBar properties={columns} {filters} {sorts} pending={mutating || editingLocked} onFiltersChange={saveFilters} onSortsChange={saveSorts} />
  {#if error && list}<Notice class="text-[0.8rem]" message={error} />{/if}
  {#if settingsOpen}
    <CollectionSettings label={t("notes.databaseViewSettings")} anchor={settingsAnchor} preferredWidth={COLLECTION_VIEW_SETTINGS_PANEL_WIDTH} showHeader={false} onClose={onCloseSettings}>
      {@render settingsHeader?.()}
      {@render viewControls()}
      {#if list}{@render propertyControls()}{/if}
      <NotesDatabaseSettingsFooter reloadLabel={t("notes.databaseListReload")} {editingLocked} reloadDisabled={loading || mutating}
        onEditProperties={() => { onCloseSettings(); onEditProperties(); }} onReload={() => { void loadList(); }} />
    </CollectionSettings>
  {/if}

  {#if !list && error}
    <div class="min-h-32 text-[0.8rem]"><Notice layout="fill" message={t("notes.databaseListFailed", error)} copy={error} onRetry={retryList} /></div>
  {:else if !list}<NotesLoadingSkeleton kind="list" />{/if}
  {#if list}
    <div class="grid gap-2 @container">
      <CollectionRow template={listTemplate} compactTemplate="minmax(0, 1fr) 2rem" header>
        <CollectionCell>{t("notes.databaseSchemaName")}</CollectionCell>
        {#each visibleColumns as column (column.id)}<CollectionCell data-collection-secondary>{column.name}</CollectionCell>{/each}
        <div></div>
      </CollectionRow>
      <div class="grid min-w-0 gap-3">
        {#each visibleGroups as group (group.id)}
          <section class="min-w-0">
            {#if groupColumn}
              <div class="mb-1 flex min-h-11 min-w-0 items-center gap-2 border-b border-border/60 px-2">
                <span class="inline-flex size-2 rounded-full bg-muted-foreground" aria-hidden="true"></span>
                <h3 class="min-w-0 flex-1 truncate text-[0.8rem] font-medium text-muted-foreground">
                  {group.name}
                </h3>
                <span class="text-[0.733333rem] text-muted-foreground">
                  {t("notes.databaseListRowsCount", group.rows.length)}
                </span>
              </div>
            {/if}

            <div class="grid min-w-0">
              {#each group.rows as row (row.id)}
                {@const title = rowTitle(row)}
                <CollectionRow template={listTemplate} compactTemplate="minmax(0, 1fr) 2rem">
                  <CollectionCell>
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-2 rounded-sm text-left outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                    onclick={() => openRow(row)}
                  >
                    <FileText class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                    <span class="min-w-0 flex-1 truncate text-[0.866667rem] text-foreground">
                      {title}
                    </span>
                  </button>
                  </CollectionCell>
                  {#each visibleColumns as column (column.id)}
                    <CollectionCell data-collection-secondary><NotesDatabasePropertyValue {row} {column} /></CollectionCell>
                  {/each}
                  <CollectionMenu iconOnly kind="actions" label={t("notes.databaseTableRowActions")}>
                    <div class="grid gap-0">
                      <button
                        type="button"
                        class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[length:inherit] text-muted-foreground hover:bg-accent hover:text-foreground"
                        aria-label={t("notes.databaseRowsOpen", title)}
                        title={t("notes.databaseRowsOpen", title)}
                        onclick={() => openRow(row)}
                      >
                        <ExternalLink class="size-3.5" aria-hidden="true" />
                        <span class="truncate">{t("notes.databaseRowsOpen", title)}</span>
                      </button>
                      <button
                        type="button"
                        class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[length:inherit] text-muted-foreground hover:bg-accent hover:text-foreground"
                        disabled={mutating}
                        aria-label={t("notes.databaseRowsDuplicate", title)}
                        title={t("notes.databaseRowsDuplicate", title)}
                        onclick={() => {
                          void duplicateRow(row);
                        }}
                      >
                        <Copy class="size-3.5" aria-hidden="true" />
                        <span class="truncate">{t("notes.databaseRowsDuplicate", title)}</span>
                      </button>
                      <button
                        type="button"
                        class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[length:inherit] text-muted-foreground hover:bg-accent hover:text-foreground"
                        disabled={mutating}
                        aria-label={t("notes.databaseRowsTrash", title)}
                        title={t("notes.databaseRowsTrash", title)}
                        onclick={() => {
                          void trashRow(row);
                        }}
                      >
                        <Trash2 class="size-3.5" aria-hidden="true" />
                        <span class="truncate">{t("notes.databaseRowsTrash", title)}</span>
                      </button>
                    </div>
                  </CollectionMenu>
                </CollectionRow>
              {/each}
              {#if group.rows.length === 0}
                <p class="px-2 py-2 text-[0.8rem] text-muted-foreground">{t("notes.databaseRowsEmpty")}</p>
              {/if}
            </div>
          </section>
        {/each}
      </div>

      {#if list.rows.length === 0 && !loading && !error}
        <p class="text-[0.8rem] text-muted-foreground">{t("notes.databaseRowsEmpty")}</p>
      {/if}

      <NotesDatabaseWindowSentinel hasMore={list.has_more} loading={loadingMore} onLoad={loadMoreList} />

      {#if configuration.row_open_mode === "side_panel" && selectedPanelRow}
        <aside class="rounded-md border border-border p-3" aria-label={t("notes.databaseTableSidePanelTitle")}>
          {#if selectedPanelRow}
            {@const title = rowTitle(selectedPanelRow)}
            <div class="flex min-w-0 items-start gap-2">
              <div class="min-w-0 flex-1">
                <h3 class="truncate text-[0.933333rem] font-medium text-foreground">{title}</h3>
                <p class="text-[0.8rem] text-muted-foreground">{t("notes.databaseTableSidePanelSubtitle")}</p>
              </div>
              <button
                type="button"
                class="inline-flex h-8 items-center gap-1 rounded-md px-2 text-[0.8rem] hover:bg-accent"
                onclick={() => onSelectPage(selectedPanelRow.id)}
              >
                <ExternalLink class="size-3.5" aria-hidden="true" />
                <span>{t("notes.databaseTableOpenFullPage")}</span>
              </button>
            </div>
            <dl class="mt-3 grid gap-2 @lg:grid-cols-2">
              {#each columns as column (column.id)}
                <div class="min-w-0 rounded-sm bg-muted/40 p-2">
                  <dt class="truncate text-[0.733333rem] text-muted-foreground">{column.name}</dt>
                  <dd class="mt-0.5 min-w-0 truncate text-[0.866667rem] text-foreground">
                    {notesDatabaseListRowText(selectedPanelRow, column) || t("notes.databaseTableEmptyCell")}
                  </dd>
                </div>
              {/each}
            </dl>
          {:else}
            <p class="text-[0.8rem] text-muted-foreground">{t("notes.databaseTableSidePanelEmpty")}</p>
          {/if}
        </aside>
      {/if}
    </div>
  {/if}

</section>
