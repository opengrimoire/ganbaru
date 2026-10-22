<script lang="ts">
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import Copy from "@lucide/svelte/icons/copy";
  import Eye from "@lucide/svelte/icons/eye";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Trash2 from "@lucide/svelte/icons/trash";
  import { tick, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    clickNotesDataSourceButton,
    createNotesDataSourceRowPage,
    createNotesDataSourceSubitem,
    getNotesDataSourceRowPageContext,
    listNotesDataSources,
    updateNotesDataSourceRowParent,
    updateNotesDataSourceRowProperty,
  } from "$lib/api/notes";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionPropertyDraft from "$lib/components/collections/CollectionPropertyDraft.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import CollectionPropertyNameField from "$lib/components/collections/CollectionPropertyNameField.svelte";
  import CollectionPropertyTypeMenu from "$lib/components/collections/CollectionPropertyTypeMenu.svelte";
  import NotesDatabasePropertyEditor from "$lib/components/notes/database/NotesDatabasePropertyEditor.svelte";
  import NotesDatabasePropertySchemaEditor from "$lib/components/notes/database/NotesDatabasePropertySchemaEditor.svelte";
  import { notesPropertyHasSchemaOptions, notesPropertyKind, notesPropertyTypeOptions } from "$lib/components/notes/database/property-kinds";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabaseCellCaret, notesDatabaseCellKeepsKey } from "$lib/notes/database/cell-keyboard";
  import {
    NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS,
    notesDataSourceSchemaDraftFromDto,
    type NotesDataSourcePropertyUpdate,
    type NotesDataSourceSchemaPropertyDraft,
    type NotesDatabaseSourceEditingScope,
  } from "$lib/notes/database/data-source-schema";
  import {
    notesDataSourceAddOption,
    notesDataSourceVisibleOptions,
    type NotesDataSourceOptionEdit,
    type NotesDataSourceOptionEditor,
  } from "$lib/notes/database/property-options";
  import {
    appendNotesDataSourceSchemaProperty,
    changeNotesDataSourceSchemaPropertyIcon,
    changeNotesDataSourceSchemaPropertyType,
    deleteNotesDataSourceSchemaProperty,
    duplicateNotesDataSourceSchemaProperty,
    renameNotesDataSourceSchemaProperty,
    updateNotesDataSourceSchemaDrafts,
    updateNotesDataSourceSchemaProperty,
    type NotesDataSourceSchemaActionContext,
    type NotesDataSourceSchemaActionResult,
  } from "$lib/notes/database/property-schema-actions";
  import type { NotesDataSourceRowPageContext } from "$lib/notes/database/row-page-context";
  import {
    moveNotesDatabaseRowPageProperty,
    notesDatabaseRowPageEntries,
    notesDatabaseRowPageValueIsEmpty,
    orderNotesDatabaseRowPageProperties,
    stepNotesDatabaseRowPageProperty,
    withNotesDatabaseRowPagePositions,
  } from "$lib/notes/database/row-page-properties";
  import { notesDatabaseSession } from "$lib/notes/database/session.svelte";
  import {
    notesDatabaseRowTitle,
    notesDatabaseTableCellEditValue,
    notesDatabaseTableColumns,
    notesDatabaseTableEditValuesEqual,
    type NotesDatabaseTableColumn,
    type NotesDatabaseTableEditValue,
  } from "$lib/notes/database/table";
  import { notesDatabaseViewDropPlacement, orderNotesDatabaseViews, type NotesDatabaseViewDropPlacement } from "$lib/notes/database/view-order";
  import type { NotesDataSource, NotesDataSourcePageVisibility, NotesDataSourcePropertyType, NotesPage } from "$lib/notes/types";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import NotesDatabaseRowPageOptionValue from "./NotesDatabaseRowPageOptionValue.svelte";
  import NotesDatabaseRowPageRelations from "./NotesDatabaseRowPageRelations.svelte";
  import NotesDatabaseRowPageRelationValue from "./NotesDatabaseRowPageRelationValue.svelte";

  /**
   * The properties of a database row, listed below its page title: each name opens the property menu and each value
   * edits in place with the editor its table cell uses. Properties set to hide fold under "N more properties".
   *
   * Values save immediately. Structure changes (names, types, visibility, order, new properties) follow the database
   * layout lock, while values stay editable under it.
   */
  let { dataSourceId, pageId, onOpenPage }: {
    dataSourceId: string;
    pageId: string;
    /** Opens another row, such as the parent item or a sub-item. */
    onOpenPage: (pageId: string) => void;
  } = $props();

  interface ConfirmRequest {
    title: string;
    message: string;
    confirmLabel: string;
    danger: boolean;
    resolve: (confirmed: boolean) => void;
  }

  const PAGE_VISIBILITIES: readonly NotesDataSourcePageVisibility[] = ["always_show", "hide_when_empty", "always_hide"];
  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const propertyTypeOptions = $derived(notesPropertyTypeOptions(t));

  let root: HTMLElement | undefined = $state();
  let addButton: HTMLButtonElement | undefined = $state();
  let context = $state<NotesDataSourceRowPageContext | null>(null);
  let loadError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let expanded = $state(false);
  let adding = $state(false);
  let availableDataSources = $state<NotesDataSource[]>([]);
  let pendingConfirm = $state<ConfirmRequest | null>(null);
  let pendingSaves = $state(0);
  /** Parent and sub-item saves in flight, which hold reloads like value saves do. */
  let relationSaves = $state(0);
  /** A row page order being saved, shown until the saved schema arrives. */
  let optimisticOrder = $state<string[] | null>(null);
  /** A property just added, whose value editor takes focus once it renders. */
  let focusPropertyId = $state<string | null>(null);
  let dragged = $state<string | null>(null);
  let dropTarget = $state<{ propertyId: string; placement: NotesDatabaseViewDropPlacement } | null>(null);
  /** Properties whose value is saving. Only their editors are disabled, so focus can move on meanwhile. */
  const savingIds = new SvelteSet<string>();
  /** Value editors holding unsaved text, which a reload must not replace. */
  const editingIds = new SvelteSet<string>();
  /** Hide-when-empty properties edited here, kept on screen when cleared until the page opens again. */
  const keptIds = new SvelteSet<string>();
  let saveQueue: Promise<void> = Promise.resolve();
  let dataSourcesRequested = false;
  let loadRequest = 0;
  /** Counts writes, so a read that started before a write finished is read again instead of shown. */
  let writeEpoch = 0;
  /** Set when a reload waits for open editors and saves to finish. */
  let stale = false;

  const locked = $derived(context?.editing_locked ?? false);
  const drafts = $derived(context ? notesDataSourceSchemaDraftFromDto(context.schema.data_source, context.schema.view) : []);
  const columns = $derived(new Map((context ? notesDatabaseTableColumns(context.schema.data_source, context.schema.view) : [])
    .map((column) => [column.id, pageColumn(column)])));
  const orderedIds = $derived(orderNotesDatabaseRowPageProperties(drafts).map((property) => property.id));
  const entries = $derived.by(() => {
    const row = context?.row;
    if (!row) return [];
    const listed = orderNotesDatabaseRowPageProperties(drafts).filter((property) => columns.has(property.id));
    const arranged = optimisticOrder ? orderNotesDatabaseViews(listed, optimisticOrder) : listed;
    return notesDatabaseRowPageEntries(arranged, (property) => {
      const column = columns.get(property.id);
      return !keptIds.has(property.id) && (!column || notesDatabaseRowPageValueIsEmpty(row, column));
    });
  });
  const hiddenCount = $derived(entries.filter((entry) => entry.collapsed).length);
  const shownEntries = $derived(expanded ? entries : entries.filter((entry) => !entry.collapsed));
  const shownIds = $derived(shownEntries.map((entry) => entry.property.id));
  const relationDataSources = $derived(context
    ? [context.schema.data_source, ...availableDataSources.filter((source) => source.id !== context?.schema.data_source.id)]
    : availableDataSources);

  $effect(() => {
    notesDatabaseSession.revision;
    void dataSourceId;
    void pageId;
    untrack(refresh);
  });

  $effect(() => {
    const propertyId = focusPropertyId;
    if (!propertyId || !shownIds.includes(propertyId)) return;
    focusPropertyId = null;
    void tick().then(() => {
      root?.querySelector<HTMLElement>(`[data-row-page-property="${CSS.escape(propertyId)}"] [data-table-cell="true"]`)?.focus();
    });
  });

  $effect(() => () => { pendingConfirm?.resolve(false); });

  /** Relation values name their target source in the picker, which needs the source list when it is another one. */
  $effect(() => {
    const ownId = context?.schema.data_source.id;
    if ([...columns.values()].some((column) => column.relationDataSourceId && column.relationDataSourceId !== ownId)) untrack(loadDataSources);
  });

  /** Row pages wrap long values instead of cutting them at one line like a table cell. */
  function pageColumn(column: NotesDatabaseTableColumn): NotesDatabaseTableColumn {
    return { ...column, displayFormat: { ...column.displayFormat, wrap: true } };
  }

  function messageOf(caught: unknown): string {
    return caught instanceof Error ? caught.message : String(caught);
  }

  function busy(): boolean {
    return editingIds.size > 0 || savingIds.size > 0 || pendingSaves > 0 || relationSaves > 0;
  }

  /** Read the row again, or once open editors and saves finish, so a reload never replaces text being typed. */
  function refresh(): Promise<void> {
    if (busy()) {
      stale = true;
      return Promise.resolve();
    }
    stale = false;
    const request = ++loadRequest;
    const epoch = writeEpoch;
    return getNotesDataSourceRowPageContext(dataSourceId, pageId).then((loaded) => {
      if (request !== loadRequest) return;
      if (epoch !== writeEpoch) return refresh();
      context = loaded;
      loadError = null;
    }).catch((caught: unknown) => {
      if (request !== loadRequest) return;
      if (context) console.warn("Reload database row properties failed", caught);
      else loadError = messageOf(caught);
    });
  }

  /** Reads the row again after its first load failed, rejecting while it is still unavailable. */
  async function retryLoad(): Promise<void> {
    await refresh();
    if (loadError) throw new Error(loadError);
  }

  /** Run a reload that waited for editors and saves, once none remain. */
  function settle(): void {
    if (stale && !busy()) void refresh();
  }

  function trackEditing(propertyId: string, editing: boolean): void {
    if (editing) {
      editingIds.add(propertyId);
      return;
    }
    editingIds.delete(propertyId);
    settle();
  }

  /**
   * Save one value and show the saved row right away; the reload that follows refreshes computed values.
   *
   * @throws An error whose message names the property, ready to show.
   */
  async function writeValue(column: NotesDatabaseTableColumn, value: NotesDatabaseTableEditValue): Promise<void> {
    const row = context?.row;
    if (!row) return;
    if (notesDatabaseTableEditValuesEqual(notesDatabaseTableCellEditValue(row, column), value)) return;
    keptIds.add(column.id);
    savingIds.add(column.id);
    writeEpoch += 1;
    try {
      const updated = await updateNotesDataSourceRowProperty(dataSourceId, row.id, { property_id: column.id, value });
      if (context?.row.id === updated.id) context = { ...context, row: updated };
    } catch (caught) {
      throw new Error(t("notes.databaseRowPageValueSaveFailed", column.name, messageOf(caught)), { cause: caught });
    } finally {
      writeEpoch += 1;
      savingIds.delete(column.id);
      settle();
    }
  }

  /** Save one value from an inline editor, showing a failure below the properties. */
  async function saveValue(column: NotesDatabaseTableColumn, value: NotesDatabaseTableEditValue): Promise<boolean> {
    error = null;
    try {
      await writeValue(column, value);
      return true;
    } catch (caught) {
      error = messageOf(caught);
      return false;
    }
  }

  /** Create a page in a relation's target source and return its id, so the relation can link it. */
  async function createRelationTarget(column: NotesDatabaseTableColumn, title: string): Promise<string> {
    const targetId = column.relationDataSourceId;
    if (!targetId) throw new Error(t("notes.databaseRelationConfigureTarget"));
    try {
      const created = await createNotesDataSourceRowPage(targetId, { id: crypto.randomUUID(), title, first_block_id: crypto.randomUUID() });
      return created.page.id;
    } catch (caught) {
      throw new Error(t("notes.databaseRowPageRelationFailed", messageOf(caught)), { cause: caught });
    }
  }

  function sourceTitle(source: Pick<NotesDataSource, "title"> | undefined): string {
    return source?.title.trim() || t("notes.untitled");
  }

  /**
   * Save a parent or sub-item change, then apply it on screen until the reload that follows brings the canonical
   * hierarchy.
   *
   * @throws An error with a message ready to show.
   */
  async function writeRelations<T>(task: () => Promise<T>, apply: (current: NotesDataSourceRowPageContext, result: T) => NotesDataSourceRowPageContext): Promise<void> {
    relationSaves += 1;
    writeEpoch += 1;
    try {
      const result = await task();
      if (context) context = apply(context, result);
    } catch (caught) {
      throw new Error(t("notes.databaseRowPageRelationFailed", messageOf(caught)), { cause: caught });
    } finally {
      relationSaves -= 1;
      writeEpoch += 1;
      stale = true;
      settle();
    }
  }

  function withChildren(current: NotesDataSourceRowPageContext, children: NotesPage[]): NotesDataSourceRowPageContext {
    return { ...current, children, hierarchy: { ...current.hierarchy, child_count: children.length } };
  }

  /** Show a new parent item; its ancestors and depth arrive with the reload that follows. */
  function withParent(current: NotesDataSourceRowPageContext, parent: NotesPage | null): NotesDataSourceRowPageContext {
    return { ...current, parent, hierarchy: { ...current.hierarchy, parent_row_page_id: parent?.id ?? null } };
  }

  function setParent(parent: NotesPage | null): Promise<void> {
    return writeRelations(() => updateNotesDataSourceRowParent(dataSourceId, pageId, parent?.id ?? null),
      (current) => withParent(current, parent));
  }

  function createParent(title: string): Promise<void> {
    return writeRelations(async () => {
      const created = await createNotesDataSourceRowPage(dataSourceId, { id: crypto.randomUUID(), title, first_block_id: crypto.randomUUID() });
      await updateNotesDataSourceRowParent(dataSourceId, pageId, created.page.id);
      return created.page;
    }, (current, parent) => withParent(current, parent));
  }

  function linkSubitem(subitem: NotesPage): Promise<void> {
    return writeRelations(() => updateNotesDataSourceRowParent(dataSourceId, subitem.id, pageId),
      (current) => withChildren(current, [...current.children.filter((child) => child.id !== subitem.id), subitem]));
  }

  function unlinkSubitem(subitem: NotesPage): Promise<void> {
    return writeRelations(() => updateNotesDataSourceRowParent(dataSourceId, subitem.id, null),
      (current) => withChildren(current, current.children.filter((child) => child.id !== subitem.id)));
  }

  function createSubitem(title: string): Promise<void> {
    return writeRelations(
      () => createNotesDataSourceSubitem(dataSourceId, pageId, { id: crypto.randomUUID(), title, first_block_id: crypto.randomUUID() }),
      (current, created) => withChildren(current, [...current.children, created.page]),
    );
  }

  async function runButton(column: NotesDatabaseTableColumn): Promise<void> {
    const row = context?.row;
    if (!row || savingIds.has(column.id)) return;
    const confirmed = column.buttonRequiresConfirmation && await confirm({
      title: t("notes.databaseRowPageButtonTitle"),
      message: t("notes.databaseButtonConfirm", column.name, notesDatabaseRowTitle(row, t("notes.untitled"))),
      confirmLabel: t("notes.databaseRowPageButtonRun"),
      danger: false,
    });
    if (column.buttonRequiresConfirmation && !confirmed) return;
    savingIds.add(column.id);
    writeEpoch += 1;
    error = null;
    try {
      await clickNotesDataSourceButton(dataSourceId, row.id, { property_id: column.id, confirmed });
    } catch (caught) {
      error = t("notes.databaseRowPageValueSaveFailed", column.name, messageOf(caught));
    } finally {
      writeEpoch += 1;
      savingIds.delete(column.id);
      stale = true;
      settle();
    }
  }

  /** Ask a question in a dialog, answering any earlier unanswered one with no. */
  function confirm(request: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
    pendingConfirm?.resolve(false);
    return new Promise((resolve) => {
      pendingConfirm = { ...request, resolve: (confirmed) => { pendingConfirm = null; resolve(confirmed); } };
    });
  }

  /** Move focus between values with the arrow keys and Enter, leaving keys the focused field still uses to it. */
  function navigate(event: KeyboardEvent, rowIndex: number): void {
    event.stopPropagation();
    if (notesDatabaseCellKeepsKey(event.key, event.shiftKey, notesDatabaseCellCaret(event.currentTarget))) return;
    if (event.defaultPrevented || (event.currentTarget instanceof HTMLElement && event.currentTarget.hasAttribute("aria-haspopup"))) return;
    const step = event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" || event.key === "Enter" ? 1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = root?.querySelector<HTMLElement>(`[data-table-cell="true"][data-row-index="${rowIndex + step}"]`);
    if (next) next.focus();
    else if (event.key === "Enter" && event.currentTarget instanceof HTMLElement) event.currentTarget.blur();
  }

  /** The source, database, and default view a structure change saves in. */
  function editingScope(): NotesDatabaseSourceEditingScope {
    const schema = context?.schema;
    if (!schema) throw new Error(t("notes.databaseSchemaLoadFailed", ""));
    return { dataSourceId: schema.data_source.id, databaseId: schema.data_source.parent.database_id, viewId: schema.view.id };
  }

  /** Action inputs, read when a queued action runs so it sees the newest loaded sources. */
  function actionContext(): NotesDataSourceSchemaActionContext {
    return { scope: editingScope(), dataSources: availableDataSources, t };
  }

  /** Run structure changes one at a time, each starting from the newest saved schema. */
  function enqueue<T>(task: () => Promise<T>): Promise<T> {
    if (locked) return Promise.reject(new Error(t("notes.databaseEditingLockDescription")));
    pendingSaves += 1;
    writeEpoch += 1;
    const result = saveQueue.then(task);
    saveQueue = result.then(() => undefined, () => undefined);
    return result.finally(() => {
      pendingSaves -= 1;
      writeEpoch += 1;
      settle();
    });
  }

  function adoptResult(result: NotesDataSourceSchemaActionResult): void {
    if (result.dataSources) availableDataSources = result.dataSources;
    if (context && result.schema.data_source.id === context.schema.data_source.id) context = { ...context, schema: result.schema };
  }

  /** Run a menu action, showing a failure below the properties and in the open menu. */
  function runAction(task: () => Promise<void>): void {
    error = null;
    void task().catch((caught: unknown) => { error = t("collections.property.saveFailed", messageOf(caught)); });
  }

  /** Load the sources a relation or rollup can point to, once, when a property menu first opens. */
  function loadDataSources(): void {
    if (dataSourcesRequested) return;
    dataSourcesRequested = true;
    listNotesDataSources().then((sources) => { availableDataSources = sources; }).catch((caught: unknown) => {
      dataSourcesRequested = false;
      console.warn("Load Notes data sources failed", caught);
    });
  }

  function renameProperty(propertyId: string, name: string): Promise<void> {
    return enqueue(async () => adoptResult(await renameNotesDataSourceSchemaProperty(actionContext(), propertyId, name)));
  }

  function changePropertyIcon(propertyId: string, icon: string | null): Promise<void> {
    return enqueue(async () => adoptResult(await changeNotesDataSourceSchemaPropertyIcon(actionContext(), propertyId, icon)));
  }

  function changePropertyType(propertyId: string, propertyType: NotesDataSourcePropertyType): void {
    runAction(() => enqueue(async () => adoptResult(await changeNotesDataSourceSchemaPropertyType(actionContext(), propertyId, propertyType))));
  }

  function updateProperty(propertyId: string, update: NotesDataSourcePropertyUpdate): Promise<void> {
    return enqueue(async () => adoptResult(await updateNotesDataSourceSchemaProperty(actionContext(), propertyId, update)));
  }

  function editProperty(propertyId: string, update: NotesDataSourcePropertyUpdate): void {
    runAction(() => updateProperty(propertyId, update));
  }

  /** Save an option edit and report its outcome, so the options editor can explain a rejected name. */
  async function editOptions(propertyId: string, edit: NotesDataSourceOptionEditor): Promise<NotesDataSourceOptionEdit["status"]> {
    let status: NotesDataSourceOptionEdit["status"] = "unchanged";
    error = null;
    try {
      await updateProperty(propertyId, (target) => {
        const result = edit(target);
        status = result.status;
        return result.status === "changed" ? result.patch : null;
      });
      return status;
    } catch (caught: unknown) {
      error = t("collections.property.saveFailed", messageOf(caught));
      return "unchanged";
    }
  }

  function setVisibility(propertyId: string, visibility: NotesDataSourcePageVisibility): void {
    if (visibility !== "hide_when_empty") keptIds.delete(propertyId);
    runAction(() => updateProperty(propertyId, (target) => target.pageVisibility === visibility ? null : { pageVisibility: visibility }));
  }

  function visibilityLabel(visibility: NotesDataSourcePageVisibility): string {
    switch (visibility) {
      case "always_show": return t("notes.databaseRowPageAlwaysShow");
      case "hide_when_empty": return t("notes.databaseRowPageHideWhenEmpty");
      case "always_hide": return t("notes.databaseRowPageAlwaysHide");
    }
  }

  function duplicateProperty(propertyId: string): void {
    runAction(() => enqueue(async () => adoptResult(await duplicateNotesDataSourceSchemaProperty(actionContext(), propertyId))));
  }

  async function deleteProperty(property: NotesDataSourceSchemaPropertyDraft): Promise<void> {
    const confirmed = await confirm({
      title: t("collections.property.deleteTitle"),
      message: t("collections.property.deleteMessage", property.name),
      confirmLabel: t("collections.property.delete"),
      danger: true,
    });
    if (confirmed) await enqueue(async () => adoptResult(await deleteNotesDataSourceSchemaProperty(actionContext(), property.id)));
  }

  /**
   * Save a new row page order. The order shows at once, and the saved one is rebuilt from the newest schema so a
   * property added meanwhile keeps its place.
   *
   * @param shown The order to show while saving.
   * @param arrange Builds the order to save from the newest saved order, or returns null to save nothing.
   */
  function reorder(shown: string[], arrange: (savedIds: string[]) => string[] | null): Promise<void> {
    optimisticOrder = shown;
    return enqueue(async () => adoptResult(await updateNotesDataSourceSchemaDrafts(actionContext(), (current) => {
      const next = arrange(orderNotesDatabaseRowPageProperties(current).map((property) => property.id));
      return next ? withNotesDatabaseRowPagePositions(current, next) : null;
    }))).finally(() => { optimisticOrder = null; });
  }

  /** Move a property one step among the properties on screen. */
  function stepProperty(propertyId: string, direction: -1 | 1): Promise<void> {
    const visible = [...shownIds];
    const shown = stepNotesDatabaseRowPageProperty(orderedIds, visible, propertyId, direction);
    if (!shown) return Promise.resolve();
    return reorder(shown, (savedIds) => stepNotesDatabaseRowPageProperty(savedIds, visible.filter((id) => savedIds.includes(id)), propertyId, direction));
  }

  async function stepWithKeyboard(event: KeyboardEvent, propertyId: string): Promise<void> {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    event.stopPropagation();
    error = null;
    try {
      await stepProperty(propertyId, event.key === "ArrowUp" ? -1 : 1);
    } catch (caught) {
      error = t("collections.property.saveFailed", messageOf(caught));
    }
    await tick();
    root?.querySelector<HTMLElement>(`[data-row-page-grip="${CSS.escape(propertyId)}"]`)?.focus({ preventScroll: true });
  }

  function startDrag(event: DragEvent & { currentTarget: HTMLElement }, propertyId: string): void {
    if (locked) {
      event.preventDefault();
      return;
    }
    dragged = propertyId;
    error = null;
    if (!event.dataTransfer) return;
    event.dataTransfer.setData("text/plain", propertyId);
    event.dataTransfer.effectAllowed = "move";
    const row = event.currentTarget.closest<HTMLElement>("[data-row-page-property]");
    if (row) event.dataTransfer.setDragImage(row, 0, row.offsetHeight / 2);
  }

  function dragOver(event: DragEvent & { currentTarget: HTMLElement }, propertyId: string): void {
    if (!dragged) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dropTarget = dragged === propertyId ? null : {
      propertyId,
      placement: notesDatabaseViewDropPlacement(event.currentTarget.getBoundingClientRect(), { x: event.clientX, y: event.clientY }, "y"),
    };
  }

  function drop(event: DragEvent): void {
    if (!dragged) return;
    event.preventDefault();
    event.stopPropagation();
    const moved = dragged;
    const target = dropTarget;
    clearDrag();
    if (!target) return;
    const shown = moveNotesDatabaseRowPageProperty(orderedIds, moved, target.propertyId, target.placement);
    if (!shown) return;
    runAction(() => reorder(shown, (savedIds) => moveNotesDatabaseRowPageProperty(savedIds, moved, target.propertyId, target.placement)));
  }

  function clearDrag(): void {
    dragged = null;
    dropTarget = null;
  }

  /** Add a property at the end of the row page order, then focus its value. */
  async function createProperty(type: NotesDataSourcePropertyType, name: string, icon: string | null): Promise<boolean> {
    const result = await enqueue(() => appendNotesDataSourceSchemaProperty(actionContext(), type, name, icon, (current, property) =>
      withNotesDatabaseRowPagePositions([...current, property],
        [...orderNotesDatabaseRowPageProperties(current).map((entry) => entry.id), property.id])));
    adoptResult(result);
    focusPropertyId = result.propertyId;
    return true;
  }
