<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { NOTES_TEXT_COLORS, notesBlockColorStyle } from "$lib/notes/blocks/color";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
  import CollectionColumnHeader from "$lib/components/collections/CollectionColumnHeader.svelte";
  import { CollectionColumnReorder } from "$lib/components/collections/column-reorder.svelte";
  import { collectionOrderById, collectionPlaceVisibleItem } from "$lib/components/collections/column-reorder";
  import CollectionQuickAdd from "$lib/components/collections/CollectionQuickAdd.svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSelect from "$lib/components/collections/CollectionMenuSelect.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionAddPropertyButton from "$lib/components/collections/CollectionAddPropertyButton.svelte";
  import CollectionPropertyDraft from "$lib/components/collections/CollectionPropertyDraft.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import type { CollectionPropertyDraftPlacement } from "$lib/components/collections/property-draft";
  import CollectionPropertyNameField from "$lib/components/collections/CollectionPropertyNameField.svelte";
  import { COLLECTION_PROPERTY_ICONS } from "$lib/components/collections/property-icons";
  import CollectionPropertyTypeMenu from "$lib/components/collections/CollectionPropertyTypeMenu.svelte";
  import { notesPropertyHasSchemaOptions, notesPropertyKind, notesPropertyTypeOptions } from "./property-kinds";
  import NotesDatabaseFilterControls from "./NotesDatabaseFilterControls.svelte";
  import NotesDatabaseSortControls from "./NotesDatabaseSortControls.svelte";
  import NotesDatabaseSettingsFooter from "./NotesDatabaseSettingsFooter.svelte";
  import NotesDatabaseQueryBar from "./NotesDatabaseQueryBar.svelte";
  import NotesDatabasePropertyEditor from "./NotesDatabasePropertyEditor.svelte";
  import NotesDatabaseRowParentControls from "./NotesDatabaseRowParentControls.svelte";
  import NotesDatabaseBulkPropertyMenu from "./NotesDatabaseBulkPropertyMenu.svelte";
  import { notesDatabaseBulkEditableColumns } from "$lib/notes/database/bulk-edit";
  import CollectionSelectionCheckbox from "$lib/components/collections/CollectionSelectionCheckbox.svelte";
  import CollectionExpandToggle from "$lib/components/collections/CollectionExpandToggle.svelte";
  import CollectionSelectionBar from "$lib/components/collections/CollectionSelectionBar.svelte";
  import CollectionSelectionAction from "$lib/components/collections/CollectionSelectionAction.svelte";
  import { COLLECTION_ROW_INSET, COLLECTION_ROW_INSET_REM, collectionRowExpanded, collectionRowIndent, toggledCollectionRowExpansion, type CollectionRowExpansion } from "$lib/components/collections/row-expansion";
  import { collectionGutterRoom } from "$lib/components/collections/gutter-room";
  import { NOTES_DATABASE_MAX_COLLAPSED_ROWS, NOTES_DATABASE_ROW_MAX_DEPTH, notesDatabaseCollapsedRows, notesDatabaseHierarchyItems, notesDatabaseTopmostRows } from "$lib/notes/database/row-hierarchy";
  import { NOTES_DATABASE_QUERY_MAX_SORTS, notesDatabaseFilterCount, notesDatabaseFilterConditions, notesDatabaseNewFilter, notesDatabaseRowMatchesFilters, notesDatabaseSortsWithColumn } from "$lib/notes/database/query-controls";
  import { onDestroy, tick, untrack, type Snippet } from "svelte";
  import { databaseResource, notesDatabaseSession, rememberDatabaseScroll } from "$lib/notes/database/session.svelte";
  import { createNotesDatabaseRowCreation } from "$lib/notes/database/row-creation.svelte";
  import { NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS, type NotesDatabasePropertyActionRequest } from "$lib/notes/database/data-source-schema";
  import { notesTableColumnPresentation, notesTableCompatibleCalculations, notesTableFrozenOffset, notesTableGroupableColumns, notesTableGroups, notesTableViewSettings } from "$lib/notes/database/table-presentation";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import { COLLECTION_VIEW_SETTINGS_PANEL_WIDTH } from "$lib/components/collections/collection-panel-width";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import {
    clickNotesDataSourceButton,
    createNotesDataSourceTemplateFromRow,
    deleteNotesDataSourceTemplate,
    duplicateNotesPage,
    getNotesDataSourceTableView,
    listNotesDataSourceTemplates,
    trashNotesPage,
    updateNotesDataSourceRowProperty,
    updateNotesDataSourceRowsProperty,
    updateNotesDataSourceRowParent,
    updateNotesDataSourceTableView,
  } from "$lib/api/notes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { formatList, formatNumber } from "$lib/i18n/formatters";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import {
    notesDatabaseTableEditValuesEqual,
    notesDatabaseTableCellEditValue,
    notesDatabaseTableCellText,
    notesDatabaseTableColumnCanEdit,
    notesDatabaseTableColumns,
    notesDatabaseTableColumnWidth,
    notesDatabaseTableConfigurationFromView,
    notesDatabaseTableFiltersFromView,
    notesDatabaseTableSortsFromView,
    notesDatabaseTableUpdate,
    notesDatabaseTableVisibleColumns,
    type NotesDatabaseTableEditValue,
    type NotesDatabaseTableColumn,
  } from "$lib/notes/database/table";
  import { notesDatabaseCellCaret, notesDatabaseCellKeepsKey } from "$lib/notes/database/cell-keyboard";
  import {
    loadNotesEditorPanel,
    retryNotesEditorPanel,
    type LoadedNotesEditorPanel,
  } from "$lib/components/notes/editor-component-registry";
  import type {
    NotesDatabaseTableFilter,
    NotesDatabaseTableCalculation,
    NotesDatabaseTableColorRule,
    NotesDatabaseTableColumnPresentation,
    NotesDatabaseTableConfiguration,
    NotesDatabaseTableRowOpenMode,
    NotesDatabaseTableSort,
    NotesDatabaseViewScope,
    NotesDataSourceTemplate,
    NotesDataSourceTableView,
    NotesDataSourcePropertyType,
    NotesPage,
    NotesColor,
  } from "$lib/notes/types";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Copy from "@lucide/svelte/icons/copy";
  import CornerDownRight from "@lucide/svelte/icons/corner-down-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FileText from "@lucide/svelte/icons/file-text";
  import FileDown from "@lucide/svelte/icons/file-down";
  import FileUp from "@lucide/svelte/icons/file-up";
  import Group from "@lucide/svelte/icons/group";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import Palette from "@lucide/svelte/icons/palette";
  import Rows3 from "@lucide/svelte/icons/rows-3";
  import Star from "@lucide/svelte/icons/star";
  import Plus from "@lucide/svelte/icons/plus";
  import Sigma from "@lucide/svelte/icons/sigma";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import ArrowLeftToLine from "@lucide/svelte/icons/arrow-left-to-line";
  import ArrowRightToLine from "@lucide/svelte/icons/arrow-right-to-line";
  import CalendarCog from "@lucide/svelte/icons/calendar-cog";
  import Clock from "@lucide/svelte/icons/clock";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import TextWrap from "@lucide/svelte/icons/text-wrap";

  let {
    dataSourceId,
    databaseId = null,
    viewId = null,
    onSelectPage,
    onSavingChange = () => {},
    onReady = () => {},
    onAddProperty,
    onEditProperties,
    propertyEditor,
    onLoadPropertyEditor,
    onPropertyAction,
    newRowRequest = 0,
    settingsOpen = false,
    settingsAnchor = null,
    settingsHeader,
    onCloseSettings,
    reloadKey = 0,
    editingLocked = false,
  }: {
    dataSourceId: string;
    databaseId?: string | null;
    viewId?: string | null;
    onSelectPage: (pageId: string) => void;
    onSavingChange?: (saving: boolean) => void;
    onReady?: () => void;
    onAddProperty: (type: NotesDataSourcePropertyType, name: string, icon: string | null) => Promise<void>;
    /** Opens the full source schema editor from view settings. */
    onEditProperties: () => void;
    /** Renders one property's schema fields in its column's Edit property submenu; the submenu is hidden without it. */
    propertyEditor?: Snippet<[string]>;
    /** Loads the schema that {@link propertyEditor} edits when the submenu opens. */
    onLoadPropertyEditor?: (propertyId: string) => void;
    onPropertyAction?: (request: NotesDatabasePropertyActionRequest) => Promise<void>;
    newRowRequest?: number;
    settingsOpen?: boolean;
    settingsAnchor?: HTMLElement | null;
    settingsHeader?: Snippet;
    onCloseSettings: () => void;
    reloadKey?: number;
    editingLocked?: boolean;
  } = $props();

  const localization = getLocalization();
  const preferences = getPreferences();
  const { t } = localization;
  const fileExportAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "notes.file-export",
  );
  const DATE_FORMATS = [
    { value: "locale", label: "notes.databaseTablePresentation.locale" },
    { value: "iso", label: "notes.databaseTablePresentation.iso" },
    { value: "relative", label: "notes.databaseTablePresentation.relative" },
  ] as const satisfies readonly { value: NotesDatabaseTableColumnPresentation["date_format"]; label: string }[];
  const TIME_FORMATS = [
    { value: "locale", label: "notes.databaseTablePresentation.locale" },
    { value: "12_hour", label: "notes.databaseTablePresentation.hour12" },
    { value: "24_hour", label: "notes.databaseTablePresentation.hour24" },
    { value: "hidden", label: "notes.databaseTablePresentation.hiddenTime" },
  ] as const satisfies readonly { value: NotesDatabaseTableColumnPresentation["time_format"]; label: string }[];
  const propertyTypeOptions = $derived(notesPropertyTypeOptions(t));

  let tableRoot: HTMLDivElement | null = $state(null);
  let tableSection: HTMLElement | null = $state(null);
  let tableViewportWidth = $state<number | undefined>();

  $effect(() => {
    const root = tableRoot;
    if (!root) return;
    const content = root.parentElement;
    const measure = () => {
      tableViewportWidth = root.clientWidth || undefined;
      const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
      remPixels = Number.isFinite(rem) ? rem : undefined;
      if (content) gutterRoom = collectionGutterRoom(content).room;
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    const clip = content ? collectionGutterRoom(content).clip : null;
    if (clip) observer.observe(clip);
    return () => observer.disconnect();
  });
  let table = $state<NotesDataSourceTableView | null>(untrack(() => notesDatabaseSession.read(databaseResource("table", dataSourceId, viewScope()))));
  let templates = $state<NotesDataSourceTemplate[]>(untrack(() => notesDatabaseSession.read(databaseResource("templates", dataSourceId)) ?? []));
  let loading = $state(false);
  let loadingMore = $state(false);
  let loadMoreSentinel: HTMLDivElement | null = $state(null);
  let tableRequestId = 0;
  let mutating = $state(false);
  let isEditingCell = $state(false);

  let isViewOpen = true;
  const rowCreation = createNotesDatabaseRowCreation(() => isViewOpen ? loadTable(false) : Promise.resolve(null));
  onDestroy(() => {
    isViewOpen = false;
    rowCreation.flush();
  });

  $effect(() => {
    onSavingChange(mutating || rowCreation.isSaving(dataSourceId));
    return () => onSavingChange(false);
  });
  let error = $state<string | null>(null);
  /** Where the draft property header is shown; a null column is the add control after the last column. */
  let propertyDraft = $state.raw<{ anchor: HTMLElement; trigger: HTMLElement | null; placement: CollectionPropertyDraftPlacement; columnId: string | null; side: "left" | "right" } | null>(null);
  let columnMenuTriggers = $state<Record<string, HTMLButtonElement | undefined>>({});
  let addPropertyButton: HTMLButtonElement | undefined = $state();
  let addRowKey = $state<string | null>(null);
  let columnResize = $state<{ id: string; pointerId: number; startX: number; startWidth: number; width: number } | null>(null);
  let pendingColumnWidth = $state<{ id: string; width: number } | null>(null);
  /** Property order shown while a moved column saves, so headers and rows stay in their new place. */
  let pendingColumnOrder = $state.raw<string[] | null>(null);
  const columnReorder = new CollectionColumnReorder({
    canStart: () => !mutating && !editingLocked && columnResize === null,
    onReorder: (columnId, targetIndex) => { void placeColumn(columnId, targetIndex); },
  });
  onDestroy(() => columnReorder.destroy());

  function startColumnResize(event: PointerEvent, column: NotesDatabaseTableColumn): void {
    if (event.button !== 0 || mutating || editingLocked) return;
    event.preventDefault();
    event.stopPropagation();
    columnResize = { id: column.id, pointerId: event.pointerId, startX: event.clientX, startWidth: column.width, width: column.width };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function moveColumnResize(event: PointerEvent): void {
    if (!columnResize || event.pointerId !== columnResize.pointerId) return;
    const column = columns.find((item) => item.id === columnResize?.id);
    if (!column) return;
    columnResize.width = notesDatabaseTableColumnWidth(column, event.clientX - columnResize.startX);
  }

  function finishColumnResize(event: PointerEvent, commit: boolean): void {
    if (!columnResize || event.pointerId !== columnResize.pointerId) return;
    const gesture = columnResize;
    columnResize = null;
    if (commit && gesture.width !== gesture.startWidth) updateColumnWidth(gesture.id, gesture.width - gesture.startWidth);
  }

  function resizeColumnKey(event: KeyboardEvent, column: NotesDatabaseTableColumn): void {
    if (editingLocked || mutating) return;
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation();
    updateColumnWidth(column.id, (event.key === "ArrowRight" ? 1 : -1) * (event.shiftKey ? 32 : 8));
  }
  let handledNewRowRequest = $state(0);
  let templateName = $state("");
  let templateSourceRowId = $state("");
  let selectedTemplateId = $state("");
  let createTemplateAsDefault = $state(false);
  let selectedPanelRowId = $state<string | null>(null);
  let lastLoadSignature = $state("");
  let lastReloadKey = untrack(() => reloadKey);
  let pendingFocusRowId = $state<string | null>(null);
  let temporarilyExpandedRowIds = $state<string[]>([]);
  let temporarilyExpandedGroupIds = $state<string[]>([]);
  /** Rows without sub-items the user opened in this session to add a first one. */
  let openedRowIds = $state<string[]>([]);
  let selectedRowIds = $state<string[]>([]);
  $effect(() => {
    dataSourceId;
    databaseId;
    viewId;
    temporarilyExpandedGroupIds = [];
    temporarilyExpandedRowIds = [];
    openedRowIds = [];
    selectedRowIds = [];
  });
  let openCsvPanelKind = $state<"database-csv-import" | "database-csv-export" | null>(null);
  let csvPanelLoadState = $state<LazyComponentLoadState<
    "database-csv-import" | "database-csv-export",
    LoadedNotesEditorPanel
  > | null>(null);

  function requestCsvPanel(
    kind: "database-csv-import" | "database-csv-export",
    retry = false,
  ): Promise<void> {
    if (kind === "database-csv-import" && editingLocked) return Promise.resolve();
    openCsvPanelKind = kind;
    if (!retry && csvPanelLoadState?.key === kind) return Promise.resolve();
    const loadingState = beginLazyComponentLoad(csvPanelLoadState, kind);
    csvPanelLoadState = loadingState;
    const request = retry ? retryNotesEditorPanel(kind) : loadNotesEditorPanel(kind);
    return request.then((component) => {
      if (!csvPanelLoadState) return;
      csvPanelLoadState = resolveLazyComponentLoad(
        csvPanelLoadState,
        kind,
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!csvPanelLoadState) return;
      csvPanelLoadState = rejectLazyComponentLoad(
        csvPanelLoadState,
        kind,
        loadingState.requestId,
        error,
      );
      console.error(`load Notes ${kind} panel failed`, error);
      if (retry) throw error;
    });
  }

  function retryCsvPanel(): Promise<void> {
    const kind = csvPanelLoadState?.key;
    return kind ? requestCsvPanel(kind, true) : Promise.resolve();
  }

  const visibleRows = $derived(rowCreation.rowsFor(dataSourceId, table?.rows ?? []));
  const templateSourceRows = $derived(visibleRows.filter((row) => !rowCreation.blocked(row.id)));
  const tableConfiguration = $derived(table ? { ...notesDatabaseTableConfigurationFromView(table.view), ...notesTableViewSettings(table.view) } : null);
  const savedColumns = $derived(table ? notesDatabaseTableColumns(table.data_source, table.view).map((column) => ({ ...column,
    displayFormat: notesTableColumnPresentation(tableConfiguration?.presentation, column.id),
  })) : []);
  const columns = $derived(pendingColumnOrder ? collectionOrderById(savedColumns, pendingColumnOrder) : savedColumns);
  const visibleColumns = $derived(notesDatabaseTableVisibleColumns(columns));
  /** Properties the selection toolbar sets on every selected row; read-only and computed values stay per row. */
  const bulkColumns = $derived(notesDatabaseBulkEditableColumns(visibleColumns));
  const renderedColumns = $derived(visibleColumns.map((column) => ({ ...column,
    width: columnResize?.id === column.id ? columnResize.width : pendingColumnWidth?.id === column.id ? pendingColumnWidth.width : column.width,
  })));
  /** Minimum width of the trailing track, which only holds the add property button (`size-9`). */
  const ADD_PROPERTY_TRACK_MIN = "2.25rem";
  /** Leading tracks for the selection checkbox and the expand chevron (`size-7`), in rem. */
  const SELECTION_TRACK_REM = 1.5;
  const EXPAND_TRACK_REM = 1.75;
  const SELECTION_TRACK = `${SELECTION_TRACK_REM}rem`;
  const EXPAND_TRACK = `${EXPAND_TRACK_REM}rem`;
  /**
   * The selection and expand tracks sit outside the table: row fills and dividers start after them, and the table hangs
   * them into the margin so its content lines up with the database title and toolbar.
   */
  const ROW_GUTTER = `calc(${SELECTION_TRACK} + ${EXPAND_TRACK})`;
  const ROW_CONTENT_START_REM = COLLECTION_ROW_INSET_REM + SELECTION_TRACK_REM + EXPAND_TRACK_REM;
  const ROW_CONTENT_START = `${ROW_CONTENT_START_REM}rem`;
  /** Draws row dividers above sticky frozen cells (`z-20`) and frozen headers (`z-30`). */
  const ROW_DIVIDER_LAYER = 31;
  /** Pixels of one rem, so frozen columns can budget the viewport left after the gutter. */
  let remPixels = $state<number | undefined>();
  /** Margin room left of the table content that the gutter can hang into without being clipped. */
  let gutterRoom = $state(0);
  const frozenViewportWidth = $derived(tableViewportWidth !== undefined && remPixels !== undefined
    ? Math.max(0, tableViewportWidth - ROW_CONTENT_START_REM * remPixels) : tableViewportWidth);
  function frozenOffset(columnId: string): number | null {
    return notesTableFrozenOffset(renderedColumns, tableConfiguration?.presentation?.frozen_property_id, columnId, frozenViewportWidth);
  }
  /** Sticky position of a frozen column, after the gutter. */
  function frozenStyle(columnId: string): string | undefined {
    const offset = frozenOffset(columnId);
    return offset === null ? undefined : `left: calc(${ROW_CONTENT_START} + ${offset}px);`;
  }
  /** While a column is frozen, the gutter sticks too, so scrolled cells pass beneath both instead of over the margin. */
  const gutterFrozen = $derived(renderedColumns.some((column) => frozenOffset(column.id) !== null));
  function gutterCellClass(header: boolean): string {
    return gutterFrozen ? `sticky bg-background ${header ? "z-30" : "z-20"}` : "";
  }
  function gutterCellStyle(track: "selection" | "expand"): string | undefined {
    if (!gutterFrozen) return undefined;
    // The selection cell also covers the row inset, which would otherwise show scrolled cells at the margin edge.
    return track === "selection"
      ? `left: ${COLLECTION_ROW_INSET}; box-shadow: calc(-1 * ${COLLECTION_ROW_INSET}) 0 0 var(--background);`
      : `left: calc(${COLLECTION_ROW_INSET} + ${SELECTION_TRACK});`;
  }
  const gridTemplate = $derived(`${SELECTION_TRACK} ${EXPAND_TRACK} ${renderedColumns.map((column) => `${column.width}px`).join(" ")} minmax(${ADD_PROPERTY_TRACK_MIN}, 1fr)`);
  const filters = $derived(table ? notesDatabaseTableFiltersFromView(table.view) : []);
  const sorts = $derived(table ? notesDatabaseTableSortsFromView(table.view) : []);
  const rowOpenMode = $derived(
    table ? notesDatabaseTableConfigurationFromView(table.view).row_open_mode : "full_page",
  );
  const groups = $derived(tableConfiguration ? notesTableGroups(visibleRows, columns, { ...tableConfiguration,
    collapsed_group_ids: (tableConfiguration.collapsed_group_ids ?? []).filter((id) => !temporarilyExpandedGroupIds.includes(id)),
  }, table?.group_counts ?? {}) : []);
  const rowHierarchy = $derived(rowCreation.hierarchyFor(dataSourceId, table?.row_hierarchy ?? {}));
  const collapsedRowIds = $derived((tableConfiguration?.collapsed_row_ids ?? []).filter((id) => !temporarilyExpandedRowIds.includes(id)));
  const rowExpansion = $derived<CollectionRowExpansion>({ collapsedIds: new Set(collapsedRowIds), openedIds: new Set(openedRowIds) });
  type TableItem = { type: "group"; id: string; group: typeof groups[number] } | { type: "row"; id: string; row: NotesPage; rowIndex: number }
    | { type: "addSubitem"; id: string; parent: NotesPage } | { type: "calculation"; id: string; groupId: string };
  const tableItems = $derived.by((): TableItem[] => {
    let rowIndex = 0;
    const collapsed = collapsedRowIds;
    const hierarchyItems = (rows: readonly NotesPage[], prefix: string): TableItem[] =>
      notesDatabaseHierarchyItems(rows, rowHierarchy, collapsed, rowExpanded).map((item): TableItem => item.type === "row"
        ? { type: "row", id: `${prefix}${item.row.id}`, row: item.row, rowIndex: rowIndex++ }
        : { type: "addSubitem", id: `${prefix}add:${item.parent.id}`, parent: item.parent });
    if (!tableConfiguration?.group_property_id) return hierarchyItems(visibleRows, "");
    return groups.flatMap((group): TableItem[] => [
      { type: "group", id: `group:${group.id}`, group },
      ...(group.collapsed ? [] : hierarchyItems(group.rows, `${group.id}:`)),
      ...(group.collapsed ? [] : [{ type: "calculation" as const, id: `calculation:${group.id}`, groupId: group.id }]),
    ]);
  });
  const renderedRows = $derived(tableItems.flatMap((item) => item.type === "row" ? [item.row] : []));
  const selectedRows = $derived(visibleRows.filter((row) => selectedRowIds.includes(row.id) && !rowCreation.blocked(row.id)));
  const selectableRows = $derived(renderedRows.filter((row) => !rowCreation.blocked(row.id)));
  const allRowsSelected = $derived(selectableRows.length > 0 && selectableRows.every((row) => selectedRowIds.includes(row.id)));
  const selectedPanelRow = $derived(
    visibleRows.find((row) => row.id === selectedPanelRowId) ?? null,
  );
  $effect(() => {
    if (newRowRequest <= handledNewRowRequest) return;
    if (loading || mutating || !table) return;
    while (handledNewRowRequest < newRowRequest) {
      handledNewRowRequest += 1;
      createRow();
    }
  });

  /** Open the draft property header after the last column, or close it when it is already there. */
  function toggleAddPropertyDraft(): void {
    const anchor = addPropertyButton?.parentElement;
    if (propertyDraft?.columnId === null || !anchor) {
      propertyDraft = null;
      return;
    }
    propertyDraft = { anchor, trigger: addPropertyButton ?? null, placement: "start", columnId: null, side: "right" };
  }

  /** Open the draft property header beside a column from its menu. */
  function openInsertPropertyDraft(column: NotesDatabaseTableColumn, side: "left" | "right"): void {
    const trigger = columnMenuTriggers[column.id] ?? null;
    const anchor = trigger?.closest<HTMLElement>("[role=\"columnheader\"]");
    if (!anchor) return;
    propertyDraft = { anchor, trigger, placement: side === "left" ? "before" : "after", columnId: column.id, side };
  }

  /**
   * Create the drafted property where the draft was opened; a blank name lets the schema owner choose a default name.
   * Rejections reach the draft, which keeps the name and icon and shows the message.
   */
  async function createDraftProperty(type: NotesDataSourcePropertyType, name: string, icon: string | null): Promise<boolean> {
    const target = propertyDraft;
    if (!target || mutating || editingLocked) throw new Error(t("notes.databaseSaving"));
    if (target.columnId !== null && !onPropertyAction) throw new Error(t("notes.databaseSaving"));
    mutating = true;
    error = null;
    try {
      if (target.columnId === null) {
        await onAddProperty(type, name, icon);
      } else {
        await onPropertyAction?.({ type: "insert", propertyId: target.columnId, side: target.side, propertyType: type, name, icon });
        await loadTable(false);
      }
      return true;
    } finally {
      mutating = false;
    }
  }

  async function performPropertyAction(request: NotesDatabasePropertyActionRequest): Promise<void> {
    if (mutating || editingLocked || !onPropertyAction) return;
    mutating = true;
    error = null;
    try {
      await onPropertyAction(request);
      await loadTable(false);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Rename a property from its column menu; rejections reach the name field, which keeps the draft and shows them. */
  async function renameProperty(propertyId: string, name: string): Promise<void> {
    if (mutating || editingLocked || !onPropertyAction) throw new Error(t("notes.databaseSaving"));
    mutating = true;
    try {
      await onPropertyAction({ type: "rename", propertyId, name });
      await loadTable(false);
    } finally {
      mutating = false;
    }
  }

  /** Change a property icon from its column menu; rejections reach the name field, which shows them. */
  async function changePropertyIcon(propertyId: string, icon: string | null): Promise<void> {
    if (mutating || editingLocked || !onPropertyAction) throw new Error(t("notes.databaseSaving"));
    mutating = true;
    try {
      await onPropertyAction({ type: "icon", propertyId, icon });
      await loadTable(false);
    } finally {
      mutating = false;
    }
  }

  function propertyInsertionSides(column: NotesDatabaseTableColumn): readonly ("left" | "right")[] {
    return column.type === "title" ? ["right"] : ["left", "right"];
  }

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (mutating || isEditingCell || rowCreation.isSaving(dataSourceId)) return;
    const signature = `${dataSourceId}:${databaseId ?? ""}:${viewId ?? ""}:${reloadKey}:${revision}`;
    if (signature === lastLoadSignature) return;
    const force = reloadKey !== lastReloadKey;
    lastReloadKey = reloadKey;
    lastLoadSignature = signature;
    void loadTable(true, force);
  });

  function viewScope(): NotesDatabaseViewScope {
    return { databaseId, viewId };
  }

  $effect(() => { if (table || error) onReady(); });

  async function loadTable(refreshTemplates = true, force = true): Promise<NotesDataSourceTableView | null> {
    const requestId = ++tableRequestId;
    const settledIds = rowCreation.settledIds(dataSourceId);
    loading = !table;
    loadingMore = false;
    error = null;
    try {
      const sourceId = dataSourceId;
      const scope = viewScope();
      const templateRead = (refreshTemplates
        ? notesDatabaseSession.load(databaseResource("templates", sourceId), () => listNotesDataSourceTemplates(sourceId), force)
        : Promise.resolve(templates)).then(
          (value) => ({ value }),
          (reason: unknown) => ({ reason }),
        );
      const loaded = await notesDatabaseSession.load(databaseResource("table", sourceId, scope), () => getNotesDataSourceTableView(sourceId, scope), force);
      if (requestId !== tableRequestId) return null;
      const focusedId = document.activeElement?.closest<HTMLElement>("[data-database-row-id]")?.dataset.databaseRowId ?? null;
      rowCreation.acceptWindow(dataSourceId, focusedId, settledIds);
      table = loaded;
      if (selectedPanelRowId && !visibleRows.some((row) => row.id === selectedPanelRowId)) {
        selectedPanelRowId = null;
      }
      if (templateSourceRowId && !visibleRows.some((row) => row.id === templateSourceRowId)) {
        templateSourceRowId = "";
      }
      await focusPendingRow();
      const templateResult = await templateRead;
      if (requestId !== tableRequestId) return null;
      if ("reason" in templateResult) throw templateResult.reason;
      const loadedTemplates = templateResult.value;
      templates = loadedTemplates;
      if (selectedTemplateId && !loadedTemplates.some((template) => template.id === selectedTemplateId)) {
        selectedTemplateId = "";
      }
      if (!selectedTemplateId) {
        selectedTemplateId = loadedTemplates.find((template) => template.is_default)?.id ?? "";
      }
      return loaded;
    } catch (caught) {
      if (requestId !== tableRequestId) return null;
      error = caught instanceof Error ? caught.message : String(caught);
      return null;
    } finally {
      if (requestId === tableRequestId) loading = false;
    }
  }

  /** Reloads a table that failed to load, rejecting while it is still unavailable. */
  async function retryTable(): Promise<void> {
    await loadTable();
    if (error) throw new Error(error);
  }

  async function loadMoreTableRows(): Promise<void> {
    const current = table;
    if (!current?.has_more || !current.next_cursor || loadingMore) return;
    const requestId = tableRequestId;
    const revision = notesDatabaseSession.revision;
    loadingMore = true;
    try {
      const loaded = await getNotesDataSourceTableView(dataSourceId, viewScope(), {
        start_cursor: current.next_cursor,
      });
      if (requestId !== tableRequestId || table !== current || revision !== notesDatabaseSession.revision) return;
      const rows = new Map(current.rows.map((row) => [row.id, row]));
      for (const row of loaded.rows) rows.set(row.id, row);
      table = { ...loaded, rows: [...rows.values()], row_hierarchy: { ...current.row_hierarchy, ...loaded.row_hierarchy } };
      notesDatabaseSession.write(databaseResource("table", dataSourceId, viewScope()), table);
    } catch (caught) {
      if (requestId === tableRequestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (requestId === tableRequestId) loadingMore = false;
    }
  }

  $effect(() => {
    const sentinel = loadMoreSentinel;
    if (!sentinel || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void loadMoreTableRows();
    }, { root: sentinel.closest("[data-notes-editor-scroll]") });
    observer.observe(sentinel);
    return () => observer.disconnect();
  });

  async function focusPendingRow(): Promise<void> {
    const rowId = pendingFocusRowId;
    pendingFocusRowId = null;
    if (!rowId) return;
    const rowIndex = renderedRows.findIndex((row) => row.id === rowId);
    if (rowIndex < 0) return;
    await tick();
    focusCell(rowIndex, 0);
  }

  async function persistTable(
    nextColumns: NotesDatabaseTableColumn[],
    nextRowOpenMode: NotesDatabaseTableRowOpenMode,
    nextFilters: NotesDatabaseTableFilter[],
    nextSorts: NotesDatabaseTableSort[],
    nextConfiguration: Partial<NotesDatabaseTableConfiguration> = tableConfiguration ?? {},
  ): Promise<void> {
    if (editingLocked || mutating) return;
    const settledIds = rowCreation.settledIds(dataSourceId);
    const resource = databaseResource("table", dataSourceId, viewScope());
    const epoch = notesDatabaseSession.epoch;
    mutating = true;
    error = null;
    try {
      const loaded = await updateNotesDataSourceTableView(
        dataSourceId,
        notesDatabaseTableUpdate(nextColumns, nextRowOpenMode, nextFilters, nextSorts, nextConfiguration),
        viewScope(),
      );
      const focusedId = document.activeElement?.closest<HTMLElement>("[data-database-row-id]")?.dataset.databaseRowId ?? null;
      rowCreation.acceptWindow(dataSourceId, focusedId, settledIds);
      table = loaded;
      notesDatabaseSession.write(resource, loaded, epoch);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Persist filter edits against the current saved table presentation. */
  function saveFilters(nextFilters: NotesDatabaseTableFilter[]): void {
    void persistTable(columns.map((column) => ({ ...column })), rowOpenMode, nextFilters, sorts);
  }

  /** Persist sort edits against the current saved table presentation. */
  function saveSorts(nextSorts: NotesDatabaseTableSort[]): void {
    void persistTable(columns.map((column) => ({ ...column })), rowOpenMode, filters, nextSorts);
  }

  async function createRow(groupId?: string): Promise<void> {
    if (mutating || loading || !table) return;
    if (groupId && tableConfiguration?.collapsed_group_ids?.includes(groupId)) {
      if (editingLocked) temporarilyExpandedGroupIds = [...temporarilyExpandedGroupIds, groupId];
      else {
        await persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
          collapsed_group_ids: tableConfiguration.collapsed_group_ids.filter((id) => id !== groupId),
        });
        if (error) return;
      }
    }
    const groupColumn = columns.find((column) => column.id === tableConfiguration?.group_property_id);
    const initial: { column: NotesDatabaseTableColumn; value: NotesDatabaseTableEditValue; payload: unknown }[] = [];
    if (groupId && groupColumn) {
      const option = groupColumn.options.find((candidate) => candidate.id === groupId);
      if (groupColumn.type === "checkbox") initial.push({ column: groupColumn, value: groupId === "true", payload: groupId === "true" });
      else if (option) initial.push({ column: groupColumn, value: groupColumn.type === "multi_select" ? [option.name] : option.name, payload: groupColumn.type === "multi_select" ? [option] : option });
      else if (groupId === "__empty__" && ["select", "status", "multi_select", "date"].includes(groupColumn.type)) initial.push({ column: groupColumn, value: groupColumn.type === "multi_select" ? [] : null, payload: groupColumn.type === "multi_select" ? [] : null });
      else if (groupColumn.type === "date" && groupId !== "__empty__") initial.push({ column: groupColumn, value: groupId, payload: { start: groupId, end: null, time_zone: null } });
    }
    const pageId = rowCreation.begin(dataSourceId, selectedTemplateId, initial);
    addRowKey = pageId;
    void tick().then(() => {
      const rowIndex = renderedRows.findIndex((row) => row.id === pageId);
      const titleIndex = visibleColumns.findIndex((column) => column.type === "title");
      if (rowIndex >= 0 && titleIndex >= 0) focusCell(rowIndex, titleIndex);
    });
  }

  /** Title inset of a row at the given depth, capped at half the title column so deep rows keep room for text. */
  function titleIndent(depth: number): string {
    const titleWidth = renderedColumns.find((column) => column.type === "title")?.width;
    return titleWidth === undefined ? collectionRowIndent(depth) : `min(${collectionRowIndent(depth)}, ${titleWidth / 2}px)`;
  }

  function rowHasChildren(row: NotesPage): boolean {
    return (rowHierarchy[row.id]?.child_count ?? 0) > 0;
  }

  function rowExpanded(row: NotesPage): boolean {
    return collectionRowExpanded(row.id, rowHasChildren(row), rowExpansion);
  }

  /**
   * Whether the chevron cannot change the row. Collapsing a row with sub-items saves the view, which a locked view or a
   * full collapse list refuses; a row the saved view collapses can always open for this session.
   */
  function rowExpansionLocked(row: NotesPage): boolean {
    if (mutating) return true;
    const saved = tableConfiguration?.collapsed_row_ids ?? [];
    if (!rowHasChildren(row) || saved.includes(row.id)) return false;
    return editingLocked || saved.length >= NOTES_DATABASE_MAX_COLLAPSED_ROWS;
  }

  /** Rows with sub-items keep their collapse in the saved view; rows without them open only for this session. */
  function toggleRowExpanded(row: NotesPage): void {
    if (rowExpansionLocked(row) || !tableConfiguration) return;
    const next = toggledCollectionRowExpansion(row.id, rowHasChildren(row), rowExpansion);
    openedRowIds = [...next.openedIds];
    if (next.collapsedIds.has(row.id) === rowExpansion.collapsedIds.has(row.id)) return;
    const saved = tableConfiguration.collapsed_row_ids ?? [];
    if (saved.includes(row.id) && temporarilyExpandedRowIds.includes(row.id)) {
      temporarilyExpandedRowIds = temporarilyExpandedRowIds.filter((id) => id !== row.id);
    } else if (saved.includes(row.id) && editingLocked) {
      temporarilyExpandedRowIds = [...temporarilyExpandedRowIds, row.id];
    } else {
      void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
        collapsed_row_ids: notesDatabaseCollapsedRows(saved, row.id),
      });
    }
  }

  function toggleRowSelected(row: NotesPage): void {
    selectedRowIds = selectedRowIds.includes(row.id) ? selectedRowIds.filter((id) => id !== row.id) : [...selectedRowIds, row.id];
  }

  function toggleAllRowsSelected(): void {
    const ids = new Set(selectableRows.map((row) => row.id));
    selectedRowIds = allRowsSelected ? selectedRowIds.filter((id) => !ids.has(id)) : [...new Set([...selectedRowIds, ...ids])];
  }

  async function createSubitem(parent: NotesPage): Promise<void> {
    if (mutating || loading || !table || rowCreation.blocked(parent.id)) return;
    if ((rowHierarchy[parent.id]?.depth ?? 0) >= NOTES_DATABASE_ROW_MAX_DEPTH) return;
    if (collapsedRowIds.includes(parent.id) && editingLocked) {
      temporarilyExpandedRowIds = [...temporarilyExpandedRowIds, parent.id];
    } else if (collapsedRowIds.includes(parent.id) && tableConfiguration?.collapsed_row_ids) {
      await persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
        collapsed_row_ids: tableConfiguration.collapsed_row_ids.filter((id) => id !== parent.id),
      });
      if (error) return;
    }
    const groupColumn = columns.find((column) => column.id === tableConfiguration?.group_property_id);
    const property = groupColumn ? parent.properties[groupColumn.name] : null;
    const payload = property && typeof property === "object" && !Array.isArray(property) && groupColumn
      ? (property as Record<string, unknown>)[groupColumn.type] : null;
    const initial = groupColumn && notesDatabaseTableColumnCanEdit(groupColumn) ? [{ column: groupColumn, value: notesDatabaseTableCellEditValue(parent, groupColumn), payload }] : [];
    const pageId = rowCreation.begin(dataSourceId, "", initial, parent.id);
    addRowKey = pageId;
    await tick();
    const rowIndex = renderedRows.findIndex((row) => row.id === pageId);
    const titleIndex = visibleColumns.findIndex((column) => column.type === "title");
    if (rowIndex >= 0 && titleIndex >= 0) focusCell(rowIndex, titleIndex);
  }

  async function moveSubitems(rows: readonly NotesPage[], parentRowId: string | null): Promise<void> {
    const movable = rows.filter((row) => !rowCreation.blocked(row.id));
    if (mutating || movable.length === 0) return;
    mutating = true;
    error = null;
    try {
      for (const row of movable) {
        await updateNotesDataSourceRowParent(dataSourceId, row.id, parentRowId);
        rowCreation.acceptParent(row.id, parentRowId);
      }
      pendingFocusRowId = movable[0].id;
      await loadTable(false);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally { mutating = false; }
  }

  async function createTemplateFromRow(): Promise<void> {
    const sourceRowId = templateSourceRowId || templateSourceRows[0]?.id || "";
    const name = templateName.trim();
    if (!sourceRowId || !name) return;
    mutating = true;
    error = null;
    try {
      const template = await createNotesDataSourceTemplateFromRow(dataSourceId, {
        id: crypto.randomUUID(),
        source_page_id: sourceRowId,
        name,
        is_default: createTemplateAsDefault,
      });
      templateName = "";
      templateSourceRowId = "";
      createTemplateAsDefault = false;
      selectedTemplateId = template.id;
      templates = await listNotesDataSourceTemplates(dataSourceId);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  async function deleteSelectedTemplate(): Promise<void> {
    const templateId = selectedTemplateId;
    if (!templateId) return;
    mutating = true;
    error = null;
    try {
      await deleteNotesDataSourceTemplate(dataSourceId, templateId);
      selectedTemplateId = "";
      templates = await listNotesDataSourceTemplates(dataSourceId);
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  async function runButton(row: NotesPage, column: NotesDatabaseTableColumn): Promise<void> {
    const title = rowTitle(row);
    const confirmed = column.buttonRequiresConfirmation
      ? window.confirm(t("notes.databaseButtonConfirm", column.name, title))
      : false;
    if (column.buttonRequiresConfirmation && !confirmed) return;
    mutating = true;
    error = null;
    try {
      pendingFocusRowId = row.id;
      await clickNotesDataSourceButton(dataSourceId, row.id, {
        property_id: column.id,
        confirmed,
      });
      await loadTable();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Duplicates each selected subtree once; a row's copy includes its sub-items, so a selected descendant is skipped. */
  async function duplicateRows(rows: readonly NotesPage[]): Promise<void> {
    const sources = notesDatabaseTopmostRows(rows, rowHierarchy);
    if (mutating || sources.length === 0) return;
    mutating = true;
    error = null;
    try {
      for (const row of sources) {
        const loaded = await duplicateNotesPage(row.id, {});
        pendingFocusRowId = loaded.page.id;
      }
      await loadTable();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Trashes the given rows; sub-items that are not given stay and move to the top level. */
  async function trashRows(rows: readonly NotesPage[]): Promise<void> {
    const targets = rows.filter((row) => !rowCreation.blocked(row.id));
    if (mutating || targets.length === 0) return;
    mutating = true;
    error = null;
    try {
      for (const row of targets) {
        await trashNotesPage(row.id, true);
        if (selectedPanelRowId === row.id) selectedPanelRowId = null;
        selectedRowIds = selectedRowIds.filter((id) => id !== row.id);
      }
      await loadTable();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      mutating = false;
    }
  }

  /** Set one property on every selected row in one save, so the rows change together or not at all. */
  async function saveRowsProperty(rows: readonly NotesPage[], column: NotesDatabaseTableColumn, value: NotesDatabaseTableEditValue): Promise<boolean> {
    const targets = rows.filter((row) => !rowCreation.blocked(row.id));
    if (targets.length === 0) return false;
    mutating = true;
    error = null;
    try {
      await updateNotesDataSourceRowsProperty(dataSourceId, targets.map((row) => row.id), { property_id: column.id, value });
      await loadTable();
      return true;
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
      return false;
    } finally {
      mutating = false;
    }
  }

  /**
   * Delete moves the selected rows to the trash while focus is in this table or nowhere, unless a value, a cell editor,
   * or a menu owns the key.
   */
  function trashSelectionKey(event: KeyboardEvent): void {
    if (event.key !== "Delete" || event.defaultPrevented || selectedRows.length === 0 || isEditingCell || mutating) return;
    const target = event.target;
    if (!(target instanceof Element) || !(target === document.body || tableSection?.contains(target)) || target.closest("[data-app-floating-surface]")
      || target.matches("textarea, select, [contenteditable]:not([contenteditable='false']), input:not([type='checkbox']):not([type='radio']):not([type='button'])")) return;
    event.preventDefault();
    void trashRows(selectedRows);
  }

  async function saveCell(
    row: NotesPage,
    column: NotesDatabaseTableColumn,
    value: NotesDatabaseTableEditValue,
  ): Promise<boolean> {
    if (rowCreation.submit(row.id, column, value)) return true;
    const current = notesDatabaseTableCellEditValue(row, column);
    if (notesDatabaseTableEditValuesEqual(current, value)) return true;
    mutating = true;
    error = null;
    try {
      pendingFocusRowId = row.id;
      await updateNotesDataSourceRowProperty(dataSourceId, row.id, {
        property_id: column.id,
        value,
      });
      await loadTable();
      return true;
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
      return false;
    } finally {
      mutating = false;
    }
  }


  function updateColumnVisibility(columnId: string, hidden: boolean): void {
    const nextColumns = columns.map((column) =>
      column.id === columnId ? { ...column, hidden } : { ...column },
    );
    const configuration = { ...tableConfiguration };
    if (hidden && configuration.presentation?.frozen_property_id === columnId) configuration.presentation = { ...configuration.presentation, frozen_property_id: null };
    void persistTable(nextColumns, rowOpenMode, filters, sorts, configuration);
  }

  /** Save a view-owned property presentation change. */
  function updateColumnPresentation(columnId: string, change: Partial<NotesDatabaseTableColumnPresentation>): void {
    const presentation = tableConfiguration?.presentation ?? { frozen_property_id: null, columns: {} };
    void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
      presentation: { ...presentation, columns: { ...presentation.columns, [columnId]: { ...notesTableColumnPresentation(presentation, columnId), ...change } } },
    });
  }

  function freezeThrough(columnId: string | null): void {
    void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
      presentation: { ...(tableConfiguration?.presentation ?? { columns: {} }), frozen_property_id: columnId },
    });
  }

  function updateGrouping(propertyId: string | null): void {
    void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
      group_property_id: propertyId, group_order: [], collapsed_group_ids: [],
    });
  }

  function toggleGroup(id: string): void {
    if (editingLocked || mutating) return;
    if (temporarilyExpandedGroupIds.includes(id) && tableConfiguration?.collapsed_group_ids?.includes(id)) {
      temporarilyExpandedGroupIds = temporarilyExpandedGroupIds.filter((groupId) => groupId !== id);
      return;
    }
    const collapsed = new Set(tableConfiguration?.collapsed_group_ids ?? []);
    if (collapsed.has(id)) collapsed.delete(id); else collapsed.add(id);
    void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration, collapsed_group_ids: [...collapsed] });
  }

  function moveGroup(id: string, direction: -1 | 1): void {
    const order = groups.map((group) => group.id);
    const index = order.indexOf(id);
    const neighbor = order[index + direction];
    if (index < 0 || neighbor === undefined) return;
    order[index + direction] = id;
    order[index] = neighbor;
    void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration, group_order: order });
  }

  function calculationLabel(calculation: NotesDatabaseTableCalculation): string {
    return t(`notes.databaseTablePresentation.calculation.${calculation}`);
  }

  function calculationText(groupId: string | null, column: NotesDatabaseTableColumn): string {
    const calculation = notesTableColumnPresentation(tableConfiguration?.presentation, column.id).calculation;
    const values = groupId === null ? table?.calculations?.overall
      : table?.calculations?.groups && Object.hasOwn(table.calculations.groups, groupId) ? table.calculations.groups[groupId] : undefined;
    const storedValue = values && Object.hasOwn(values, column.id) ? values[column.id] : undefined;
    const isEmptyGroup = groupId !== null && (!table?.group_counts || !Object.hasOwn(table.group_counts, groupId) || table.group_counts[groupId] === 0);
    const value = storedValue === undefined && isEmptyGroup && calculation && ["count_all", "count_values", "empty", "unique", "sum"].includes(calculation) ? 0 : storedValue;
    return typeof value === "number" ? formatNumber(localization.locale, value, calculation === "percent_checked" ? { style: "percent", maximumFractionDigits: 1 } : { maximumFractionDigits: 3 }) : t("notes.databaseTableEmptyCell");
  }

  function groupName(group: typeof groups[number]): string {
    if (group.id === "__empty__") return t("notes.databaseTablePresentation.noValue");
    const column = columns.find((candidate) => candidate.id === tableConfiguration?.group_property_id);
    if (column?.type === "checkbox") return t(group.id === "true" ? "notes.databaseTablePresentation.checked" : "notes.databaseTablePresentation.unchecked");
    return group.name;
  }

  function saveColorRules(rules: NotesDatabaseTableColorRule[]): void {
    void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration,
      presentation: { ...(tableConfiguration?.presentation ?? { columns: {}, frozen_property_id: null }), color_rules: rules },
    });
  }

  function updateColorRule(id: string, change: Partial<NotesDatabaseTableColorRule>): void {
    saveColorRules((tableConfiguration?.presentation?.color_rules ?? []).map((rule) => rule.id === id ? { ...rule, ...change } : rule));
  }

  function addColorRule(): void {
    const property = columns.find((column) => notesDatabaseFilterConditions(column).length > 0);
    if (property) saveColorRules([...(tableConfiguration?.presentation?.color_rules ?? []), { id: crypto.randomUUID(), property_id: null, color: "blue", filters: [notesDatabaseNewFilter(property)] }]);
  }

  /** Paint a matching color rule: a row-level color fills the row after the gutter, a property color fills its cell. */
  function conditionalColorStyle(row: NotesPage, propertyId: string | null): string {
    const rules = tableConfiguration?.presentation?.color_rules ?? [];
    const matches = (candidate: NotesDatabaseTableColorRule) => notesDatabaseRowMatchesFilters(row, columns, candidate.filters);
    const rule = rules.find((candidate) => candidate.property_id === propertyId && matches(candidate))
      ?? (propertyId === null ? undefined : rules.find((candidate) => candidate.property_id === null && matches(candidate)));
    if (!rule) return "";
    const fill = propertyId === null ? "--collection-row-fill" : "background-color";
    return `${notesBlockColorStyle(`${rule.color}_background` as NotesColor)}; ${fill}: var(--notes-block-bg);`;
  }

  /** Move a visible property one place left or right, keeping the required title first. */
  function moveColumn(column: NotesDatabaseTableColumn, direction: -1 | 1): void {
    void placeColumn(column.id, visibleColumns.findIndex((candidate) => candidate.id === column.id) + direction);
  }

  /**
   * Place a property at `visibleIndex` among the visible columns. The title stays first, hidden properties keep their
   * place in the saved order, and the new order shows until the save settles or rolls back.
   */
  async function placeColumn(columnId: string, visibleIndex: number): Promise<void> {
    const column = columns.find((candidate) => candidate.id === columnId);
    if (!column || column.type === "title" || mutating || editingLocked) return;
    const ids = columns.map((candidate) => candidate.id);
    const firstMovable = visibleColumns.findIndex((candidate) => candidate.type !== "title");
    const order = collectionPlaceVisibleItem(ids, visibleColumns.map((candidate) => candidate.id), columnId, Math.max(firstMovable, visibleIndex));
    if (order.every((id, index) => id === ids[index])) return;
    pendingColumnOrder = order;
    try {
      await persistTable(collectionOrderById(columns, order).map((candidate) => ({ ...candidate })), rowOpenMode, filters, sorts);
    } finally {
      pendingColumnOrder = null;
    }
  }

  /** Change the chosen property sort while retaining its existing priority. */
  function sortColumn(columnId: string, direction: NotesDatabaseTableSort["direction"]): void {
    if (mutating || (sorts.length >= NOTES_DATABASE_QUERY_MAX_SORTS && !sorts.some((sort) => sort.property_id === columnId))) return;
    saveSorts(notesDatabaseSortsWithColumn(sorts, columnId, direction));
  }

  function updateColumnWidth(columnId: string, delta: number): void {
    if (mutating) return;
    const nextColumns = columns.map((column) =>
      column.id === columnId
        ? { ...column, width: notesDatabaseTableColumnWidth(column, delta) }
        : { ...column },
    );
    const resizedColumn = nextColumns.find((column) => column.id === columnId);
    if (!resizedColumn) return;
    pendingColumnWidth = { id: columnId, width: resizedColumn.width };
    void persistTable(nextColumns, rowOpenMode, filters, sorts).finally(() => {
      pendingColumnWidth = null;
    });
  }

  function updateRowOpenMode(mode: NotesDatabaseTableRowOpenMode): void {
    void persistTable(columns.map((column) => ({ ...column })), mode, filters, sorts);
  }

  function openRow(row: NotesPage): void {
    if (rowOpenMode === "side_panel") {
      selectedPanelRowId = row.id;
      return;
    }
    onSelectPage(row.id);
  }

  function rowTitle(row: NotesPage): string {
    const titleColumn = columns.find((column) => column.type === "title");
    const title = titleColumn ? notesDatabaseTableCellText(row, titleColumn).trim() : "";
    return title || t("notes.untitled");
  }

  function rowOpenModeLabel(mode: NotesDatabaseTableRowOpenMode): string {
    return mode === "side_panel"
      ? t("notes.databaseTableOpenSidePanel")
      : t("notes.databaseTableOpenFullPage");
  }

  function handleCellKeydown(
    event: KeyboardEvent,
    rowIndex: number,
    columnIndex: number,
  ): void {
    event.stopPropagation();
    if (notesDatabaseCellKeepsKey(event.key, event.shiftKey, notesDatabaseCellCaret(event.currentTarget))) return;
    if (event.defaultPrevented || (event.currentTarget instanceof HTMLElement && event.currentTarget.hasAttribute("aria-haspopup"))) return;
    const maxRowIndex = Math.max(0, renderedRows.length - 1);
    const maxColumnIndex = Math.max(0, visibleColumns.length - 1);
    let nextRowIndex = rowIndex;
    let nextColumnIndex = columnIndex;
    if (event.key === "ArrowRight") nextColumnIndex = Math.min(maxColumnIndex, columnIndex + 1);
    else if (event.key === "ArrowLeft") nextColumnIndex = Math.max(0, columnIndex - 1);
    else if (event.key === "ArrowDown") nextRowIndex = Math.min(maxRowIndex, rowIndex + 1);
    else if (event.key === "ArrowUp") nextRowIndex = Math.max(0, rowIndex - 1);
    else if (event.key === "Enter") nextRowIndex = Math.min(maxRowIndex, rowIndex + 1);
    else return;
    event.preventDefault();
    if (event.key === "Enter" && nextRowIndex === rowIndex) {
      if (event.currentTarget instanceof HTMLElement) event.currentTarget.blur();
      tableRoot?.querySelector<HTMLButtonElement>("[data-database-new-row] button")?.focus();
    } else focusCell(nextRowIndex, nextColumnIndex);
  }

  function focusCell(rowIndex: number, columnIndex: number): void {
    const selector = `[data-table-cell="true"][data-row-index="${rowIndex}"][data-column-index="${columnIndex}"]`;
    const target = tableRoot?.querySelector(selector);
    if (target instanceof HTMLElement) target.focus();
  }
