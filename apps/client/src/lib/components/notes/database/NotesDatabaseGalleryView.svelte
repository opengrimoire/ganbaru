<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { untrack, type Snippet } from "svelte";
  import { databaseResource, notesDatabaseSession } from "$lib/notes/database/session.svelte";
  import NotesDatabasePropertyValue from "./NotesDatabasePropertyValue.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import { COLLECTION_VIEW_SETTINGS_PANEL_WIDTH } from "$lib/components/collections/collection-panel-width";
  import CollectionCard from "$lib/components/collections/CollectionCard.svelte";
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
    duplicateNotesPage,
    getNotesDataSourceGalleryView,
    trashNotesPage,
    updateNotesDataSourceGalleryView,
  } from "$lib/api/notes";
  import { formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseGalleryCardCover,
    notesDatabaseGalleryCardText,
    notesDatabaseGalleryCardTitle,
    notesDatabaseGalleryColumns,
    notesDatabaseGalleryConfigurationFromView,
    notesDatabaseGalleryCoverColumns,
    notesDatabaseGalleryFiltersFromView,
    notesDatabaseGallerySortsFromView,
    notesDatabaseGalleryUpdate,
    notesDatabaseGalleryVisibleColumns,
  } from "$lib/notes/database/gallery";
  import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { mergeNotesDatabaseRows } from "$lib/notes/database/view-window";
  import type {
    NotesDatabaseGalleryCardSize,
    NotesDatabaseGalleryConfiguration,
    NotesDatabaseGalleryCoverSource,
    NotesDatabaseGalleryRowOpenMode,
    NotesDatabaseTableFilter,
    NotesDatabaseTableSort,
    NotesDatabaseViewScope,
    NotesDataSourceGalleryView,
    NotesPage,
  } from "$lib/notes/types";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Crop from "@lucide/svelte/icons/crop";
  import FileText from "@lucide/svelte/icons/file-text";
  import ImageIcon from "@lucide/svelte/icons/image";
  import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Scaling from "@lucide/svelte/icons/scaling";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash";
  import NotesPageCover from "$lib/components/notes/pages/NotesPageCover.svelte";
  import NotesDatabaseWindowSentinel from "./NotesDatabaseWindowSentinel.svelte";

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

  let gallery = $state<NotesDataSourceGalleryView | null>(untrack(() => notesDatabaseSession.read(databaseResource("gallery", dataSourceId, viewScope()))));
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

  const columns = $derived(gallery ? notesDatabaseGalleryColumns(gallery.data_source, gallery.view) : []);
  const configuration = $derived(
    gallery ? notesDatabaseGalleryConfigurationFromView(gallery.view) : defaultConfiguration(),
  );
  const visibleColumns = $derived(notesDatabaseGalleryVisibleColumns(columns, configuration));
  const coverColumns = $derived(notesDatabaseGalleryCoverColumns(columns));
  const filters = $derived(gallery ? notesDatabaseGalleryFiltersFromView(gallery.view) : []);
  const sorts = $derived(gallery ? notesDatabaseGallerySortsFromView(gallery.view) : []);
  const selectedPanelRow = $derived(
    gallery?.rows.find((row) => row.id === selectedPanelRowId) ?? null,
  );
  const gridStyle = $derived(`grid-template-columns: repeat(auto-fill, minmax(min(100%, ${cardMinWidthPx()}px), 1fr));`);
  const previewStyle = $derived(`height: ${previewHeightPx()}px;`);

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (mutating) return;
    const signature = `${dataSourceId}:${databaseId ?? ""}:${viewId ?? ""}:${reloadKey}:${revision}`;
    if (signature === lastLoadSignature) return;
    const force = reloadKey !== lastReloadKey;
    lastReloadKey = reloadKey;
    lastLoadSignature = signature;
    void loadGallery(force);
  });

  function viewScope(): NotesDatabaseViewScope {
    return { databaseId, viewId };
  }

  $effect(() => { if (gallery || error) onReady(); });

  async function loadGallery(force = true): Promise<NotesDataSourceGalleryView | null> {
    const currentRequest = ++requestId;
    loading = !gallery;
    loadingMore = false;
    error = null;
    try {
      const sourceId = dataSourceId;
      const scope = viewScope();
      const resource = databaseResource("gallery", sourceId, scope);
      const loaded = await notesDatabaseSession.load(resource, () => getNotesDataSourceGalleryView(sourceId, scope), force);
      if (currentRequest !== requestId) return null;
      gallery = loaded;
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
  async function retryGallery(): Promise<void> {
    await loadGallery();
    if (error) throw new Error(error);
  }

  async function loadMoreGallery(): Promise<void> {
    const current = gallery;
    if (!current?.has_more || !current.next_cursor || loadingMore) return;
    const currentRequest = requestId;
    const revision = notesDatabaseSession.revision;
    loadingMore = true;
    try {
      const loaded = await getNotesDataSourceGalleryView(dataSourceId, viewScope(), {
        start_cursor: current.next_cursor,
      });
      if (currentRequest !== requestId || gallery !== current || revision !== notesDatabaseSession.revision) return;
      gallery = { ...loaded, rows: mergeNotesDatabaseRows(current.rows, loaded.rows) };
      notesDatabaseSession.write(databaseResource("gallery", dataSourceId, viewScope()), gallery);
    } catch (caught) {
      if (currentRequest === requestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (currentRequest === requestId) loadingMore = false;
    }
  }

  async function persistGallery(
    nextConfiguration: NotesDatabaseGalleryConfiguration,
    nextVisibleColumns = visibleColumns,
    nextFilters = filters,
    nextSorts = sorts,
  ): Promise<void> {
    if (editingLocked || mutating || !gallery) return;
    const resource = databaseResource("gallery", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      gallery = await updateNotesDataSourceGalleryView(
        dataSourceId,
        notesDatabaseGalleryUpdate(nextConfiguration, nextVisibleColumns, nextFilters, nextSorts),
        viewScope(),
      );
      notesDatabaseSession.write(resource, gallery, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Persist filter edits without replacing the current layout configuration. */
  function saveFilters(nextFilters: NotesDatabaseTableFilter[]): void {
    void persistGallery(configuration, visibleColumns, nextFilters, sorts);
  }

  /** Persist sort edits without replacing the current layout configuration. */
  function saveSorts(nextSorts: NotesDatabaseTableSort[]): void {
    void persistGallery(configuration, visibleColumns, filters, nextSorts);
  }

  async function duplicateCard(row: NotesPage): Promise<void> {
    mutating = true;
    error = null;
    try {
      const loaded = await duplicateNotesPage(row.id, {});
      selectedPanelRowId = loaded.page.id;
      await loadGallery();
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
      await loadGallery();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  function updateCoverSource(coverSource: NotesDatabaseGalleryCoverSource): void {
    const firstCoverColumn = coverColumns[0];
    void persistGallery({
      ...configuration,
      cover_source: coverSource,
      cover_property_id: coverSource === "files_property" ? firstCoverColumn?.id ?? null : null,
    });
  }

  function updateCoverProperty(propertyId: string): void {
    void persistGallery({ ...configuration, cover_property_id: propertyId || null });
  }

  function updateCardSize(cardSize: NotesDatabaseGalleryCardSize): void {
    void persistGallery({ ...configuration, card_size: cardSize });
  }

  function updateFitImage(fitImage: boolean): void {
    void persistGallery({ ...configuration, fit_image: fitImage });
  }

  function updateRowOpenMode(mode: NotesDatabaseGalleryRowOpenMode): void {
    void persistGallery({ ...configuration, row_open_mode: mode });
  }

  function updateCardProperty(columnId: string, visible: boolean): void {
    const byId = new Map(columns.map((column) => [column.id, column]));
    const nextIds = visible
      ? [...configuration.visible_property_ids, columnId]
      : configuration.visible_property_ids.filter((id) => id !== columnId);
    const nextVisibleColumns = nextIds
      .map((id) => byId.get(id))
      .filter((column): column is NotesDatabaseTableColumn =>
        column !== undefined && column.type !== "title"
      );
    void persistGallery({ ...configuration, visible_property_ids: nextIds }, nextVisibleColumns);
  }

  function openCard(row: NotesPage): void {
    if (configuration.row_open_mode === "side_panel") {
      selectedPanelRowId = row.id;
      return;
    }
    onSelectPage(row.id);
  }

  function rowTitle(row: NotesPage): string {
    return notesDatabaseGalleryCardTitle(row, columns, t("notes.untitled"));
  }

  function cardMinWidthPx(): number {
    if (configuration.card_size === "small") return 176;
    if (configuration.card_size === "large") return 304;
    return 240;
  }

  function previewHeightPx(): number {
    if (configuration.card_size === "small") return 104;
    if (configuration.card_size === "large") return 192;
    return 144;
  }

  function defaultConfiguration(): NotesDatabaseGalleryConfiguration {
    return {
      cover_source: "page_cover",
      cover_property_id: null,
      visible_property_ids: [],
      card_size: "medium",
      fit_image: false,
      row_open_mode: "full_page",
    };
  }
</script>
{#snippet viewControls()}
  <span class="sr-only" role="status">
    {#if loading}
      {t("notes.databaseGalleryLoading")}
    {:else if error}
      {t("notes.databaseGalleryFailed", error)}
    {:else}
      {t("notes.databaseGalleryCardsCount", gallery?.rows.length ?? 0)}
    {/if}
  </span>
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseLayout")} icon={PanelsTopLeft} summary={t("notes.databaseViewGallery")}>
    <CollectionMenuSelect label={t("notes.databaseGalleryPreview")} icon={ImageIcon} value={configuration.cover_source} disabled={editingLocked || loading || mutating || !gallery}
      options={[{ value: "page_cover", label: t("notes.databaseGalleryPreviewPageCover") }, { value: "files_property", label: t("notes.databaseGalleryPreviewFilesProperty") }, { value: "none", label: t("notes.databaseGalleryPreviewNone") }]}
      onChange={updateCoverSource} />
    {#if configuration.cover_source === "files_property"}
      <CollectionMenuSelect label={t("notes.databaseGalleryCoverProperty")} icon={Paperclip} value={configuration.cover_property_id ?? ""} disabled={editingLocked || loading || mutating || !gallery || coverColumns.length === 0}
        options={[{ value: "", label: t("notes.databaseTableEmptyCell") }, ...coverColumns.map((column) => ({ value: column.id, label: column.name }))]}
        onChange={updateCoverProperty} />
    {/if}
    <CollectionMenuSelect label={t("notes.databaseGalleryCardSize")} icon={Scaling} value={configuration.card_size} disabled={editingLocked || loading || mutating || !gallery}
      options={[{ value: "small", label: t("notes.databaseGalleryCardSizeSmall") }, { value: "medium", label: t("notes.databaseGalleryCardSizeMedium") }, { value: "large", label: t("notes.databaseGalleryCardSizeLarge") }]}
      onChange={updateCardSize} />
    <CollectionMenuItem icon={Crop} label={t("notes.databaseGalleryFitImage")} checked={configuration.fit_image}
      disabled={editingLocked || loading || mutating || !gallery || configuration.cover_source === "none"} onclick={() => updateFitImage(!configuration.fit_image)} />
    <CollectionMenuSelect label={t("notes.databaseTableOpenMode")} icon={ExternalLink} value={configuration.row_open_mode} disabled={editingLocked || loading || mutating || !gallery}
      options={[{ value: "full_page", label: t("notes.databaseTableOpenFullPage") }, { value: "side_panel", label: t("notes.databaseTableOpenSidePanel") }]}
      onChange={updateRowOpenMode} />
  </CollectionMenu>
{/snippet}

{#snippet propertyControls()}
  <CollectionMenu fullWidth disabled={editingLocked} label={t("notes.databaseTableColumns")} kind="properties" summary={formatNumber(localization.locale, visibleColumns.length)}>
    {#each columns.filter((column) => column.type !== "title") as column (column.id)}
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
{/snippet}

<section class="space-y-3 pt-2" aria-label={t("notes.databaseGalleryTitle")}>

  <NotesDatabaseQueryBar properties={columns} {filters} {sorts} pending={mutating || editingLocked} onFiltersChange={saveFilters} onSortsChange={saveSorts} />
  {#if error && gallery}<Notice class="text-[0.8rem]" message={error} />{/if}
  {#if settingsOpen}
    <CollectionSettings label={t("notes.databaseViewSettings")} anchor={settingsAnchor} preferredWidth={COLLECTION_VIEW_SETTINGS_PANEL_WIDTH} showHeader={false} onClose={onCloseSettings}>
      {@render settingsHeader?.()}
      {@render viewControls()}
      {#if gallery}{@render propertyControls()}{/if}
      <NotesDatabaseSettingsFooter reloadLabel={t("notes.databaseGalleryReload")} {editingLocked} reloadDisabled={loading || mutating}
        onEditProperties={() => { onCloseSettings(); onEditProperties(); }} onReload={() => { void loadGallery(); }} />
    </CollectionSettings>
  {/if}

  {#if !gallery && error}
    <div class="min-h-32 text-[0.8rem]"><Notice layout="fill" message={t("notes.databaseGalleryFailed", error)} copy={error} onRetry={retryGallery} /></div>
  {:else if !gallery}<NotesLoadingSkeleton kind="gallery" />{/if}
  {#if gallery}
    <div class="grid gap-2 @container">
      <div class="grid min-w-0 gap-3" style={gridStyle}>
        {#each gallery.rows as row (row.id)}
          {@const title = rowTitle(row)}
          {@const cardCover = notesDatabaseGalleryCardCover(row, configuration)}
          <CollectionCard {title} onOpen={() => openCard(row)}>
            {#snippet cover()}
              {#if configuration.cover_source !== "none"}
              <button
                type="button"
                class="block w-full overflow-hidden border-b border-border bg-muted/40"
                style={previewStyle}
                aria-label={t("notes.databaseRowsOpen", title)}
                onclick={() => openCard(row)}
              >
                <NotesPageCover
                  cover={cardCover}
                  unavailableLabel={t("notes.databaseGalleryPreviewUnavailable")}
                  objectFit={configuration.fit_image ? "contain" : "cover"}
                />
              </button>
              {/if}
            {/snippet}
            {#snippet actions()}
              <CollectionMenu kind="actions" iconOnly showHeader={false} label={t("notes.databaseTableRowActions")}>
                <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" onclick={() => openCard(row)}><FileText class="size-4" />{t("notes.databaseRowsOpen", title)}</button>
                <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-accent" disabled={mutating} onclick={() => { void duplicateCard(row); }}><Copy class="size-4" />{t("notes.databaseRowsDuplicate", title)}</button>
                <button type="button" class="flex min-h-9 w-full items-center gap-2 rounded-md px-2 text-left text-destructive hover:bg-destructive/10" disabled={mutating} onclick={() => { void trashCard(row); }}><Trash2 class="size-4" />{t("notes.databaseRowsTrash", title)}</button>
              </CollectionMenu>
            {/snippet}
              {#if visibleColumns.length > 0}
                <dl class="grid gap-1">
                  {#each visibleColumns as column (column.id)}
                    <div class="min-w-0">
                      <dt class="truncate text-[0.666667rem] text-muted-foreground">{column.name}</dt>
                      <dd class="truncate text-[0.8rem] text-foreground">
                        <NotesDatabasePropertyValue {row} {column} />
                      </dd>
                    </div>
                  {/each}
                </dl>
              {/if}
          </CollectionCard>
        {/each}
      </div>

      {#if gallery.rows.length === 0 && !loading && !error}
        <p class="text-[0.8rem] text-muted-foreground">{t("notes.databaseRowsEmpty")}</p>
      {/if}
      <NotesDatabaseWindowSentinel
        hasMore={gallery.has_more}
        loading={loadingMore}
        onLoad={loadMoreGallery}
      />

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
                    {notesDatabaseGalleryCardText(selectedPanelRow, column) || t("notes.databaseTableEmptyCell")}
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
