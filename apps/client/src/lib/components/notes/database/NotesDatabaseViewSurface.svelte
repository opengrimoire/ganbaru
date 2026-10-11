<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import { tick, untrack, type Snippet } from "svelte";
  import { cn } from "$lib/utils";
  import { NOTES_DATABASE_VIEW_TAB_DISPLAYS, notesDatabaseViewTabDisplays, type NotesDatabaseViewTabDisplay } from "$lib/notes/database/view-tab-display.svelte";
  import { databaseResource, notesDatabaseSession } from "$lib/notes/database/session.svelte";
  import { compactSourceLocation } from "$lib/notes/database/source-location";
  import Table2 from "@lucide/svelte/icons/table-2";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import List from "@lucide/svelte/icons/list";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import ChartGantt from "@lucide/svelte/icons/chart-gantt";
  import Copy from "@lucide/svelte/icons/copy";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Database from "@lucide/svelte/icons/database";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import Link from "@lucide/svelte/icons/link";
  import Plus from "@lucide/svelte/icons/plus";
  import Paintbrush from "@lucide/svelte/icons/paintbrush";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import type { NotesDatabasePropertyActionRequest, NotesDatabaseSourceEditingScope } from "$lib/notes/database/data-source-schema";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import {
    deleteNotesDatabaseView,
    duplicateNotesDatabaseView,
    applyNotesDataSourceTemplate,
    createNotesDataSourceRowPage,
    getNotesDataSourceBoardView,
    getNotesDataSourceCalendarView,
    getNotesDataSourceGalleryView,
    getNotesDataSourceListView,
    getNotesDataSourceTableView,
    getNotesDataSourceTimelineView,
    listNotesDatabaseViews,
    listNotesDataSourceTemplates,
    renameNotesDatabaseView,
    listNotesDataSources,
    createNotesDataSource,
    attachNotesDataSource,
    setNotesDatabaseEditingLock,
  } from "$lib/api/notes";
  import {
    NOTES_DATABASE_VIEW_KINDS,
    type NotesDatabaseView,
    type NotesDatabaseViewKind,
    type NotesDataSourceTemplate,
    type NotesDataSourcePropertyType,
    type NotesDataSourceListItem,
  } from "$lib/notes/types";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import {
    loadNotesDatabaseView,
    retryNotesDatabaseView,
    readNotesDatabaseView,
    type LoadedNotesDatabaseView,
  } from "$lib/components/notes/editor-component-registry";

  let {
    dataSourceId,
    databaseId,
    initialViewId,
    reloadKeys,
    onSelectPage,
    onEditProperties,
    propertyEditor,
    onLoadPropertyEditor,
    onCreateLinkedDatabaseView,
    onAddProperty,
    onPropertyAction,
    onCopyViewLink,
    onOpenFullPage,
    editingLocked = false,
    onEditingLockChange = () => {},
    onSavingChange = () => {},
    onReady = () => {},
  }: {
    dataSourceId: string;
    databaseId: string | null;
    initialViewId: string | null;
    reloadKeys: Record<NotesDatabaseViewKind, number>;
    onSelectPage: (pageId: string) => void;
    onEditProperties: (anchor?: HTMLElement | null, scope?: NotesDatabaseSourceEditingScope) => void;
    /** Renders the schema fields of one property inside a table column's Edit property submenu. */
    propertyEditor: Snippet<[string]>;
    /** Loads the schema that {@link propertyEditor} edits for a property in the active view's source. */
    onLoadPropertyEditor: (propertyId: string, scope: NotesDatabaseSourceEditingScope) => void;
    onCreateLinkedDatabaseView: () => void;
    onAddProperty: (type: NotesDataSourcePropertyType, name: string, icon: string | null, scope?: NotesDatabaseSourceEditingScope) => Promise<void>;
    onPropertyAction?: (request: NotesDatabasePropertyActionRequest, scope: NotesDatabaseSourceEditingScope) => Promise<void>;
    /** Copies a link that opens this database with the given saved view selected. */
    onCopyViewLink?: (viewId: string) => Promise<void>;
    /** Opens this database alone in the note pane, keeping the selected view. */
    onOpenFullPage?: () => void;
    editingLocked?: boolean;
    onEditingLockChange?: (locked: boolean) => void;
    onSavingChange?: (saving: boolean) => void;
    onReady?: () => void;
  } = $props();

  const { t } = getLocalization();
  const MAX_VISIBLE_VIEWS = 3;
  /** Sources from other databases listed before Show more. */
  const SOURCE_PREVIEW_COUNT = 5;
  const viewIcons = {
    table: Table2,
    board: Columns3,
    gallery: LayoutGrid,
    list: List,
    calendar: CalendarDays,
    timeline: ChartGantt,
  };
  let views = $state<NotesDatabaseView[]>(untrack(() => notesDatabaseSession.read(databaseResource("views", dataSourceId, { databaseId })) ?? []));
  let selectedViewId = $state<string | null>(untrack(() => notesDatabaseSession.recall(databaseId ?? dataSourceId)?.viewId ?? initialViewId));
  let selectionKey = untrack(() => databaseId ?? dataSourceId);
  let viewError = $state<string | null>(null);
  let busy = $state(false);
  let layoutSavingState = $state<{ reporter: (saving: boolean) => void; saving: boolean } | null>(null);

  $effect(() => {
    onSavingChange(busy || layoutSaving);
    return () => onSavingChange(false);
  });

  $effect(() => { if (viewError || viewLoadState?.status === "failed") onReady(); });
  let editingName = $state(false);
  let nameInput: HTMLInputElement | null = $state(null);
  let nameDraft = $state("");
  let viewSearch = $state("");
  let viewSettingsOpen = $state(false);
  /** Set by Edit view or Source in a tab menu so settings open on that page once the menu has closed. */
  let pendingSettings: "view" | "source" | null = null;
  /** Asks the Source row to open its page once settings have rendered it. */
  let sourcePageRequested = $state(false);
  let sourceSettingsRow: ReturnType<typeof CollectionMenu> | undefined = $state();
  let sourceListExpanded = $state(false);
  let settingsAnchor: HTMLButtonElement | null = $state(null);
  let newRowRequestState = $state<{ reporter: (saving: boolean) => void; count: number } | null>(null);
  let templates = $state<NotesDataSourceTemplate[]>(untrack(() => notesDatabaseSession.read(databaseResource("templates", dataSourceId)) ?? []));
  let templateSourceId = $state(untrack(() => dataSourceId));
  let templateError = $state<string | null>(null);
  let pendingDeleteView = $state<NotesDatabaseView | null>(null);
  let lastViewsLoadSignature = $state<string | null>(null);
  let metadataRequest = 0;
  let dataSources = $state<NotesDataSourceListItem[]>([]);
  let sourcesLoading = $state(false);
  let sourceName = $state("");
  let sourceError = $state<string | null>(null);
  let sourceRequest = 0;
  let viewLoadState = $state<LazyComponentLoadState<NotesDatabaseViewKind, LoadedNotesDatabaseView> | null>(untrack(() => {
    const kind = (views.find((view) => view.id === selectedViewId) ?? views[0])?.type ?? "table";
    if (!NOTES_DATABASE_VIEW_KINDS.includes(kind as NotesDatabaseViewKind)) return null;
    const component = readNotesDatabaseView(kind as NotesDatabaseViewKind);
    return component ? { key: component.kind, status: "ready", requestId: 0, component } : null;
  }));

  const supportedViews = $derived(views.filter((view) => NOTES_DATABASE_VIEW_KINDS.includes(view.type as NotesDatabaseViewKind)));
  const selectedView = $derived(supportedViews.find((view) => view.id === selectedViewId) ?? supportedViews[0] ?? null);
  const activeViewKind = $derived((selectedView?.type ?? "table") as NotesDatabaseViewKind);
  const activeViewId = $derived(selectedView?.id ?? null);
  const activeSourceId = $derived(selectedView?.data_source_id ?? dataSourceId);
  const activeSource = $derived(dataSources.find((source) => source.id === activeSourceId));
  const attachedSourceIds = $derived(new Set(views.map((view) => view.data_source_id)));
  const attachedSources = $derived(dataSources.filter((source) => attachedSourceIds.has(source.id)));
  const otherSources = $derived(dataSources.filter((source) => !attachedSourceIds.has(source.id)));
  const sourceQuery = $derived(sourceName.trim().toLocaleLowerCase());
  const matchingAttachedSources = $derived(attachedSources.filter(sourceMatches));
  const matchingOtherSources = $derived(otherSources.filter(sourceMatches));
  const shownOtherSources = $derived(sourceListExpanded ? matchingOtherSources : matchingOtherSources.slice(0, SOURCE_PREVIEW_COUNT));
  const hiddenSourceCount = $derived(matchingOtherSources.length - shownOtherSources.length);
  const layoutInstance = $derived.by(() => {
    const identity = JSON.stringify([activeSourceId, databaseId, activeViewId, activeViewKind]);
    const reportSaving = (saving: boolean): void => {
      if (layoutInstance.reportSaving !== reportSaving) return;
      layoutSavingState = { reporter: reportSaving, saving };
    };
    return { identity, reportSaving };
  });
  const layoutSaving = $derived(layoutSavingState?.reporter === layoutInstance.reportSaving && layoutSavingState.saving);
  const selectionDisabled = $derived(busy || layoutSaving);
  const newRowRequest = $derived(newRowRequestState?.reporter === layoutInstance.reportSaving ? newRowRequestState.count : 0);
  const shownViews = $derived.by(() => {
    if (supportedViews.length <= MAX_VISIBLE_VIEWS) return supportedViews;
    const firstTwo = supportedViews.slice(0, 2);
    return selectedView && !firstTwo.some((view) => view.id === selectedView.id)
      ? [...firstTwo, selectedView]
      : supportedViews.slice(0, MAX_VISIBLE_VIEWS);
  });
  const overflowCount = $derived(Math.max(0, supportedViews.length - shownViews.length));
  const searchableViews = $derived(supportedViews.filter((view) => view.name.toLocaleLowerCase().includes(viewSearch.toLocaleLowerCase())));

  $effect(() => {
    const revision = notesDatabaseSession.revision;
    if (!databaseId || busy) return;
    const signature = `${dataSourceId}:${databaseId}:${revision}`;
    if (signature === lastViewsLoadSignature) return;
    lastViewsLoadSignature = signature;
    void reloadViews(selectedViewId ?? initialViewId, false);
  });

  /** Follow live preselection while retaining the mounted view if its cached entry expires. */
  $effect(() => {
    const key = databaseId ?? dataSourceId;
    const remembered = notesDatabaseSession.recall(key)?.viewId;
    if (selectionKey !== key) {
      selectionKey = key;
      selectedViewId = remembered ?? initialViewId;
    } else if (remembered && remembered !== selectedViewId) selectedViewId = remembered;
  });

  /** Select a view locally and synchronize other renderers of the same database. */
  function selectView(viewId: string, closeSettings = true): void {
    selectedViewId = viewId;
    notesDatabaseSession.selectView(databaseId ?? dataSourceId, viewId);
    if (closeSettings) viewSettingsOpen = false;
  }

  /** Keep toolbar navigation on the current saved view until its pending write settles. */
  function selectViewFromToolbar(viewId: string): void {
    if (selectionDisabled) return;
    selectView(viewId);
  }

  $effect(() => {
    activeViewKind;
    requestActiveView();
  });

  $effect(() => {
    notesDatabaseSession.revision;
    if (busy) return;
    if (viewSettingsOpen) return;
    void loadTemplates(false);
  });

  $effect(() => {
    if (viewSettingsOpen) void loadSources();
    else sourceListExpanded = false;
  });
  // The request lasts until settings close, so a Source row that remounts while settings first load still opens.
  $effect(() => {
    const row = sourceSettingsRow;
    if (!sourcePageRequested || !row) return;
    untrack(() => row.show());
  });
  $effect(() => {
    if (!viewSettingsOpen) sourcePageRequested = false;
  });
  $effect(() => {
    if (templateSourceId === activeSourceId) return;
    templateSourceId = activeSourceId;
    templates = notesDatabaseSession.read(databaseResource("templates", activeSourceId)) ?? [];
    templateError = null;
  });

  async function loadSources(): Promise<void> {
    const request = ++sourceRequest;
    sourcesLoading = true;
    try {
      const loaded = await listNotesDataSources();
      if (request !== sourceRequest) return;
      dataSources = loaded;
      sourceError = null;
    } catch (caught: unknown) {
      if (request === sourceRequest) sourceError = caught instanceof Error ? caught.message : String(caught);
    } finally { if (request === sourceRequest) sourcesLoading = false; }
  }

  /** A source's title, or the untitled label. */
  function sourceTitle(source: NotesDataSourceListItem): string {
    return source.title || t("notes.databaseSourcesUnnamed");
  }

  /** Where a source's database lives, from its project group down to the page that holds it, with untitled pages labeled. */
  function sourceLocationTitles(source: NotesDataSourceListItem): string[] {
    return source.location.map((title) => title || t("notes.untitled"));
  }

  /** A source's full location as one line, such as "Personal / Productivity / Tasks". */
  function sourceLocation(source: NotesDataSourceListItem): string {
    return sourceLocationTitles(source).join(" / ");
  }

  /** Whether a source's title or location contains the Source page query. */
  function sourceMatches(source: NotesDataSourceListItem): boolean {
    if (!sourceQuery) return true;
    return sourceTitle(source).toLocaleLowerCase().includes(sourceQuery) || sourceLocation(source).toLocaleLowerCase().includes(sourceQuery);
  }

  async function manageSource(sourceId?: string): Promise<void> {
    if (!databaseId || selectionDisabled || editingLocked || (!sourceId && !sourceName.trim())) return;
    busy = true;
    sourceError = null;
    try {
      const result = sourceId
        ? await attachNotesDataSource({ data_source_id: sourceId, database_id: databaseId, view_id: crypto.randomUUID(), view_name: viewLabel("table") })
        : await createNotesDataSource({ id: crypto.randomUUID(), database_id: databaseId, view_id: crypto.randomUUID(), title: sourceName.trim(), view_name: viewLabel("table") });
      await reloadViews(result.view.id);
      sourceName = "";
      await loadSources();
    } catch (caught: unknown) { sourceError = caught instanceof Error ? caught.message : String(caught); }
    finally { busy = false; }
  }

  async function toggleEditingLock(): Promise<void> {
    if (!databaseId || selectionDisabled) return;
    busy = true;
    viewError = null;
    try {
      const reference = await setNotesDatabaseEditingLock(databaseId, !editingLocked);
      onEditingLockChange(reference.editing_locked);
    } catch (caught: unknown) { viewError = caught instanceof Error ? caught.message : String(caught); }
    finally { busy = false; }
  }

  function editingScope(): NotesDatabaseSourceEditingScope {
    return { dataSourceId: activeSourceId, databaseId, viewId: activeViewId };
  }

  async function loadTemplates(force = true): Promise<void> {
    const sourceId = activeSourceId;
    try {
      const loaded = await notesDatabaseSession.load(databaseResource("templates", sourceId), () => listNotesDataSourceTemplates(sourceId), force);
      if (sourceId !== activeSourceId) return;
      templates = loaded;
      templateError = null;
    } catch (caught) {
      if (sourceId !== activeSourceId) return;
      templateError = caught instanceof Error ? caught.message : String(caught);
    }
  }

  async function reloadViews(preferredId: string | null, force = true): Promise<void> {
    if (!databaseId) return;
    const request = ++metadataRequest;
    const sourceId = dataSourceId;
    const shellId = databaseId;
    const previousSelection = selectedViewId;
    try {
      const loaded = await notesDatabaseSession.load(databaseResource("views", sourceId, { databaseId: shellId }), () => listNotesDatabaseViews(shellId), force);
      if (request !== metadataRequest) return;
      views = loaded;
      const preferred = selectedViewId !== previousSelection ? selectedViewId : preferredId;
      const nextViewId = loaded.some((view) => view.id === preferred)
        ? preferred
        : loaded.some((view) => view.id === selectedViewId)
          ? selectedViewId
          : loaded[0]?.id ?? null;
      if (nextViewId) selectView(nextViewId, false);
      viewError = null;
    } catch (caught) {
      if (request !== metadataRequest) return;
      viewError = caught instanceof Error ? caught.message : String(caught);
    }
  }

  function requestActiveView(retry = false): void {
    if (!selectedView) return;
    if (!retry && viewLoadState?.key === activeViewKind) return;
    const kind = activeViewKind;
    const component = readNotesDatabaseView(kind);
    if (component) {
      viewLoadState = { key: kind, status: "ready", requestId: (viewLoadState?.requestId ?? 0) + 1, component };
      return;
    }
    const loadingState = beginLazyComponentLoad(viewLoadState, kind);
    viewLoadState = loadingState;
    const request = retry ? retryNotesDatabaseView(kind) : loadNotesDatabaseView(kind);
    void request.then((component) => {
      if (!viewLoadState) return;
      viewLoadState = resolveLazyComponentLoad(viewLoadState, kind, loadingState.requestId, component);
    }).catch((error: unknown) => {
      if (!viewLoadState) return;
      viewLoadState = rejectLazyComponentLoad(viewLoadState, kind, loadingState.requestId, error);
      console.error(`load Notes ${kind} database view failed`, error);
    });
  }

  function viewLabel(view: NotesDatabaseViewKind): string {
    if (view === "table") return t("notes.databaseViewTable");
    if (view === "board") return t("notes.databaseViewBoard");
    if (view === "gallery") return t("notes.databaseViewGallery");
    if (view === "list") return t("notes.databaseViewList");
    if (view === "calendar") return t("notes.databaseViewCalendar");
    return t("notes.databaseViewTimeline");
  }

  async function seedView(kind: NotesDatabaseViewKind): Promise<NotesDatabaseView> {
    const scope = { databaseId, viewId: null };
    if (kind === "table") return (await getNotesDataSourceTableView(activeSourceId, scope)).view;
    if (kind === "board") return (await getNotesDataSourceBoardView(activeSourceId, scope)).view;
    if (kind === "gallery") return (await getNotesDataSourceGalleryView(activeSourceId, scope)).view;
    if (kind === "list") return (await getNotesDataSourceListView(activeSourceId, scope)).view;
    if (kind === "calendar") return (await getNotesDataSourceCalendarView(activeSourceId, scope)).view;
    return (await getNotesDataSourceTimelineView(activeSourceId, scope)).view;
  }

  async function addView(kind: NotesDatabaseViewKind): Promise<void> {
    if (!databaseId || selectionDisabled || editingLocked) return;
    busy = true;
    viewError = null;
    try {
      const existing = supportedViews.find((view) => view.type === kind && view.data_source_id === activeSourceId);
      const created = existing
        ? await duplicateNotesDatabaseView({ id: crypto.randomUUID(), database_id: databaseId, source_view_id: existing.id, name: t("notes.databaseViewCopyName", existing.name) })
        : await seedView(kind);
      await reloadViews(created.id);
    } catch (caught) {
      viewError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      busy = false;
    }
  }

  async function duplicateView(): Promise<void> {
    const current = selectedView;
    if (!databaseId || !current || selectionDisabled || editingLocked) return;
    busy = true;
    viewError = null;
    try {
      const duplicate = await duplicateNotesDatabaseView({
        id: crypto.randomUUID(), database_id: databaseId, source_view_id: current.id,
        name: t("notes.databaseViewCopyName", current.name),
      });
      await reloadViews(duplicate.id);
    } catch (caught) {
      viewError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      busy = false;
    }
  }

  async function saveName(): Promise<void> {
    const current = selectedView;
    const name = nameDraft.trim();
    editingName = false;
    if (!databaseId || !current || busy || editingLocked) return;
    if (!name || name === current.name) {
      nameDraft = current.name;
      return;
    }
    busy = true;
    viewError = null;
    try {
      await renameNotesDatabaseView(databaseId, current.id, name);
      await reloadViews(current.id);
    } catch (caught) {
      viewError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      busy = false;
    }
  }

  async function deleteView(): Promise<void> {
    const current = pendingDeleteView;
    pendingDeleteView = null;
    if (!databaseId || !current || selectionDisabled || editingLocked || !viewDeletable(current)) return;
    busy = true;
    viewError = null;
    try {
      await deleteNotesDatabaseView(databaseId, current.id);
      notesDatabaseViewTabDisplays.set(current.id, null);
      await reloadViews(null);
    } catch (caught) {
      viewError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      busy = false;
    }
  }

  /**
   * Mark the switch inside a settings row so the settings panel arrow keys reach it like other rows.
   *
   * @param node Row element that contains the switch.
   */
  function settingsRowSwitch(node: HTMLElement): void {
    node.querySelector<HTMLButtonElement>("button[role=\"switch\"]")?.setAttribute("data-collection-settings-row", "");
  }

  /**
   * Decide whether a view tab opens its menu. A press on an inactive tab only selects it, while a press on the active tab
   * or a context menu on any tab opens the menu for that view.
   *
   * @param viewId View whose tab was pressed.
   * @param source Whether a press or a context menu asked to open.
   */
  function openViewMenu(viewId: string, source: "press" | "context"): boolean {
    if (viewId === activeViewId) return true;
    if (selectionDisabled) return false;
    selectView(viewId);
    return source === "context";
  }

  /**
   * Load sources for the Source summary while a view menu is open, and open view settings only after the menu that
   * asked for them has closed, so its returning focus does not dismiss them.
   *
   * @param open Whether the view menu is now open.
   */
  function viewMenuOpenChanged(open: boolean): void {
    if (open) {
      void loadSources();
      return;
    }
    editingName = false;
    if (!pendingSettings) return;
    sourcePageRequested = pendingSettings === "source";
    pendingSettings = null;
    viewSettingsOpen = true;
  }

  /** Whether deleting a view keeps at least one view and one table for its source. */
  function viewDeletable(view: NotesDatabaseView): boolean {
    if (supportedViews.length < 2) return false;
    return view.type !== "table" || supportedViews.filter((candidate) => candidate.data_source_id === view.data_source_id && candidate.type === "table").length >= 2;
  }

  async function copyViewLink(viewId: string): Promise<void> {
    try {
      await onCopyViewLink?.(viewId);
    } catch (caught: unknown) { viewError = caught instanceof Error ? caught.message : String(caught); }
  }

  function tabDisplayLabel(display: NotesDatabaseViewTabDisplay): string {
    if (display === "text") return t("notes.databaseViewDisplayText");
    if (display === "icon") return t("notes.databaseViewDisplayIcon");
    return t("notes.databaseViewDisplayTextAndIcon");
  }

  function beginRename(): void {
    if (selectionDisabled || editingLocked) return;
    nameDraft = selectedView?.name ?? "";
    editingName = true;
    void tick().then(() => nameInput?.focus());
  }

  async function createNewRow(): Promise<void> {
    if (activeViewKind === "table") {
      newRowRequestState = { reporter: layoutInstance.reportSaving, count: newRowRequest + 1 };
      return;
    }
    if (busy) return;
    busy = true;
    viewError = null;
    try {
      const created = await createNotesDataSourceRowPage(activeSourceId, {
        id: crypto.randomUUID(),
        first_block_id: crypto.randomUUID(),
        title: t("notes.untitled"),
      });
      onSelectPage(created.page.id);
    } catch (caught) {
      viewError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      busy = false;
    }
  }

  async function createFromTemplate(templateId: string): Promise<void> {
    if (busy) return;
    busy = true;
    viewError = null;
    try {
      const created = await applyNotesDataSourceTemplate(activeSourceId, templateId, { title: t("notes.untitled") });
      onSelectPage(created.page.id);
    } catch (caught) {
      viewError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      busy = false;
    }
  }

  function openTemplateSettings(): void {
    if (selectionDisabled) return;
    const tableView = supportedViews.find((view) => view.type === "table" && view.data_source_id === activeSourceId);
    if (!tableView) return;
    selectView(tableView.id);
    viewSettingsOpen = true;
  }

  /** Open the full source schema editor beside the database toolbar. */
  function editProperties(): void {
    if (editingLocked) return;
    viewSettingsOpen = false;
    onEditProperties(settingsAnchor, editingScope());
  }