</script>

{#snippet messages()}
  {#if locked}<p class="px-2 py-1 text-panel-detail text-muted-foreground">{t("notes.databaseEditingLockDescription")}</p>{/if}
  {#if error}<Notice class="px-2 py-1 text-panel-detail" message={error} />{/if}
{/snippet}

{#snippet propertyMenu(property: NotesDataSourceSchemaPropertyDraft, index: number)}
  {@const kind = notesPropertyKind(property.type)}
  <CollectionMenu label={property.name} kind="property" fullWidth showHeader={false} dismissOnAction contextMenu
    rootClass="min-w-0" triggerClass="cursor-pointer justify-start px-1.5" onOpenChange={(open) => { if (open) loadDataSources(); }}>
    {#snippet leading()}{#if preferences.notesShowPropertyIcons}<CollectionPropertyIcon icon={property.icon} {kind} class="size-3.5 shrink-0" />{/if}{/snippet}
    <div class="grid min-w-0 gap-0 font-normal">
      <CollectionPropertyNameField propertyId={property.id} name={property.name} {kind} icon={property.icon} editable={!locked}
        maxLength={NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS}
        onRename={(name) => renameProperty(property.id, name)} onIconChange={(icon) => changePropertyIcon(property.id, icon)} />
      {#if notesPropertyHasSchemaOptions(property.type)}
        <CollectionMenu label={t("collections.property.editProperty")} kind="properties" icon={Settings2} fullWidth disabled={locked}>
          <NotesDatabasePropertySchemaEditor {property} properties={drafts} dataSources={relationDataSources} {dataSourceId} disabled={locked}
            onUpdate={(update) => editProperty(property.id, update)} onOptionEdit={(edit) => editOptions(property.id, edit)} />
          {@render messages()}
        </CollectionMenu>
      {/if}
      <CollectionPropertyTypeMenu types={propertyTypeOptions} current={property.type} disabled={locked}
        onChange={(propertyType) => changePropertyType(property.id, propertyType)} />
      <CollectionMenu label={t("notes.databaseRowPageVisibility")} kind="properties" icon={Eye} fullWidth dismissOnAction disabled={locked}
        summary={visibilityLabel(property.pageVisibility)}>
        {#each PAGE_VISIBILITIES as visibility (visibility)}
          <CollectionMenuItem label={visibilityLabel(visibility)} checked={property.pageVisibility === visibility} disabled={locked}
            onclick={() => setVisibility(property.id, visibility)} />
        {/each}
      </CollectionMenu>
      <CollectionMenuSeparator />
      <CollectionMenuItem icon={ArrowUp} label={t("notes.databaseRowPageMoveUp")} disabled={locked || index === 0}
        onclick={() => runAction(() => stepProperty(property.id, -1))} />
      <CollectionMenuItem icon={ArrowDown} label={t("notes.databaseRowPageMoveDown")} disabled={locked || index === shownEntries.length - 1}
        onclick={() => runAction(() => stepProperty(property.id, 1))} />
      <CollectionMenuItem icon={Copy} label={t("collections.property.duplicate")} disabled={locked} onclick={() => duplicateProperty(property.id)} />
      <CollectionMenuItem icon={Trash2} label={t("collections.property.delete")} destructive disabled={locked}
        onclick={() => runAction(() => deleteProperty(property))} />
      {@render messages()}
    </div>
  </CollectionMenu>
{/snippet}

{#if context}
  {@const row = context.row}
  <!-- Names, values, and buttons keep 0.375rem of inner padding, which the negative margin moves out so their text starts at the page text. -->
  <section bind:this={root} class="@container -ml-1.5 min-w-0 text-collection" aria-label={t("notes.databaseRowPageProperties")}>
    {#each shownEntries as entry, index (entry.property.id)}
      {@const property = entry.property}
      {@const column = columns.get(property.id)}
      {#if column}
        {@const marker = dropTarget?.propertyId === property.id ? dropTarget.placement : null}
        <div class={cn("group/property relative grid min-w-0 grid-cols-1 items-start @sm:grid-cols-[minmax(6rem,10rem)_minmax(0,1fr)] @sm:gap-2", dragged === property.id && "opacity-50")}
          role="group" aria-label={property.name} data-row-page-property={property.id}
          ondragover={(event) => dragOver(event, property.id)} ondrop={drop}>
          {#if marker}
            <span class={cn("pointer-events-none absolute inset-x-0 h-0.5 rounded-full bg-primary", marker === "before" ? "-top-px" : "-bottom-px")} aria-hidden="true"></span>
          {/if}
          {#if !locked}
            <button type="button" draggable="true"
              class="absolute top-1 -left-6 flex h-6 w-5 cursor-grab items-center justify-center rounded text-muted-foreground opacity-100 hover:bg-accent hover:text-foreground focus-visible:opacity-100 active:cursor-grabbing pointer-coarse:hidden [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/property:opacity-100"
              data-row-page-grip={property.id} aria-label={t("notes.databaseRowPageReorder", property.name)} data-app-tooltip={t("notes.databaseRowPageReorderHint")}
              ondragstart={(event) => startDrag(event, property.id)} ondragend={clearDrag}
              onkeydown={(event) => { void stepWithKeyboard(event, property.id); }}>
              <GripVertical class="size-3.5" strokeWidth={1.75} aria-hidden="true" />
            </button>
          {/if}
          {@render propertyMenu(property, index)}
          <div class="min-w-0">
            {#if column.type === "relation"}
              <NotesDatabaseRowPageRelationValue {row} {column} rowIndex={index} columnIndex={0} disabled={savingIds.has(column.id)}
                sourceName={sourceTitle(relationDataSources.find((source) => source.id === column.relationDataSourceId))}
                onSave={(relationIds) => { error = null; return writeValue(column, relationIds); }}
                onCreate={(title) => createRelationTarget(column, title)}
                onNavigate={(event, rowIndex) => navigate(event, rowIndex)} {onOpenPage} />
            {:else if column.type === "select" || column.type === "status" || column.type === "multi_select"}
              <NotesDatabaseRowPageOptionValue {row} {column} rowIndex={index} columnIndex={0} options={notesDataSourceVisibleOptions(property)}
                onSave={(value) => saveValue(column, value)}
                onCreate={locked || column.type === "status" ? undefined : (name) => editOptions(property.id, (target) => notesDataSourceAddOption(target, name))}
                onNavigate={(event, rowIndex) => navigate(event, rowIndex)} onEditingChange={(editing) => trackEditing(column.id, editing)} />
            {:else}
              <NotesDatabasePropertyEditor {row} {column} rowIndex={index} columnIndex={0} variant="page"
                value={notesDatabaseTableCellEditValue(row, column)} disabled={savingIds.has(column.id)}
                onSave={(value) => saveValue(column, value)} onNavigate={(event, rowIndex) => navigate(event, rowIndex)}
                onEditingChange={(editing) => trackEditing(column.id, editing)} onRunButton={() => { void runButton(column); }} />
            {/if}
          </div>
        </div>
      {/if}
    {/each}
    <div class="flex min-w-0 flex-wrap items-center gap-x-1">
      {#if hiddenCount > 0 && !expanded}
        <button type="button" class="row-page-footer-button" aria-expanded="false" onclick={() => { expanded = true; }}>
          <ChevronDown class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
          <span class="truncate">{t("notes.databaseRowPageMoreProperties", hiddenCount, formatNumber(localization.locale, hiddenCount))}</span>
        </button>
      {:else}
        {#if !locked}
          <button bind:this={addButton} type="button" class="row-page-footer-button" aria-expanded={adding} onclick={() => { adding = !adding; }}>
            <Plus class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
            <span class="truncate">{t("notes.databaseRowPageAddProperty")}</span>
          </button>
        {/if}
        {#if hiddenCount > 0}
          <button type="button" class="row-page-footer-button" aria-expanded="true" onclick={() => { expanded = false; }}>
            <ChevronUp class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
            <span class="truncate">{t("notes.databaseRowPageHideProperties", hiddenCount, formatNumber(localization.locale, hiddenCount))}</span>
          </button>
        {/if}
      {/if}
    </div>
    {#if error}<Notice class="px-1.5 py-1 text-panel-detail" message={error} />{/if}
    <NotesDatabaseRowPageRelations {dataSourceId} sourceName={sourceTitle(context.schema.data_source)} {row}
      hierarchy={context.hierarchy} parent={context.parent} subitems={context.children} {onOpenPage}
      onSetParent={setParent} onCreateParent={createParent} onLinkSubitem={linkSubitem} onUnlinkSubitem={unlinkSubitem}
      onCreateSubitem={createSubitem} />
  </section>
  {#if adding && addButton && !locked}
    <CollectionPropertyDraft anchor={addButton} placement="start" trigger={addButton} types={propertyTypeOptions} pending={pendingSaves > 0}
      maxLength={NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS} onCreate={createProperty} onClose={() => { adding = false; }} />
  {/if}
{:else if loadError}
  <Notice class="text-panel-detail" message={t("notes.databaseRowPageLoadFailed", loadError)} copy={loadError} onRetry={retryLoad} />
{/if}

{#if pendingConfirm}
  {@const request = pendingConfirm}
  <ConfirmDialog title={request.title} message={request.message} confirmLabel={request.confirmLabel} cancelLabel={t("common.cancel")}
    danger={request.danger} onConfirm={() => request.resolve(true)} onCancel={() => request.resolve(false)} />
{/if}

<style>
  .row-page-footer-button {
    display: inline-flex;
    min-width: 0;
    height: 2rem;
    cursor: pointer;
    align-items: center;
    gap: 0.375rem;
    border-radius: 0.25rem;
    padding-inline: 0.375rem;
    color: var(--muted-foreground);
  }

  .row-page-footer-button:hover,
  .row-page-footer-button:focus-visible {
    background-color: color-mix(in oklab, var(--accent) 60%, transparent);
    color: var(--foreground);
    outline: none;
  }

  @media (any-pointer: coarse) and (any-hover: none) {
    .row-page-footer-button {
      min-height: var(--panel-row-height);
    }
  }
</style>