</script>

{#snippet calculationRow(groupId: string | null)}
  {#if visibleColumns.some((column) => notesTableColumnPresentation(tableConfiguration?.presentation, column.id).calculation)}
    <CollectionRow template={gridTemplate} gutter={ROW_GUTTER} role="row" data-table-calculation-group={groupId ?? "__all__"} class="text-muted-foreground"
      style="--collection-row-fill: color-mix(in srgb, var(--muted) 20%, transparent);">
      <div role="cell" class={`self-stretch ${gutterCellClass(false)}`} style={gutterCellStyle("selection")}></div>
      <div role="cell" class={`self-stretch ${gutterCellClass(false)}`} style={gutterCellStyle("expand")}></div>
      {#each visibleColumns as column (column.id)}
        {@const calculation = notesTableColumnPresentation(tableConfiguration?.presentation, column.id).calculation}
        <CollectionCell role="cell" class={frozenOffset(column.id) !== null ? "sticky z-20 bg-background" : ""} style={frozenStyle(column.id)}>
          {#if calculation}<span class="min-w-0 truncate text-[0.75rem]" title={calculationLabel(calculation)}>{calculationLabel(calculation)} <span class="text-foreground">{calculationText(groupId, column)}</span></span>{/if}
        </CollectionCell>
      {/each}
      <div role="cell"></div>
    </CollectionRow>
  {/if}
{/snippet}

<svelte:window onpointermove={moveColumnResize} onpointerup={(event) => finishColumnResize(event, true)} onpointercancel={(event) => finishColumnResize(event, false)} onkeydown={(event) => { if (event.key === "Escape") columnResize = null; trashSelectionKey(event); }} />

<section bind:this={tableSection} data-collection-selection-scope class="space-y-3 pt-2" aria-label={t("notes.databaseTableTitle")}>

  <NotesDatabaseQueryBar properties={columns} {filters} {sorts} pending={mutating || editingLocked} onFiltersChange={saveFilters} onSortsChange={saveSorts} />
  {#if table}<span class="sr-only" role="status">{t("notes.databaseRowsCount", visibleRows.length)}</span>{/if}

  {#if error && table}<Notice class="text-[0.8rem]" message={error} />{/if}

  {#if !table && error}
    <div class="min-h-32 text-[0.8rem]"><Notice layout="fill" message={t("notes.databaseTableFailed", error)} copy={error} onRetry={retryTable} /></div>
  {:else if !table}<NotesLoadingSkeleton kind="table" />{/if}
  {#if table}
    <div class="grid gap-2 @container">
      {#if settingsOpen}
        <CollectionSettings label={t("notes.databaseViewSettings")} anchor={settingsAnchor} preferredWidth={COLLECTION_VIEW_SETTINGS_PANEL_WIDTH} showHeader={false} onClose={onCloseSettings}>
          {@render settingsHeader?.()}
          {#if !editingLocked}
            <CollectionMenu label={t("notes.databaseLayout")} icon={PanelsTopLeft} summary={t("notes.databaseViewTable")} fullWidth>
              <CollectionMenuSelect label={t("notes.databaseTableOpenMode")} icon={ExternalLink} value={rowOpenMode} disabled={loading || mutating || !table}
                options={[{ value: "full_page", label: rowOpenModeLabel("full_page") }, { value: "side_panel", label: rowOpenModeLabel("side_panel") }]}
                onChange={updateRowOpenMode} />
            </CollectionMenu>
            <CollectionMenu label={t("notes.databaseTablePresentation.groupBy")} kind="group" summary={columns.find((column) => column.id === tableConfiguration?.group_property_id)?.name ?? t("notes.databaseTablePresentation.noGroup")} fullWidth>
              <CollectionMenuItem label={t("notes.databaseTablePresentation.noGroup")} checked={!tableConfiguration?.group_property_id} disabled={mutating} onclick={() => updateGrouping(null)} />
              {#each notesTableGroupableColumns(columns) as column (column.id)}
                <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={tableConfiguration?.group_property_id === column.id} disabled={mutating} onclick={() => updateGrouping(column.id)} />
              {/each}
              <CollectionMenuSeparator />
              <CollectionMenuItem icon={EyeOff} label={t("notes.databaseTablePresentation.hideEmptyGroups")} checked={tableConfiguration?.hide_empty_groups ?? false} disabled={mutating}
                onclick={() => { void persistTable(columns, rowOpenMode, filters, sorts, { ...tableConfiguration, hide_empty_groups: !(tableConfiguration?.hide_empty_groups ?? false) }); }} />
              {#if tableConfiguration?.group_property_id && groups.length > 0}
                <CollectionMenuSeparator />
                {#each groups as group, index (group.id)}
                  <div class="collection-menu-row flex min-w-0 items-center gap-1 pl-2">
                    <span class="min-w-0 flex-1 truncate">{groupName(group)}</span>
                    <span class="px-1 tabular-nums text-muted-foreground">{formatNumber(localization.locale, group.count)}</span>
                    <button type="button" class="collection-settings-control flex items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:text-muted-foreground/50 disabled:hover:bg-transparent" aria-label={t("notes.databaseTablePresentation.moveGroupUp", groupName(group))} disabled={mutating || index === 0} onclick={() => moveGroup(group.id, -1)}><ArrowUp class="size-3.5" /></button>
                    <button type="button" class="collection-settings-control flex items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:text-muted-foreground/50 disabled:hover:bg-transparent" aria-label={t("notes.databaseTablePresentation.moveGroupDown", groupName(group))} disabled={mutating || index === groups.length - 1} onclick={() => moveGroup(group.id, 1)}><ArrowDown class="size-3.5" /></button>
                  </div>
                {/each}
              {/if}
            </CollectionMenu>
            <CollectionMenu label={t("notes.databaseTableColumns")} kind="properties" summary={formatNumber(localization.locale, visibleColumns.length)} fullWidth>
              {#each columns as column (column.id)}
                <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)]} label={column.name} checked={!column.hidden} disabled={mutating || column.type === "title"}
                  onclick={() => updateColumnVisibility(column.id, !column.hidden)} />
              {/each}
            </CollectionMenu>
            <CollectionMenu label={t("notes.databaseTableSorts")} kind="sort" activeCount={sorts.length}
              summary={formatList(localization.locale, sorts.flatMap((sort) => {
                const column = columns.find((candidate) => candidate.id === sort.property_id);
                return column ? [column.name] : [];
              }))} fullWidth>
              <NotesDatabaseSortControls properties={columns} {sorts} pending={mutating} onChange={saveSorts} />
            </CollectionMenu>
            <CollectionMenu label={t("notes.databaseTableFilters")} kind="filter" activeCount={notesDatabaseFilterCount(filters)} summary={notesDatabaseFilterCount(filters) > 0 ? formatNumber(localization.locale, notesDatabaseFilterCount(filters)) : ""} fullWidth>
              <NotesDatabaseFilterControls properties={columns} {filters} pending={mutating} onChange={saveFilters} />
            </CollectionMenu>
            <CollectionMenu label={t("notes.databaseTablePresentation.conditionalColor")} icon={Palette} summary={formatNumber(localization.locale, tableConfiguration?.presentation?.color_rules?.length ?? 0)} fullWidth>
              {#each tableConfiguration?.presentation?.color_rules ?? [] as rule (rule.id)}
                <CollectionMenuSelect label={t("notes.databaseTablePresentation.target")} value={rule.property_id ?? ""} disabled={mutating}
                  options={[{ value: "", label: t("notes.databaseTablePresentation.rowColor") }, ...columns.map((column) => ({ value: column.id, label: column.name, icon: COLLECTION_PROPERTY_ICONS[notesPropertyKind(column.type)] }))]}
                  onChange={(value) => updateColorRule(rule.id, { property_id: value || null })} />
                <CollectionMenuSelect label={t("notes.databaseTablePresentation.color")} icon={Palette} value={rule.color} disabled={mutating}
                  options={NOTES_TEXT_COLORS.filter((color) => color !== "default").map((color) => ({ value: color, label: t(`notes.blockColor.${color}`) }))}
                  onChange={(value) => updateColorRule(rule.id, { color: value })} />
                <NotesDatabaseFilterControls properties={columns} filters={rule.filters} pending={mutating} onChange={(next) => { if (next.length) updateColorRule(rule.id, { filters: next }); }} />
                <CollectionMenuItem icon={Trash2} label={t("notes.databaseTablePresentation.removeColorRule")} destructive disabled={mutating}
                  onclick={() => saveColorRules((tableConfiguration?.presentation?.color_rules ?? []).filter((candidate) => candidate.id !== rule.id))} />
                <CollectionMenuSeparator />
              {/each}
              <CollectionMenuItem icon={Plus} label={t("notes.databaseTablePresentation.addColorRule")} disabled={mutating || (tableConfiguration?.presentation?.color_rules?.length ?? 0) >= 16} onclick={addColorRule} />
            </CollectionMenu>
            <CollectionMenu label={t("notes.databaseTemplatesTitle")} icon={LayoutTemplate} summary={templates.length > 0 ? formatNumber(localization.locale, templates.length) : ""} fullWidth>
              <CollectionMenuSelect label={t("notes.databaseTemplatesUse")} icon={FileText} value={selectedTemplateId} disabled={mutating}
                options={[{ value: "", label: t("notes.databaseTemplatesNone") },
                  ...templates.map((template) => ({ value: String(template.id), label: template.is_default ? t("notes.databaseTemplatesDefaultOption", template.name) : template.name }))]}
                onChange={(nextValue) => { selectedTemplateId = nextValue; }} />
              <CollectionMenuItem icon={Trash2} label={t("notes.databaseTemplatesDelete")} destructive disabled={mutating || !selectedTemplateId} onclick={() => { void deleteSelectedTemplate(); }} />
              <CollectionMenuSeparator />
              <input class="field mb-1 w-full min-w-0" aria-label={t("notes.databaseTemplatesName")}
                value={templateName} placeholder={t("notes.databaseTemplatesNamePlaceholder")} disabled={mutating}
                oninput={(event) => { templateName = event.currentTarget.value; }}
                onkeydown={(event) => event.stopPropagation()} />
              <CollectionMenuSelect label={t("notes.databaseTemplatesSourceRow")} icon={Rows3} value={templateSourceRowId} disabled={mutating || templateSourceRows.length === 0}
                options={[{ value: "", label: t("notes.databaseTemplatesFirstRow") }, ...templateSourceRows.map((row) => ({ value: String(row.id), label: rowTitle(row) }))]}
                onChange={(nextValue) => { templateSourceRowId = nextValue; }} />
              <CollectionMenuItem icon={Star} label={t("notes.databaseTemplatesMakeDefault")} checked={createTemplateAsDefault} disabled={mutating} onclick={() => { createTemplateAsDefault = !createTemplateAsDefault; }} />
              <CollectionMenuItem icon={Plus} label={t("notes.databaseTemplatesCreate")} disabled={mutating || !templateName.trim() || templateSourceRows.length === 0} onclick={() => { void createTemplateFromRow(); }} />
            </CollectionMenu>
          {/if}
          <NotesDatabaseSettingsFooter reloadLabel={t("notes.databaseTableReload")} {editingLocked} reloadDisabled={loading || mutating}
            onEditProperties={() => { onCloseSettings(); onEditProperties(); }} onReload={() => { void loadTable(); }}>
            {#snippet actions()}
              <CollectionMenu label={t("notes.databaseMore")} kind="actions" fullWidth>
                <CollectionMenuItem icon={FileUp} label={t("notes.databaseCsvImportTitle")} onclick={() => requestCsvPanel("database-csv-import")} />
                {#if fileExportAvailable}
                  <CollectionMenuItem icon={FileDown} label={t("notes.databaseCsvExportTitle")} onclick={() => requestCsvPanel("database-csv-export")} />
                {/if}
              </CollectionMenu>
            {/snippet}
          </NotesDatabaseSettingsFooter>
        </CollectionSettings>
      {/if}

      {#if !editingLocked && openCsvPanelKind === "database-csv-import" && csvPanelLoadState?.status === "ready" && csvPanelLoadState.component.kind === "database-csv-import"}
        {@const NotesDatabaseCsvImportPanel = csvPanelLoadState.component.component}
        <NotesDatabaseCsvImportPanel
          {dataSourceId}
          disabled={mutating || loading}
          onImported={async () => {
            await loadTable();
          }}
        />
      {:else if fileExportAvailable && openCsvPanelKind === "database-csv-export" && csvPanelLoadState?.status === "ready" && csvPanelLoadState.component.kind === "database-csv-export"}
        {@const NotesDatabaseCsvExportPanel = csvPanelLoadState.component.component}
        <NotesDatabaseCsvExportPanel
          {dataSourceId}
          {databaseId}
          {viewId}
          disabled={mutating || loading}
        />
      {:else if openCsvPanelKind && csvPanelLoadState?.status === "failed"}
        <Notice class="text-[0.8rem]" message={t("common.viewLoadFailed", t(openCsvPanelKind === "database-csv-import" ? "notes.databaseCsvImportTitle" : "notes.databaseCsvExportTitle"))} copy={false} onRetry={retryCsvPanel} />
      {/if}

      {#if selectedRows.length > 0}
        {@const single = selectedRows.length === 1 ? selectedRows[0] : null}
        <!-- A zero-height sticky anchor floats the toolbar over the controls above the header without moving the rows, and keeps it in view while the table scrolls under the page top. -->
        <div class="sticky top-11 z-30 -mb-2 h-0">
          <CollectionSelectionBar class="absolute bottom-1 left-0" count={selectedRows.length} onClear={() => { selectedRowIds = []; }}>
            {#each bulkColumns as column (column.id)}
              <NotesDatabaseBulkPropertyMenu {column} rows={selectedRows} disabled={mutating} onApply={(value) => saveRowsProperty(selectedRows, column, value)} />
            {/each}
            <CollectionSelectionAction icon={Trash2} iconOnly label={t("notes.databaseSelectionTrash")} disabled={mutating} onclick={() => { void trashRows(selectedRows); }} />
            <CollectionMenu iconOnly kind="actions" label={t("collections.selection.more")} triggerClass="h-8 rounded-none px-2.5" disabled={mutating}>
              <div class="grid gap-0">
                {#if single}
                  <CollectionMenuItem icon={rowOpenMode === "side_panel" ? PanelRight : FileText} label={t("collections.property.open")} onclick={() => openRow(single)} />
                {/if}
                <CollectionMenuItem icon={Copy} label={t("notes.databaseSelectionDuplicate")} onclick={() => { void duplicateRows(selectedRows); }} />
                {#if single}
                  <CollectionMenu label={t("notes.databaseSubitemMove")} kind="property" icon={CornerDownRight} fullWidth disabled={mutating}>
                    <NotesDatabaseRowParentControls rowId={single.id} rows={visibleRows} hierarchy={rowHierarchy} titleFor={rowTitle} pending={mutating} onSave={(parentId) => moveSubitems([single], parentId)} />
                  </CollectionMenu>
                {/if}
                {#if selectedRows.some((row) => rowHierarchy[row.id]?.parent_row_page_id)}
                  <CollectionMenuItem icon={ArrowUp} label={t("notes.databaseSubitemMoveToRoot")}
                    onclick={() => { void moveSubitems(selectedRows.filter((row) => rowHierarchy[row.id]?.parent_row_page_id), null); }} />
                {/if}
                <CollectionMenuItem icon={X} label={t("collections.selection.clear")} onclick={() => { selectedRowIds = []; }} />
                <CollectionMenuSeparator />
                <CollectionMenuItem icon={Trash2} label={t("notes.databaseSelectionTrash")} destructive onclick={() => { void trashRows(selectedRows); }} />
              </div>
            </CollectionMenu>
          </CollectionSelectionBar>
        </div>
      {/if}

      <!-- The bottom padding keeps the last row border inside the scroll clip when zoom gives rows fractional heights. -->
      <div bind:this={tableRoot} use:rememberDatabaseScroll={databaseResource("table", dataSourceId, viewScope()).key} class="min-w-0 overflow-x-auto overflow-y-hidden pb-px"
        style={`margin-left: calc(-1 * min(${gutterRoom}px, ${ROW_CONTENT_START}));`}>
        <div role="table" aria-label={t("notes.databaseViewTable")} class="isolate min-w-max" style={`--collection-divider-layer: ${ROW_DIVIDER_LAYER};`}>
          <CollectionRow template={gridTemplate} gutter={ROW_GUTTER} header dividerTop role="row">
            <div role="columnheader" class={`flex justify-center self-stretch items-center ${gutterCellClass(true)}`} style={gutterCellStyle("selection")}>
              <CollectionSelectionCheckbox checked={allRowsSelected} indeterminate={!allRowsSelected && selectedRows.length > 0} disabled={selectableRows.length === 0}
                label={t(allRowsSelected ? "collections.selection.unselectAll" : "collections.selection.selectAll")} onToggle={toggleAllRowsSelected} />
            </div>
            <div role="columnheader" class={`self-stretch ${gutterCellClass(true)}`} style={gutterCellStyle("expand")}></div>
            {#each visibleColumns as column (column.id)}
              {@const columnIndex = visibleColumns.findIndex((candidate) => candidate.id === column.id)}
              {@const sortUnavailable = mutating || editingLocked || (sorts.length >= NOTES_DATABASE_QUERY_MAX_SORTS && !sorts.some((sort) => sort.property_id === column.id))}
              <CollectionColumnHeader label={column.name} resizeLabel={t("notes.databaseTableResizeColumn", column.name)} disabled={mutating || editingLocked}
                columnId={column.id} movable={column.type !== "title"} reorder={columnReorder}
                class={frozenOffset(column.id) !== null ? "sticky z-30 bg-background" : ""}
                style={frozenStyle(column.id)}
                onpointerdown={(event) => startColumnResize(event, column)} onkeydown={(event) => resizeColumnKey(event, column)}>
                {#snippet actions()}
                  {@const presentation = notesTableColumnPresentation(tableConfiguration?.presentation, column.id)}
                  {@const frozen = tableConfiguration?.presentation?.frozen_property_id === column.id}
                  {@const kind = notesPropertyKind(column.type)}
                  {@const schemaOptions = Boolean(propertyEditor) && notesPropertyHasSchemaOptions(column.type)}
                  {@const dateColumn = column.type === "date" || column.type === "created_time" || column.type === "last_edited_time"}
                  {@const columnSort = sorts.find((sort) => sort.property_id === column.id)}
                  {@const groupable = notesTableGroupableColumns(columns).some((candidate) => candidate.id === column.id)}
                  {@const grouped = tableConfiguration?.group_property_id === column.id}
                  <CollectionMenu label={column.name} kind="property" fullWidth showHeader={false} dismissOnAction bind:triggerElement={columnMenuTriggers[column.id]}
                    triggerClass="h-auto justify-start rounded-none px-2 font-normal" triggerAttributes={{ "data-collection-cell-primary": "" }} contextMenu disabled={editingLocked}>
                    {#snippet leading()}{#if preferences.notesShowPropertyIcons}<CollectionPropertyIcon icon={column.icon ?? null} {kind} class="size-3.5 shrink-0 text-muted-foreground" />{/if}{/snippet}
                    <div class="grid gap-0 font-normal">
                      <CollectionPropertyNameField propertyId={column.id} name={column.name} icon={column.icon ?? null} {kind} editable={!editingLocked && Boolean(onPropertyAction)}
                        maxLength={NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS} onRename={(name) => renameProperty(column.id, name)}
                        onIconChange={onPropertyAction ? (icon) => changePropertyIcon(column.id, icon) : undefined} />
                      {#if schemaOptions || dateColumn}
                        <CollectionMenu label={t("collections.property.editProperty")} kind="properties" icon={Settings2} fullWidth disabled={mutating}
                          onOpenChange={(open) => { if (open && schemaOptions) onLoadPropertyEditor?.(column.id); }}>
                          {#if schemaOptions && propertyEditor}{@render propertyEditor(column.id)}{/if}
                          {#if dateColumn}
                            <CollectionMenuSelect label={t("notes.databaseTablePresentation.dateFormat")} icon={CalendarCog} value={presentation.date_format} disabled={mutating}
                              options={DATE_FORMATS.map((format) => ({ value: format.value, label: t(format.label) }))} onChange={(date_format) => updateColumnPresentation(column.id, { date_format })} />
                            <CollectionMenuSelect label={t("notes.databaseTablePresentation.timeFormat")} icon={Clock} value={presentation.time_format} disabled={mutating}
                              options={TIME_FORMATS.map((format) => ({ value: format.value, label: t(format.label) }))} onChange={(time_format) => updateColumnPresentation(column.id, { time_format })} />
                          {/if}
                        </CollectionMenu>
                      {/if}
                      {#if column.type !== "title" && onPropertyAction}
                        <CollectionPropertyTypeMenu types={propertyTypeOptions} current={column.type} disabled={mutating}
                          onChange={(propertyType) => { void performPropertyAction({ type: "changeType", propertyId: column.id, propertyType }); }} />
                      {/if}
                      <CollectionMenuSeparator />
                      {#if notesDatabaseFilterConditions(column).length > 0}
                        <CollectionMenu label={t("collections.property.filter")} kind="filter" fullWidth activeCount={notesDatabaseFilterCount(filters, column.id)}>
                          <NotesDatabaseFilterControls properties={columns} {filters} propertyId={column.id} pending={mutating} onChange={saveFilters} />
                        </CollectionMenu>
                      {/if}
                      <CollectionMenu label={t("collections.property.sort")} kind="sort" fullWidth dismissOnAction disabled={sortUnavailable}
                        summary={columnSort ? t(columnSort.direction === "ascending" ? "collections.property.sortAscending" : "collections.property.sortDescending") : undefined}>
                        <CollectionMenuItem icon={ArrowUp} label={t("collections.property.sortAscending")} disabled={sortUnavailable}
                          checked={columnSort?.direction === "ascending"} onclick={() => sortColumn(column.id, "ascending")} />
                        <CollectionMenuItem icon={ArrowDown} label={t("collections.property.sortDescending")} disabled={sortUnavailable}
                          checked={columnSort?.direction === "descending"} onclick={() => sortColumn(column.id, "descending")} />
                      </CollectionMenu>
                      {#if groupable}
                        <CollectionMenuItem icon={Group} label={t(grouped ? "collections.property.ungroup" : "collections.property.group")} disabled={mutating}
                          onclick={() => updateGrouping(grouped ? null : column.id)} />
                      {/if}
                      <CollectionMenu label={t("collections.property.calculate")} kind="properties" icon={Sigma} fullWidth summary={presentation.calculation ? calculationLabel(presentation.calculation) : undefined}>
                        <CollectionMenuItem label={t("notes.databaseTablePresentation.none")} checked={presentation.calculation === null} disabled={mutating} onclick={() => updateColumnPresentation(column.id, { calculation: null })} />
                        {#each notesTableCompatibleCalculations(column) as calculation}
                          <CollectionMenuItem label={calculationLabel(calculation)} checked={presentation.calculation === calculation} disabled={mutating} onclick={() => updateColumnPresentation(column.id, { calculation })} />
                        {/each}
                      </CollectionMenu>
                      <CollectionMenuItem icon={frozen ? PinOff : Pin} label={t(frozen ? "collections.property.unfreeze" : "collections.property.freeze")} disabled={mutating} onclick={() => freezeThrough(frozen ? null : column.id)} />
                      {#if column.type !== "title"}<CollectionMenuItem icon={EyeOff} label={t("collections.property.hide")} disabled={mutating} onclick={() => updateColumnVisibility(column.id, true)} />{/if}
                      <CollectionMenuItem icon={TextWrap} label={t("collections.property.wrap")} checked={presentation.wrap} disabled={mutating} onclick={() => updateColumnPresentation(column.id, { wrap: !presentation.wrap })} />
                      <CollectionMenuSeparator />
                      {#if column.type !== "title"}
                        <CollectionMenuItem icon={ArrowLeft} label={t("collections.property.moveLeft")} disabled={mutating || columnIndex < 2} onclick={() => moveColumn(column, -1)} />
                        <CollectionMenuItem icon={ArrowRight} label={t("collections.property.moveRight")} disabled={mutating || columnIndex === visibleColumns.length - 1} onclick={() => moveColumn(column, 1)} />
                      {/if}
                      {#if onPropertyAction}
                        {#each propertyInsertionSides(column) as side (side)}
                          <CollectionMenuItem icon={side === "left" ? ArrowLeftToLine : ArrowRightToLine} label={t(side === "left" ? "collections.property.insertLeft" : "collections.property.insertRight")}
                            disabled={mutating} onclick={() => openInsertPropertyDraft(column, side)} />
                        {/each}
                        {#if column.type !== "title"}
                          <CollectionMenuItem icon={Copy} label={t("collections.property.duplicate")} disabled={mutating} onclick={() => { void performPropertyAction({ type: "duplicate", propertyId: column.id }); }} />
                          <CollectionMenuItem icon={Trash2} label={t("collections.property.delete")} destructive disabled={mutating} onclick={() => { void performPropertyAction({ type: "delete", propertyId: column.id }); }} />
                        {/if}
                      {/if}
                    </div>
                  </CollectionMenu>
                {/snippet}
              </CollectionColumnHeader>
            {/each}
            <div role="columnheader" class="grid min-h-(--collection-table-row-height) min-w-0 items-center justify-items-start self-stretch font-normal text-muted-foreground">
              <CollectionAddPropertyButton bind:element={addPropertyButton} open={propertyDraft?.columnId === null} disabled={mutating || editingLocked} onclick={toggleAddPropertyDraft} />
            </div>
            {#if propertyDraft}
              {#key propertyDraft}
                <CollectionPropertyDraft anchor={propertyDraft.anchor} placement={propertyDraft.placement} trigger={propertyDraft.trigger} types={propertyTypeOptions} pending={mutating}
                  maxLength={NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS} onCreate={createDraftProperty} onClose={() => { propertyDraft = null; }} />
              {/key}
            {/if}
          </CollectionRow>
            {#each tableItems as item (item.id)}
              {#if item.type === "group"}
                <div role="row" data-table-group-id={item.group.id} class="flex min-h-(--collection-table-row-height) items-center gap-2 px-2 text-[0.8rem]"
                  style={`margin-left: ${ROW_CONTENT_START}; box-shadow: inset 0 -1px 0 var(--cal-gridline);`}>
                  <button type="button" class="flex min-h-8 min-w-0 items-center gap-2 rounded px-1 text-left hover:bg-accent" disabled={mutating || editingLocked} aria-expanded={!item.group.collapsed} onclick={() => toggleGroup(item.group.id)}><ChevronRight class={`size-3.5 shrink-0 ${item.group.collapsed ? "" : "rotate-90"}`} /><span>{groupName(item.group)}</span><span class="text-muted-foreground">{formatNumber(localization.locale, item.group.count)}</span></button>
                  {#if ["select", "multi_select", "status", "checkbox", "date"].includes(columns.find((column) => column.id === tableConfiguration?.group_property_id)?.type ?? "")}
                    <button type="button" class="ml-auto flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-accent" aria-label={t("notes.databaseNewPage")} disabled={mutating || loading} onclick={() => createRow(item.group.id)}><Plus class="size-3.5" /></button>
                  {/if}
                </div>
              {:else if item.type === "calculation"}
                {@render calculationRow(item.groupId)}
              {:else if item.type === "addSubitem"}
                <CollectionRow template={gridTemplate} gutter={ROW_GUTTER} role="row" data-database-subitem-add-row={item.parent.id}>
                  <CollectionQuickAdd label={t("notes.databaseSubitemCreate")} contentColumn={3}
                    indent={titleIndent((rowHierarchy[item.parent.id]?.depth ?? 0) + 1)}
                    disabled={mutating || loading || rowCreation.blocked(item.parent.id)} onCreate={() => { void createSubitem(item.parent); }} />
                </CollectionRow>
              {:else}
              {@const row = item.row}
              {@const rowIndex = item.rowIndex}
              {@const title = rowTitle(row)}
              {@const hierarchy = rowHierarchy[row.id]}
              {@const selected = selectedRowIds.includes(row.id)}
              {@const expanded = rowExpanded(row)}
              <CollectionRow template={gridTemplate} gutter={ROW_GUTTER} role="row" {selected} data-database-row-id={row.id} data-database-row-depth={hierarchy?.depth ?? 0} style={conditionalColorStyle(row, null)}>
                <div role="cell" class={`flex items-center justify-center self-stretch ${gutterCellClass(false)}`} style={gutterCellStyle("selection")}>
                  <CollectionSelectionCheckbox checked={selected} disabled={rowCreation.blocked(row.id)}
                    label={t(selected ? "collections.selection.unselect" : "collections.selection.select", title)} onToggle={() => toggleRowSelected(row)} />
                </div>
                <div role="cell" class={`flex items-center justify-center self-stretch ${gutterCellClass(false)}`} style={gutterCellStyle("expand")}>
                  <CollectionExpandToggle {expanded} hasChildren={rowHasChildren(row)} disabled={rowExpansionLocked(row) || rowCreation.blocked(row.id)}
                    label={t(expanded ? "collections.selection.collapse" : "collections.selection.expand", title)}
                    onToggle={() => toggleRowExpanded(row)} />
                </div>
                {#each visibleColumns as column, columnIndex (column.id)}
                  {@const editValue = rowCreation.valueFor(row, column)}
                  <CollectionCell role="cell" class={`${frozenOffset(column.id) !== null ? "sticky z-20 bg-background" : ""} ${notesTableColumnPresentation(tableConfiguration?.presentation, column.id).wrap ? "notes-table-wrap" : ""}`}
                    style={`${frozenStyle(column.id) ?? ""} ${conditionalColorStyle(row, column.id)}`}>
                    <div class={column.type === "title" ? "flex w-full min-w-0 items-start gap-0.5" : "contents"}
                      style={column.type === "title" ? `padding-left: ${titleIndent(hierarchy?.depth ?? 0)};` : ""}>
                    <NotesDatabasePropertyEditor {row} {column} {rowIndex} {columnIndex} value={editValue}
                      disabled={mutating || (column.type !== "title" && rowCreation.blocked(row.id))}
                      onSave={(value) => saveCell(row, column, value)}
                      onNavigate={handleCellKeydown}
                      onEditingChange={(editing) => { isEditingCell = editing; }}
                      onDraft={(value) => rowCreation.draft(row.id, column.id, value)}
                      onRunButton={() => { void runButton(row, column); }} />
                    </div>
                    {#if column.type === "title"}
                      <button type="button" data-collection-hover-reveal="" class="absolute top-1/2 right-1 z-10 flex h-6 -translate-y-1/2 items-center gap-1 rounded-md border border-border bg-popover px-1.5 text-[0.75rem] text-popover-foreground opacity-100 hover:bg-accent hover:text-accent-foreground [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/row:opacity-100 [@media(hover:hover)]:group-has-focus-visible/row:opacity-100"
                        aria-label={t("notes.databaseRowsOpen", title)} disabled={mutating || rowCreation.blocked(row.id)} onclick={() => openRow(row)}>
                        <PanelRight class="size-3.5" aria-hidden="true" /><span>{t("collections.property.open")}</span>
                      </button>
                    {/if}
                  </CollectionCell>
                {/each}
                <div role="cell"></div>
                {#if rowCreation.errorFor(row.id)}
                  <Notice class="col-span-full px-2 py-1 text-[0.8rem]" message={rowCreation.errorFor(row.id) ?? ""} onRetry={() => rowCreation.retry(row.id)} />
                {/if}
              </CollectionRow>
              {/if}
            {/each}
            {@render calculationRow(null)}

          {#key addRowKey}
            <CollectionRow template={gridTemplate} gutter={ROW_GUTTER} data-database-new-row>
              <CollectionQuickAdd label={t("notes.databaseNewPage")} contentColumn={3} disabled={mutating || loading} onCreate={() => { void createRow(); }} />
            </CollectionRow>
          {/key}
        </div>
      </div>

      {#if table.has_more}
        <div bind:this={loadMoreSentinel} class="h-px" aria-hidden="true"></div>
        {#if loadingMore}
          <Loading layout="rows" rows={2} />
        {/if}
      {/if}

      {#if visibleColumns.length === 0}
        <p class="text-[0.8rem] text-muted-foreground">{t("notes.databaseTableNoVisibleColumns")}</p>
      {/if}

      {#if rowOpenMode === "side_panel" && selectedPanelRow}
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
                    {notesDatabaseTableCellText(selectedPanelRow, column) || t("notes.databaseTableEmptyCell")}
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
