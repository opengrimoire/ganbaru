<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { untrack, type Snippet } from "svelte";
  import { databaseResource, notesDatabaseSession, rememberDatabaseScroll } from "$lib/notes/database/session.svelte";
  import NotesDatabaseOptionBadge from "./NotesDatabaseOptionBadge.svelte";
  import NotesDatabasePropertyValue from "./NotesDatabasePropertyValue.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import { COLLECTION_VIEW_SETTINGS_PANEL_WIDTH } from "$lib/components/collections/collection-panel-width";
  import CollectionBoard from "$lib/components/collections/CollectionBoard.svelte";
  import CollectionCard from "$lib/components/collections/CollectionCard.svelte";
  import CollectionQuickAdd from "$lib/components/collections/CollectionQuickAdd.svelte";
  import { formatList, formatNumber } from "$lib/i18n/formatters";
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
    getNotesDataSourceBoardView,
    moveNotesDataSourceBoardRow,
    trashNotesPage,
    updateNotesDataSourceBoardView,
  } from "$lib/api/notes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseBoardCanMoveCards,
    notesDatabaseBoardCardText,
    notesDatabaseBoardCardTitle,
    notesDatabaseBoardColumns,
    notesDatabaseBoardConfigurationFromView,
    notesDatabaseBoardFiltersFromView,
    notesDatabaseBoardGroupableColumns,
    notesDatabaseBoardSortsFromView,
    notesDatabaseBoardUpdate,
    notesDatabaseBoardVisibleColumns,
  } from "$lib/notes/database/board";
  import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { mergeNotesDatabaseBoardWindow } from "$lib/notes/database/view-window";
  import NotesDatabaseWindowSentinel from "./NotesDatabaseWindowSentinel.svelte";
  import type {
    NotesDatabaseBoardConfiguration,
    NotesDatabaseBoardRowOpenMode,
    NotesDatabaseTableFilter,
    NotesDatabaseTableSort,
    NotesDatabaseViewScope,
    NotesDataSourceBoardGroup,
    NotesDataSourceBoardView,
    NotesPage,
  } from "$lib/notes/types";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
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

  let board = $state<NotesDataSourceBoardView | null>(untrack(() => notesDatabaseSession.read(databaseResource("board", dataSourceId, viewScope()))));
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

  const columns = $derived(board ? notesDatabaseBoardColumns(board.data_source, board.view) : []);
  const configuration = $derived(
    board ? notesDatabaseBoardConfigurationFromView(board.view) : defaultConfiguration(),
  );
  const groupableColumns = $derived(notesDatabaseBoardGroupableColumns(columns));
  const groupColumn = $derived(
    configuration.group_property_id
      ? groupableColumns.find((column) => column.id === configuration.group_property_id) ?? null
      : null,
  );
  const visibleColumns = $derived(notesDatabaseBoardVisibleColumns(columns, configuration));
  const filters = $derived(board ? notesDatabaseBoardFiltersFromView(board.view) : []);
  const sorts = $derived(board ? notesDatabaseBoardSortsFromView(board.view) : []);
  const visibleGroups = $derived(board ? board.groups.filter((group) => !group.hidden) : []);
  const hiddenGroups = $derived(board ? board.groups.filter((group) => group.hidden) : []);
  const cardCount = $derived(board?.total_row_count ?? 0);
  const selectedPanelRow = $derived(findBoardRow(selectedPanelRowId));
  const canMoveCards = $derived(notesDatabaseBoardCanMoveCards(groupColumn));

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (mutating) return;
    const signature = `${dataSourceId}:${databaseId ?? ""}:${viewId ?? ""}:${reloadKey}:${revision}`;
    if (signature === lastLoadSignature) return;
    const force = reloadKey !== lastReloadKey;
    lastReloadKey = reloadKey;
    lastLoadSignature = signature;
    void loadBoard(force);
  });

  function viewScope(): NotesDatabaseViewScope {
    return { databaseId, viewId };
  }

  $effect(() => { if (board || error) onReady(); });

  async function loadBoard(force = true): Promise<NotesDataSourceBoardView | null> {
    const currentRequest = ++requestId;
    loading = !board;
    loadingMore = false;
    error = null;
    try {
      const sourceId = dataSourceId;
      const scope = viewScope();
      const resource = databaseResource("board", sourceId, scope);
      const loaded = await notesDatabaseSession.load(resource, () => getNotesDataSourceBoardView(sourceId, scope), force);
      if (currentRequest !== requestId) return null;
      board = loaded;
      if (selectedPanelRowId && !hasBoardRow(selectedPanelRowId, loaded.groups)) {
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
  async function retryBoard(): Promise<void> {
    await loadBoard();
    if (error) throw new Error(error);
  }

  async function loadMoreBoard(): Promise<void> {
    const current = board;
    if (!current?.has_more || !current.next_cursor || loadingMore) return;
    const currentRequest = requestId;
    const revision = notesDatabaseSession.revision;
    loadingMore = true;
    try {
      const loaded = await getNotesDataSourceBoardView(dataSourceId, viewScope(), {
        start_cursor: current.next_cursor,
      });
      if (currentRequest !== requestId || board !== current || revision !== notesDatabaseSession.revision) return;
      board = mergeNotesDatabaseBoardWindow(current, loaded);
      notesDatabaseSession.write(databaseResource("board", dataSourceId, viewScope()), board);
    } catch (caught) {
      if (currentRequest === requestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (currentRequest === requestId) loadingMore = false;
    }
  }

  async function persistBoard(
    nextConfiguration: NotesDatabaseBoardConfiguration,
    nextVisibleColumns = visibleColumns,
    nextFilters = filters,
    nextSorts = sorts,
  ): Promise<void> {
    if (editingLocked || mutating || !board) return;
    const resource = databaseResource("board", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      board = await updateNotesDataSourceBoardView(
        dataSourceId,
        notesDatabaseBoardUpdate(
          nextConfiguration,
          board.groups,
          nextVisibleColumns,
          nextFilters,
          nextSorts,
        ),
        viewScope(),
      );
      notesDatabaseSession.write(resource, board, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Persist filter edits without replacing the current layout configuration. */
  function saveFilters(nextFilters: NotesDatabaseTableFilter[]): void {
    void persistBoard(configuration, visibleColumns, nextFilters, sorts);
  }

  /** Persist sort edits without replacing the current layout configuration. */
  function saveSorts(nextSorts: NotesDatabaseTableSort[]): void {
    void persistBoard(configuration, visibleColumns, filters, nextSorts);
  }

  async function createCard(group: NotesDataSourceBoardGroup, title: string): Promise<boolean> {
    mutating = true;
    error = null;
    let created = false;
    try {
      const loaded = await createNotesDataSourceRowPage(dataSourceId, {
        id: crypto.randomUUID(), first_block_id: crypto.randomUUID(), title,
      });
      created = true;
      if (canMoveCards) await moveNotesDataSourceBoardRow(dataSourceId, { page_id: loaded.page.id, group_id: group.id }, viewScope());
      await loadBoard();
    } catch (caught) {
      if (created) await loadBoard();
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
    return created;
  }

  async function moveCardToGroup(row: NotesPage, group: NotesDataSourceBoardGroup): Promise<void> {
    if (!canMoveCards || mutating) return;
    const resource = databaseResource("board", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      board = await moveNotesDataSourceBoardRow(dataSourceId, { page_id: row.id, group_id: group.id }, viewScope());
      notesDatabaseSession.write(resource, board, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  async function duplicateCard(row: NotesPage): Promise<void> {
    mutating = true;
    error = null;
    try {
      const loaded = await duplicateNotesPage(row.id, {});
      selectedPanelRowId = loaded.page.id;
      await loadBoard();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  async function trashCard(row: NotesPage): Promise<void> {
    mutating = true;
    error = null;
    try {
      await trashNotesPage(row.id, true);
      if (selectedPanelRowId === row.id) selectedPanelRowId = null;
      await loadBoard();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  function updateGroupProperty(propertyId: string): void {
    const nextConfiguration = {
      ...configuration,
      group_property_id: propertyId || null,
      group_order: [],
      hidden_group_ids: [],
    };
    const nextVisibleColumns = notesDatabaseBoardVisibleColumns(columns, nextConfiguration);
    void persistBoard(nextConfiguration, nextVisibleColumns);
  }

  function updateRowOpenMode(mode: NotesDatabaseBoardRowOpenMode): void {
    void persistBoard({ ...configuration, row_open_mode: mode });
  }

  function updateCardProperty(columnId: string, visible: boolean): void {
    const byId = new Map(columns.map((column) => [column.id, column]));
    const nextIds = visible
      ? [...configuration.visible_property_ids, columnId]
      : configuration.visible_property_ids.filter((id) => id !== columnId);
    const nextVisibleColumns = nextIds
      .map((id) => byId.get(id))
      .filter((column): column is NotesDatabaseTableColumn =>
        column !== undefined && column.type !== "title" && column.id !== configuration.group_property_id
      );
    void persistBoard({ ...configuration, visible_property_ids: nextIds }, nextVisibleColumns);
  }

  function updateGroupHidden(group: NotesDataSourceBoardGroup, hidden: boolean): void {
    const hiddenIds = hidden
      ? [...configuration.hidden_group_ids, group.id]
      : configuration.hidden_group_ids.filter((id) => id !== group.id);
    void persistBoard({ ...configuration, hidden_group_ids: Array.from(new Set(hiddenIds)) });
  }

  function openCard(row: NotesPage): void {
    if (configuration.row_open_mode === "side_panel") {
      selectedPanelRowId = row.id;
      return;
    }
    onSelectPage(row.id);
  }

  function rowTitle(row: NotesPage): string {
    return notesDatabaseBoardCardTitle(row, columns, t("notes.untitled"));
  }

  function findBoardRow(rowId: string | null): NotesPage | null {
    if (!rowId || !board) return null;
    for (const group of board.groups) {
      const row = group.rows.find((item) => item.id === rowId);
      if (row) return row;
    }
    return null;
  }

  function hasBoardRow(rowId: string, groups: NotesDataSourceBoardGroup[]): boolean {
    return groups.some((group) => group.rows.some((row) => row.id === rowId));
  }

  function defaultConfiguration(): NotesDatabaseBoardConfiguration {
    return {
      group_property_id: null,
      group_order: [],
      hidden_group_ids: [],
      visible_property_ids: [],
      row_open_mode: "full_page",
    };
  }
</script>
{#snippet viewControls()}
  <span class="sr-only" role="status">
    {#if loading}
      {t("notes.databaseBoardLoading")}
    {:else if error}
      {t("notes.databaseBoardFailed", error)}
    {:else}
      {t("notes.databaseBoardCardsCount", cardCount)}
    {/if}
  </span>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseLayout")} icon={PanelsTopLeft} summary={t("notes.databaseViewBoard")}>
    <CollectionMenuSelect label={t("notes.databaseTableOpenMode")} icon={ExternalLink} value={configuration.row_open_mode} disabled={editingLocked || loading || mutating || !board}
      options={[{ value: "full_page", label: t("notes.databaseTableOpenFullPage") }, { value: "side_panel", label: t("notes.databaseTableOpenSidePanel") }]}
      onChange={updateRowOpenMode} />
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseGroup")} kind="group" summary={groupColumn?.name ?? t("common.none")}>
    <CollectionMenuItem label={t("notes.databaseBoardNoGroupProperty")} checked={!configuration.group_property_id} disabled={editingLocked || loading || mutating || !board} onclick={() => updateGroupProperty("")} />
    {#each groupableColumns as column (column.id)}
      <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={configuration.group_property_id === column.id}
        disabled={editingLocked || loading || mutating || !board} onclick={() => updateGroupProperty(column.id)} />
    {/each}
  </CollectionMenu>
{/snippet}

{#snippet propertyControls()}
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableColumns")} kind="properties" summary={formatNumber(localization.locale, visibleColumns.length)}>
    {#each columns.filter((column) => column.type !== "title" && column.id !== configuration.group_property_id) as column (column.id)}
      {@const visible = visibleColumns.some((visibleColumn) => visibleColumn.id === column.id)}
      <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={visible} disabled={mutating || editingLocked}
        onclick={() => updateCardProperty(column.id, !visible)} />
    {/each}
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableSorts")} kind="sort" activeCount={sorts.length} summary={formatList(localization.locale, sorts.map((sort) => columns.find((column) => column.id === sort.property_id)?.name ?? ""))}>
    <NotesDatabaseSortControls properties={columns} {sorts} pending={mutating || editingLocked} onChange={saveSorts} />
  </CollectionMenu>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableFilters")} kind="filter" activeCount={notesDatabaseFilterCount(filters)} summary={notesDatabaseFilterCount(filters) ? formatNumber(localization.locale, notesDatabaseFilterCount(filters)) : ""}>
    <NotesDatabaseFilterControls properties={columns} {filters} pending={mutating || editingLocked} onChange={saveFilters} />
  </CollectionMenu>
  {#if hiddenGroups.length > 0}
    <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseBoardHiddenGroups")} icon={EyeOff} summary={formatNumber(localization.locale, hiddenGroups.length)}>
      {#each hiddenGroups as group (group.id)}
        <CollectionMenuItem icon={Eye} label={t("notes.databaseBoardShowGroup", group.name)} disabled={mutating || editingLocked} onclick={() => updateGroupHidden(group, false)} />
      {/each}
    </CollectionMenu>
  {/if}
{/snippet}

<section class="space-y-3 pt-2" aria-label={t("notes.databaseBoardTitle")}>

  <NotesDatabaseQueryBar properties={columns} {filters} {sorts} pending={mutating || editingLocked} onFiltersChange={saveFilters} onSortsChange={saveSorts} />
  {#if error && board}<Notice class="text-[0.8rem]" message={error} />{/if}
  {#if settingsOpen}
    <CollectionSettings label={t("notes.databaseViewSettings")} anchor={settingsAnchor} preferredWidth={COLLECTION_VIEW_SETTINGS_PANEL_WIDTH} showHeader={false} onClose={onCloseSettings}>
      {@render settingsHeader?.()}
      {@render viewControls()}
      {#if board}{@render propertyControls()}{/if}
      <NotesDatabaseSettingsFooter reloadLabel={t("notes.databaseBoardReload")} {editingLocked} reloadDisabled={loading || mutating}
        onEditProperties={() => { onCloseSettings(); onEditProperties(); }} onReload={() => { void loadBoard(); }} />
    </CollectionSettings>
  {/if}

  {#if !board && error}
    <div class="min-h-32 text-[0.8rem]"><Notice layout="fill" message={t("notes.databaseBoardFailed", error)} copy={error} onRetry={retryBoard} /></div>
  {:else if !board}<NotesLoadingSkeleton kind="board" />{/if}
  {#if board}
    <div use:rememberDatabaseScroll={databaseResource("board", dataSourceId, viewScope()).key} class="grid gap-2 @container">
      <CollectionBoard groups={visibleGroups} items={(group) => group.rows} label={(group) => group.name}
        emptyLabel={t("notes.databaseRowsEmpty")} dragLabel={(row) => t("notes.databaseBoardDragCard", rowTitle(row))}
        canMove={() => canMoveCards} disabled={mutating} onMove={moveCardToGroup}>
        {#snippet header(group)}
          <div class="min-w-0 flex-1"><NotesDatabaseOptionBadge label={group.name} color={group.color} /></div>
          <span class="text-[0.8rem] tabular-nums text-muted-foreground">{formatNumber(localization.locale, board?.group_counts[group.id] ?? group.rows.length)}</span>
          <CollectionMenu kind="actions" iconOnly showHeader={false} label={group.name}>
            <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" disabled={mutating || editingLocked} onclick={() => updateGroupHidden(group, true)}><EyeOff class="size-4" />{t("notes.databaseBoardHideGroup", group.name)}</button>
          </CollectionMenu>
        {/snippet}
        {#snippet card(row, group, dragHandle)}
          {@const title = rowTitle(row)}
          <CollectionCard {title} onOpen={() => openCard(row)}>
            {#snippet leading()}{@render dragHandle()}{/snippet}
            {#snippet actions()}
              <CollectionMenu kind="actions" iconOnly showHeader={false} label={t("notes.databaseTableRowActions")}>
                <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" onclick={() => openCard(row)}><FileText class="size-4" />{t("notes.databaseRowsOpen", title)}</button>
                {#if canMoveCards}
                  {#each visibleGroups.filter((entry) => entry.id !== group.id) as destination (destination.id)}
                    <button type="button" class="min-h-9 w-full rounded-md px-2 text-left hover:bg-accent" disabled={mutating} onclick={() => { void moveCardToGroup(row, destination); }}>{t("notes.databaseBoardMoveCard", destination.name)}</button>
                  {/each}
                {/if}
                <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" disabled={mutating} onclick={() => { void duplicateCard(row); }}><Copy class="size-4" />{t("notes.databaseRowsDuplicate", title)}</button>
                <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left text-destructive hover:bg-destructive/10" disabled={mutating} onclick={() => { void trashCard(row); }}><Trash2 class="size-4" />{t("notes.databaseRowsTrash", title)}</button>
              </CollectionMenu>
            {/snippet}
            {#if visibleColumns.length > 0}
              <dl class="grid gap-1.5">
                {#each visibleColumns as column (column.id)}
                  {@const text = notesDatabaseBoardCardText(row, column)}
                  {#if text}<div class="flex min-w-0 items-baseline gap-2"><dt class="max-w-24 truncate text-[0.733333rem] text-muted-foreground">{column.name}</dt><dd class="min-w-0 flex-1"><NotesDatabasePropertyValue {row} {column} /></dd></div>{/if}
                {/each}
              </dl>
            {/if}
          </CollectionCard>
        {/snippet}
        {#snippet footer(group)}
          <CollectionQuickAdd label={t("notes.databaseBoardAddCard")} disabled={loading || mutating} onSubmit={(title) => createCard(group, title)} />
        {/snippet}
      </CollectionBoard>

      <NotesDatabaseWindowSentinel hasMore={board.has_more} loading={loadingMore} onLoad={loadMoreBoard} />

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
                    {notesDatabaseBoardCardText(selectedPanelRow, column) || t("notes.databaseTableEmptyCell")}
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