</script>

{#snippet settingsHeader()}
  {#if selectedView}
    {@const Icon = viewIcons[selectedView.type as NotesDatabaseViewKind]}
    <div class="flex min-w-0 items-center gap-1.5 pb-1.5">
      <span class="inline-flex size-8 shrink-0 items-center justify-center rounded-floating-item border border-border text-muted-foreground">
        <Icon class="size-4" strokeWidth={1.75} aria-hidden="true" />
      </span>
      <input class="field min-w-0 flex-1" aria-label={t("notes.databaseViewName")} value={selectedView.name} disabled={selectionDisabled || editingLocked}
        onblur={(event) => { nameDraft = event.currentTarget.value; void saveName(); }}
        onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); event.currentTarget.blur(); } }} />
    </div>
  {/if}
  {@render sourcePage()}
  <label class="flex min-h-(--panel-row-height) w-full min-w-0 cursor-pointer items-center gap-2 px-2" aria-busy={selectionDisabled} use:settingsRowSwitch>
    {#if editingLocked}<Lock class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />{:else}<LockOpen class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />{/if}
    <span class="min-w-0 flex-1 truncate">{t("notes.databaseLockLayout")}</span>
    <Switch size="compact" checked={editingLocked} ariaLabel={t("notes.databaseEditingLock")} onChange={() => { void toggleEditingLock(); }} />
  </label>
  {#if editingLocked}<p class="px-2 py-1 text-muted-foreground">{t("notes.databaseEditingLockDescription")}</p>{/if}
  <CollectionMenuSeparator />
{/snippet}

{#snippet viewMenu(view: NotesDatabaseView)}
  {@const display = notesDatabaseViewTabDisplays.get(view.id)}
  {#if editingName}
    <input bind:this={nameInput} class="field w-full" aria-label={t("notes.databaseViewName")} bind:value={nameDraft} onkeydown={(event) => { event.stopPropagation(); if (event.key === "Enter") void saveName(); if (event.key === "Escape") { nameDraft = view.name; editingName = false; } }} onblur={() => { if (editingName) void saveName(); }} />
  {:else}
    <div class="grid gap-0">
      <button data-collection-menu-keep-open type="button" class="menu-item min-w-0" disabled={editingLocked} onclick={beginRename}>
        <Pencil class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" /><span class="min-w-0 flex-1 truncate">{t("notes.databaseViewRename")}</span>
      </button>
      <CollectionMenu fullWidth dismissOnAction label={t("notes.databaseViewDisplayAs")} icon={Paintbrush}>
        {#each NOTES_DATABASE_VIEW_TAB_DISPLAYS as option (option)}
          <CollectionMenuItem label={tabDisplayLabel(option)} checked={display === option} onclick={() => notesDatabaseViewTabDisplays.set(view.id, option)} />
        {/each}
        <CollectionMenuSeparator />
        <p class="px-2 py-1 text-muted-foreground">{t("notes.databaseViewDisplayPersonal")}</p>
      </CollectionMenu>
      <CollectionMenuItem icon={SlidersHorizontal} label={t("notes.databaseViewEdit")} onclick={() => { pendingSettings = "view"; }} />
      <button type="button" class="menu-item min-w-0" disabled={selectionDisabled} onclick={() => { pendingSettings = "source"; }}>
        <Database class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" /><span class="min-w-0 flex-1 truncate">{t("notes.databaseViewSource")}</span>
        {#if activeSource}<span class="max-w-32 truncate text-muted-foreground">{sourceTitle(activeSource)}</span>{/if}
        <ChevronRight class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      </button>
      {#if onCopyViewLink || onOpenFullPage}
        <CollectionMenuSeparator />
        {#if onCopyViewLink}<CollectionMenuItem icon={Link} label={t("notes.databaseViewCopyLink")} onclick={() => { void copyViewLink(view.id); }} />{/if}
        {#if onOpenFullPage}<CollectionMenuItem icon={ArrowUpRight} label={t("notes.databaseViewOpenFullPage")} onclick={onOpenFullPage} />{/if}
      {/if}
      <CollectionMenuSeparator />
      <CollectionMenuItem icon={Copy} label={t("notes.databaseViewDuplicate")} disabled={editingLocked} onclick={() => { void duplicateView(); }} />
      <CollectionMenuItem icon={Trash2} label={t("notes.databaseViewDelete")} destructive disabled={editingLocked || !viewDeletable(view)} onclick={() => { pendingDeleteView = view; }} />
    </div>
  {/if}
{/snippet}

{#snippet sourcePage()}
  <CollectionMenu bind:this={sourceSettingsRow} fullWidth label={t("notes.databaseViewSource")} summary={activeSource ? sourceTitle(activeSource) : ""} icon={Database} disabled={selectionDisabled}>
    <input class="field mb-1 w-full min-w-0" aria-label={t("notes.databaseSourceSearch")} placeholder={t("notes.databaseSourceSearch")}
      bind:value={sourceName} disabled={selectionDisabled} onkeydown={(event) => event.stopPropagation()} />
    {#if sourceError}<p class="px-2 py-1 text-destructive" role="alert">{sourceError}</p>{/if}
    {#if sourcesLoading && dataSources.length === 0}<p class="px-2 py-1 text-muted-foreground" role="status">{t("notes.databaseSourcesLoading")}</p>{/if}
    {#if matchingAttachedSources.length > 0}
      <p class="menu-label">{t("notes.databaseSourcesAttached")}</p>
      {#each matchingAttachedSources as source (source.id)}
        <CollectionMenuItem icon={Database} label={sourceTitle(source)} detail={t(source.parent.database_id === databaseId ? "notes.databaseSourceOwned" : "notes.databaseSourceLinked")}
          checked={source.id === activeSourceId} disabled={selectionDisabled} onclick={() => {
            const view = supportedViews.find((candidate) => candidate.data_source_id === source.id && candidate.type === activeViewKind)
              ?? supportedViews.find((candidate) => candidate.data_source_id === source.id);
            if (view) selectViewFromToolbar(view.id);
          }} />
      {/each}
    {/if}
    {#if !sourcesLoading || dataSources.length > 0}
      <p class="menu-label">{t("notes.databaseSourceAttach")}</p>
      {#each shownOtherSources as source (source.id)}
        {@const location = sourceLocation(source)}
        <button type="button" class="menu-item min-w-0" data-collection-settings-row data-collection-row-two-line title={location || undefined}
          disabled={selectionDisabled || editingLocked} onclick={() => { void manageSource(source.id); }}>
          <Database class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />
          <span class="relative grid min-w-0 flex-1">
            <span class="truncate">{sourceTitle(source)}</span>
            {#if location}
              <span class="sr-only">{location}</span>
              <span class="flex min-w-0 text-panel-detail text-muted-foreground" aria-hidden="true">
                {#each compactSourceLocation(sourceLocationTitles(source)) as segment, index (index)}
                  {#if index > 0}<span class="shrink-0 px-1">/</span>{/if}
                  {#if segment.kind === "gap"}
                    <span class="shrink-0">…</span>
                  {:else}
                    <span class={["min-w-8 truncate", segment.last ? "shrink" : "shrink-4"]}>{segment.text}</span>
                  {/if}
                {/each}
              </span>
            {/if}
          </span>
        </button>
      {/each}
      {#if hiddenSourceCount > 0}
        <button type="button" class="menu-item min-w-0 text-muted-foreground" data-collection-settings-row onclick={() => { sourceListExpanded = true; }}>
          <Ellipsis class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" /><span class="min-w-0 flex-1 truncate">{t("notes.databaseSourceShowMore", hiddenSourceCount)}</span>
        </button>
      {/if}
      {#if matchingOtherSources.length === 0}
        <p class="px-2 py-1 text-muted-foreground">{t(sourceQuery ? "notes.databaseSourceNoMatches" : "notes.databaseSourceNoOtherSources")}</p>
      {/if}
    {/if}
    {#if sourceName.trim()}
      <CollectionMenuSeparator />
      <CollectionMenuItem icon={Plus} label={t("notes.databaseSourceCreateNamed", sourceName.trim())} disabled={selectionDisabled || editingLocked} onclick={() => { void manageSource(); }} />
    {/if}
  </CollectionMenu>
{/snippet}

<div class="flex min-w-0 flex-wrap items-center gap-1 py-1">
  <div class="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
    {#each shownViews as view (view.id)}
      {@const Icon = viewIcons[view.type as NotesDatabaseViewKind]}
      {@const active = activeViewId === view.id}
      {@const display = notesDatabaseViewTabDisplays.get(view.id)}
      <CollectionMenu label={view.name} kind="actions" showHeader={false} contextMenu pressed={active} iconOnly={display === "icon"} rootClass="shrink-0"
        triggerClass={cn("max-w-44 rounded-md text-[0.866667rem] font-medium", active && "bg-accent text-foreground")} disabled={selectionDisabled}
        beforeOpen={(source) => openViewMenu(view.id, source)} onOpenChange={viewMenuOpenChanged}>
        {#snippet leading()}
          {#if display !== "text"}<Icon class="size-4 shrink-0" strokeWidth={1.75} aria-hidden="true" />{/if}
        {/snippet}
        {@render viewMenu(view)}
      </CollectionMenu>
    {/each}
    {#if overflowCount > 0}
      <CollectionMenu label={t("notes.databaseMoreViews", overflowCount)} kind="actions" showHeader={false} dismissOnAction disabled={selectionDisabled}>
        <input class="field mb-1.5 w-full" aria-label={t("notes.databaseSearchViews")} placeholder={t("notes.databaseSearchViews")} bind:value={viewSearch} />
        <div class="grid gap-0">
          {#each searchableViews as view (view.id)}
            {@const Icon = viewIcons[view.type as NotesDatabaseViewKind]}
            <button type="button" class="menu-item" disabled={selectionDisabled} onclick={() => selectViewFromToolbar(view.id)}>
              <Icon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" /><span class="min-w-0 flex-1 truncate">{view.name}</span>
            </button>
          {/each}
        </div>
      </CollectionMenu>
    {/if}
    <CollectionMenu label={t("notes.databaseAddView")} kind="new" iconOnly showHeader={false} dismissOnAction disabled={selectionDisabled || editingLocked}>
      <p class="menu-label">{t("notes.databaseAddView")}</p>
      <div class="grid gap-0">
        {#each NOTES_DATABASE_VIEW_KINDS as kind}
          {@const Icon = viewIcons[kind]}
          <button type="button" class="menu-item text-foreground" disabled={selectionDisabled || editingLocked} onclick={() => { void addView(kind); }}>
            <Icon class="size-4 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" /><span>{viewLabel(kind)}</span>
          </button>
        {/each}
      </div>
      <div class="menu-separator" role="separator"></div>
      <div>
        <button type="button" class="menu-item" onclick={onCreateLinkedDatabaseView}>{t("notes.databaseLinkedViewCreate")}</button>
      </div>
    </CollectionMenu>
  </div>
  <button bind:this={settingsAnchor} type="button" class="inline-flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" class:bg-accent={viewSettingsOpen} aria-label={t("notes.databaseViewSettings")} aria-haspopup="dialog" aria-expanded={viewSettingsOpen} title={t("notes.databaseViewSettings")} onclick={() => { viewSettingsOpen = !viewSettingsOpen; }}><SlidersHorizontal class="size-4" aria-hidden="true" /></button>
  <div class="inline-flex shrink-0 items-center rounded-md bg-primary">
    <button type="button" class="min-h-8 rounded-l-md px-3 text-[0.866667rem] font-medium text-primary-foreground hover:bg-primary/90" disabled={busy} onclick={() => { void createNewRow(); }}>{t("notes.databaseNew")}</button>
    <span class="h-5 w-px bg-primary-foreground/25" aria-hidden="true"></span>
    <CollectionMenu label={t("notes.databaseNewOptions")} kind="new-options" iconOnly primary showHeader={false} dismissOnAction>
      <p class="menu-label">{t("notes.databaseTemplatesTitle")}</p>
      {#if templateError}<p class="mb-2 px-2 text-panel-detail text-destructive" role="alert">{templateError}</p>{/if}
      {#if templates.length === 0}
        <p class="mb-2 px-2 text-panel-detail text-muted-foreground">{t("notes.databaseTemplatesEmpty")}</p>
      {:else}
        <div class="grid gap-0">
          {#each templates as template (template.id)}
            <button type="button" class="menu-item" disabled={busy} onclick={() => { void createFromTemplate(template.id); }}>{template.name}</button>
          {/each}
        </div>
      {/if}
      <div class="menu-separator" role="separator"></div>
      <div>
        <button type="button" class="menu-item" disabled={selectionDisabled} onclick={openTemplateSettings}>{t("notes.databaseTemplatesManage")}</button>
      </div>
    </CollectionMenu>
  </div>
</div>

{#if pendingDeleteView}
  <ConfirmDialog
    message={t("notes.databaseViewDeleteConfirm", pendingDeleteView.name)}
    confirmLabel={t("notes.databaseViewDelete")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { void deleteView(); }}
    onCancel={() => { pendingDeleteView = null; }}
  />
{/if}

{#if viewError}
  <div class="my-2 flex items-center gap-2 text-[0.8rem] text-destructive" role="alert">
    <span>{viewError}</span>
    {#if !selectedView}
      <button type="button" class="rounded-md px-2 py-1 hover:bg-destructive/10" onclick={() => { void reloadViews(initialViewId); }}>{t("common.retry")}</button>
    {/if}
  </div>
{/if}

{#key layoutInstance.identity}
{@const reportLayoutSaving = untrack(() => layoutInstance.reportSaving)}
{#if selectedView && viewLoadState?.status === "ready" && viewLoadState.key === activeViewKind && viewLoadState.component.kind === "table"}
  {@const NotesDatabaseTableView = viewLoadState.component.component}
  <NotesDatabaseTableView {onReady} onSavingChange={reportLayoutSaving} dataSourceId={activeSourceId} {databaseId} viewId={activeViewId} {onSelectPage} {editingLocked} onAddProperty={(type, name, icon) => onAddProperty(type, name, icon, editingScope())} onPropertyAction={onPropertyAction ? (request) => onPropertyAction?.(request, editingScope()) ?? Promise.resolve() : undefined} {newRowRequest} settingsOpen={viewSettingsOpen} {settingsAnchor} {settingsHeader} onCloseSettings={() => { viewSettingsOpen = false; }} onEditProperties={editProperties} {propertyEditor} onLoadPropertyEditor={(propertyId) => onLoadPropertyEditor(propertyId, editingScope())} reloadKey={reloadKeys.table} />
{:else if selectedView && viewLoadState?.status === "ready" && viewLoadState.key === activeViewKind && viewLoadState.component.kind === "board"}
  {@const NotesDatabaseBoardView = viewLoadState.component.component}
  <NotesDatabaseBoardView {onReady} onSavingChange={reportLayoutSaving} settingsOpen={viewSettingsOpen} {settingsAnchor} {settingsHeader} onCloseSettings={() => { viewSettingsOpen = false; }} onEditProperties={editProperties} dataSourceId={activeSourceId} {databaseId} {editingLocked} viewId={activeViewId} {onSelectPage} reloadKey={reloadKeys.board} />
{:else if selectedView && viewLoadState?.status === "ready" && viewLoadState.key === activeViewKind && viewLoadState.component.kind === "gallery"}
  {@const NotesDatabaseGalleryView = viewLoadState.component.component}
  <NotesDatabaseGalleryView {onReady} onSavingChange={reportLayoutSaving} settingsOpen={viewSettingsOpen} {settingsAnchor} {settingsHeader} onCloseSettings={() => { viewSettingsOpen = false; }} onEditProperties={editProperties} dataSourceId={activeSourceId} {databaseId} {editingLocked} viewId={activeViewId} {onSelectPage} reloadKey={reloadKeys.gallery} />
{:else if selectedView && viewLoadState?.status === "ready" && viewLoadState.key === activeViewKind && viewLoadState.component.kind === "list"}
  {@const NotesDatabaseListView = viewLoadState.component.component}
  <NotesDatabaseListView {onReady} onSavingChange={reportLayoutSaving} settingsOpen={viewSettingsOpen} {settingsAnchor} {settingsHeader} onCloseSettings={() => { viewSettingsOpen = false; }} onEditProperties={editProperties} dataSourceId={activeSourceId} {databaseId} {editingLocked} viewId={activeViewId} {onSelectPage} reloadKey={reloadKeys.list} />
{:else if selectedView && viewLoadState?.status === "ready" && viewLoadState.key === activeViewKind && viewLoadState.component.kind === "calendar"}
  {@const NotesDatabaseCalendarView = viewLoadState.component.component}
  <NotesDatabaseCalendarView {onReady} onSavingChange={reportLayoutSaving} settingsOpen={viewSettingsOpen} {settingsAnchor} {settingsHeader} onCloseSettings={() => { viewSettingsOpen = false; }} onEditProperties={editProperties} dataSourceId={activeSourceId} {databaseId} {editingLocked} viewId={activeViewId} {onSelectPage} reloadKey={reloadKeys.calendar} />
{:else if selectedView && viewLoadState?.status === "ready" && viewLoadState.key === activeViewKind && viewLoadState.component.kind === "timeline"}
  {@const NotesDatabaseTimelineView = viewLoadState.component.component}
  <NotesDatabaseTimelineView settingsOpen={viewSettingsOpen} {settingsAnchor} {settingsHeader} onCloseSettings={() => { viewSettingsOpen = false; }} onEditProperties={editProperties} {onReady} onSavingChange={reportLayoutSaving} dataSourceId={activeSourceId} {databaseId} {editingLocked} viewId={activeViewId} {onSelectPage} reloadKey={reloadKeys.timeline} />
{:else if viewLoadState?.status === "failed" && viewLoadState.key === activeViewKind}
  <div class="my-3 rounded-md border border-destructive/40 p-3 text-[0.8rem] text-destructive" role="alert"><p>{t("common.viewLoadFailed", viewLabel(activeViewKind))}</p><button class="mt-2 min-h-8 rounded-md border border-border px-2 text-foreground hover:bg-accent" type="button" onclick={() => requestActiveView(true)}>{t("common.retry")}</button></div>
{:else}
  <NotesLoadingSkeleton kind={activeViewKind} />
{/if}
{/key}
