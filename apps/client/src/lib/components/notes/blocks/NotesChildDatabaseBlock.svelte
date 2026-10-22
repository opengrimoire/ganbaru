<script lang="ts">
  import CollectionSaveIndicator from "$lib/components/collections/CollectionSaveIndicator.svelte";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionPropertyCreator from "$lib/components/collections/CollectionPropertyCreator.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import CollectionPropertyNameField from "$lib/components/collections/CollectionPropertyNameField.svelte";
  import CollectionPropertyTypeMenu from "$lib/components/collections/CollectionPropertyTypeMenu.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { tick, untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    getNotesDataSourceSchema,
    getNotesDatabaseReference,
    listNotesDatabaseViews,
    listNotesDataSources,
    renameNotesDatabase,
  } from "$lib/api/notes";
  import NotesDatabasePropertySchemaEditor from "$lib/components/notes/database/NotesDatabasePropertySchemaEditor.svelte";
  import NotesDatabaseViewSurface from "$lib/components/notes/database/NotesDatabaseViewSurface.svelte";
  import {
    notesPropertyHasSchemaOptions,
    notesPropertyKind,
    notesPropertyTypeLabel,
    notesPropertyTypeOptions,
  } from "$lib/components/notes/database/property-kinds";
  import {
    NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS,
    notesDataSourceSchemaDraftFromDto,
    type NotesDataSourcePropertyUpdate,
    type NotesDataSourceSchemaPropertyDraft,
    type NotesDatabasePropertyActionRequest,
    type NotesDatabaseSourceEditingScope,
  } from "$lib/notes/database/data-source-schema";
  import {
    appendNotesDataSourceSchemaProperty,
    changeNotesDataSourceSchemaPropertyIcon,
    changeNotesDataSourceSchemaPropertyType,
    deleteNotesDataSourceSchemaProperty,
    duplicateNotesDataSourceSchemaProperty,
    insertNotesDataSourceSchemaProperty,
    loadNotesDataSourceSchemaDrafts,
    renameNotesDataSourceSchemaProperty,
    updateNotesDataSourceSchemaProperty,
    type NotesDataSourceSchemaActionContext,
    type NotesDataSourceSchemaActionResult,
  } from "$lib/notes/database/property-schema-actions";
  import type { NotesDataSourceOptionEdit, NotesDataSourceOptionEditor } from "$lib/notes/database/property-options";
  import type {
    NotesChildDatabaseBlock,
    NotesDataSource,
    NotesDataSourcePropertyType,
    NotesDataSourceSchema,
    NotesDatabaseReference,
  } from "$lib/notes/types";
  import Database from "@lucide/svelte/icons/database";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Trash2 from "@lucide/svelte/icons/trash";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import Copy from "@lucide/svelte/icons/copy";
  import { buildNotesBlockLink, buildNotesDatabaseViewLink } from "$lib/notes/links/block-link";
  import { notesDatabaseSession } from "$lib/notes/database/session.svelte";

  let {
    block,
    focusBlockId,
    focusRequestId,
    onFocusBlock,
    onKeydown,
    onSelectPage,
    onCreateLinkedDatabaseView,
    onReady = () => {},
    onTitleSaved = () => {},
    onOpenDatabase,
    onDeleteDatabase,
  }: {
    block: NotesChildDatabaseBlock;
    focusBlockId: string | null;
    focusRequestId: number;
    onFocusBlock: (blockId: string) => void;
    onKeydown: (event: KeyboardEvent) => void;
    onSelectPage: (pageId: string) => void;
    onCreateLinkedDatabaseView: (blockId: string) => Promise<void> | void;
    onReady?: () => void;
    onTitleSaved?: (databaseId: string, title: string) => void;
    onOpenDatabase?: (blockId: string) => Promise<void | boolean> | void;
    onDeleteDatabase?: (blockId: string) => Promise<void | boolean> | void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;

  let titleInput: HTMLInputElement | null = $state(null);
  let expanded = $state(false);
  let schemaAnchor: HTMLElement | null = $state(null);
  let titleDraft = $state(untrack(() => block.child_database.title));
  let savedTitle = $state(untrack(() => block.child_database.title));
  let titleSaving = $state(false);
  let viewSaving = $state(false);
  let contentReady = $state(false);
  let titleError = $state<string | null>(null);
  let reference = $state<NotesDatabaseReference | null>(null);
  let loading = $state(false);
  let linking = $state(false);
  let error = $state<string | null>(null);
  let linkedViewError = $state<string | null>(null);
  let schema = $state<NotesDataSourceSchema | null>(null);
  let schemaViewId = $state<string | null>(null);
  let editorSourceId = $state<string | null>(null);
  /** The data source whose property a column's Edit property submenu shows. */
  let propertyEditorSourceId = $state<string | null>(null);
  let editingLocked = $state(untrack(() => block.child_database.editing_locked ?? false));
  /** A property deletion waiting for confirmation, with the callback that answers it. */
  let pendingDelete = $state<{ name: string; resolve: (confirmed: boolean) => void } | null>(null);
  let pendingSaves = $state(0);
  let saveQueue: Promise<void> = Promise.resolve();
  let schemaLoadPromise: Promise<void> | null = null;
  let schemaRequestId = 0;
  /** Counts adopted saves, so a schema load that started before a save cannot replace what the save returned. */
  let adoptRevision = 0;
  let lockRevision = 0;
  let incomingLock = untrack(() => block.child_database.editing_locked ?? false);
  let availableDataSources = $state<NotesDataSource[]>([]);
  let properties = $state<NotesDataSourceSchemaPropertyDraft[]>([]);
  let tableReloadKey = $state(0);
  let boardReloadKey = $state(0);
  let galleryReloadKey = $state(0);
  let listReloadKey = $state(0);
  let calendarReloadKey = $state(0);
  let timelineReloadKey = $state(0);

  const title = $derived(titleDraft.trim());
  const dataSourceId = $derived(block.child_database.data_source_id ?? null);
  const databaseId = $derived(block.child_database.database_id ?? null);
  const viewId = $derived(block.child_database.view_id ?? null);
  const isLocalDatabase = $derived(
    block.child_database.database_id !== undefined
      && block.child_database.data_source_id !== undefined
      && block.child_database.view_id !== undefined,
  );
  const schemaDataSourceId = $derived(editorSourceId ?? dataSourceId);
  const saving = $derived(pendingSaves > 0);
  const propertyTypeOptions = $derived(notesPropertyTypeOptions(t));

  /** Reveal this database and defer requested title focus until its surface can receive input. */
  function reportReady(): void {
    contentReady = true;
    onReady();
  }

  $effect(() => { if (!isLocalDatabase || !dataSourceId) reportReady(); });

  $effect(() => {
    const incoming = block.child_database.title;
    if (titleSaving || incoming === savedTitle
      || (titleInput === document.activeElement && titleDraft.trim() !== savedTitle)) return;
    savedTitle = incoming;
    titleDraft = incoming;
  });

  $effect(() => {
    const id = block.id;
    notesDatabaseSession.revision;
    if (!isLocalDatabase) return;
    const revision = lockRevision;
    let cancelled = false;
    void getNotesDatabaseReference(id).then((value) => { if (!cancelled) {
      reference = revision === lockRevision ? value : { ...value, editing_locked: editingLocked };
      if (revision === lockRevision) editingLocked = value.editing_locked;
    } })
      .catch((caught: unknown) => { if (!cancelled) console.warn("Load database source reference failed", caught); });
    return () => { cancelled = true; };
  });

  $effect(() => {
    const locked = block.child_database.editing_locked ?? false;
    if (locked === incomingLock) return;
    incomingLock = locked;
    lockRevision += 1;
    editingLocked = locked;
  });

  function messageOf(caught: unknown): string {
    return caught instanceof Error ? caught.message : String(caught);
  }

  async function openDatabase(id: string): Promise<void> {
    try { await onOpenDatabase?.(id); }
    catch (caught: unknown) { titleError = messageOf(caught); }
  }

  async function deleteDatabase(): Promise<void> {
    try { await onDeleteDatabase?.(block.id); }
    catch (caught: unknown) { titleError = messageOf(caught); }
  }

  /**
   * Copy a link to this database, or to one of its saved views when a view is given.
   *
   * @param viewId Saved view the link selects when opened.
   */
  async function copyDatabaseLink(viewId?: string): Promise<void> {
    try {
      const destination = reference ?? await getNotesDatabaseReference(block.id);
      const target = { pageId: destination.page_id, blockId: destination.block_id };
      await navigator.clipboard.writeText(viewId
        ? buildNotesDatabaseViewLink(window.location.href, { ...target, viewId })
        : buildNotesBlockLink(window.location.href, target));
    } catch (caught: unknown) { titleError = messageOf(caught); }
  }

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (!contentReady || focusBlockId !== block.id) return;
    void tick().then(() => {
      if (focusBlockId === block.id && _focusRequestId === focusRequestId
        && !titleInput?.closest("[inert], [hidden]")) titleInput?.focus({ preventScroll: true });
    });
  });

  /**
   * Run schema saves one at a time in request order, so each one reads the schema the previous one saved.
   * A locked layout rejects the save without queueing it.
   */
  function enqueue<T>(task: () => Promise<T>): Promise<T> {
    if (editingLocked) return Promise.reject(new Error(t("notes.databaseEditingLockDescription")));
    pendingSaves += 1;
    const result = saveQueue.then(task);
    saveQueue = result.then(() => undefined, () => undefined);
    return result.finally(() => { pendingSaves -= 1; });
  }

  function loadSchema(scope?: NotesDatabaseSourceEditingScope, request = ++schemaRequestId): Promise<void> {
    if (request !== schemaRequestId) return Promise.resolve();
    if (schemaLoadPromise) return schemaLoadPromise.then(() => request === schemaRequestId ? loadSchema(scope, request) : undefined);
    const sourceId = scope?.dataSourceId ?? schemaDataSourceId;
    if (!sourceId) return Promise.resolve();
    schemaLoadPromise = performLoadSchema(sourceId, scope?.viewId ?? null, request).finally(() => { schemaLoadPromise = null; });
    return schemaLoadPromise;
  }

  /** Open the property list from view settings, reading the newest saved schema. */
  function openProperties(anchor?: HTMLElement | null, scope?: NotesDatabaseSourceEditingScope): void {
    if (editingLocked) return;
    schemaAnchor = anchor ?? titleInput;
    expanded = true;
    void loadSchema(scope);
  }

  /** Load the schema a column's Edit property submenu edits. */
  function preparePropertyEditor(_propertyId: string, scope?: NotesDatabaseSourceEditingScope): void {
    propertyEditorSourceId = scope?.dataSourceId ?? schemaDataSourceId;
    if (editingLocked) return;
    const request = ++schemaRequestId;
    if (schema && (!scope || scope.dataSourceId === editorSourceId)) {
      loading = false;
      if (availableDataSources.length === 0) void loadSourceMetadata(request);
      return;
    }
    void loadSchema(scope, request);
  }

  /** Load relation target sources when the schema was cached by a header change before an editor first opened. */
  async function loadSourceMetadata(request: number): Promise<void> {
    loading = true;
    try {
      const sources = await listNotesDataSources();
      if (request === schemaRequestId) availableDataSources = sources;
    } catch (caught: unknown) {
      if (request === schemaRequestId) error = t("notes.databaseSchemaLoadFailed", messageOf(caught));
    } finally {
      if (request === schemaRequestId) loading = false;
    }
  }

  async function performLoadSchema(sourceId: string, requestedViewId: string | null, request: number): Promise<void> {
    const revision = adoptRevision;
    loading = true;
    error = null;
    try {
      const [views, dataSources] = await Promise.all([
        databaseId ? listNotesDatabaseViews(databaseId) : Promise.resolve([]),
        listNotesDataSources(),
      ]);
      const tableViewId = views.find((view) => view.id === requestedViewId && view.type === "table" && view.data_source_id === sourceId)?.id
        ?? views.find((view) => view.type === "table" && view.data_source_id === sourceId)?.id ?? null;
      const loaded = await getNotesDataSourceSchema(sourceId, { databaseId, viewId: tableViewId });
      if (request !== schemaRequestId) return;
      availableDataSources = dataSources;
      if (revision !== adoptRevision && editorSourceId === sourceId) return;
      schema = loaded;
      editorSourceId = sourceId;
      schemaViewId = loaded.view.id;
      properties = notesDataSourceSchemaDraftFromDto(loaded.data_source, loaded.view);
      reloadViews();
    } catch (caught) {
      if (request === schemaRequestId) error = t("notes.databaseSchemaLoadFailed", messageOf(caught));
    } finally {
      if (request === schemaRequestId) loading = false;
    }
  }

  /** Show a saved schema when it belongs to the source being edited, or when no source is being edited yet. */
  function adopt(updated: NotesDataSourceSchema): void {
    const sourceId = updated.data_source.id;
    if (editorSourceId && editorSourceId !== sourceId) return;
    adoptRevision += 1;
    schema = updated;
    editorSourceId = sourceId;
    schemaViewId = updated.view.id;
    properties = notesDataSourceSchemaDraftFromDto(updated.data_source, updated.view);
  }

  /** The source the settings panel and Edit property submenus edit, or null before a schema has loaded. */
  function editorScope(): NotesDatabaseSourceEditingScope | null {
    return editorSourceId ? { dataSourceId: editorSourceId, databaseId, viewId: schemaViewId } : null;
  }

  function withEditorScope(task: (scope: NotesDatabaseSourceEditingScope) => Promise<void>): Promise<void> {
    const scope = editorScope();
    return scope ? task(scope) : Promise.reject(new Error(t("notes.databaseSchemaLoadFailed", "")));
  }

  /** Run a settings panel action, showing a failure in the panel. */
  function runPanelAction(task: (scope: NotesDatabaseSourceEditingScope) => Promise<void>): void {
    error = null;
    void withEditorScope(task).catch((caught: unknown) => { error = messageOf(caught); });
  }

  async function saveTitle(): Promise<void> {
    const renamedDatabaseId = databaseId;
    const acknowledgeTitle = onTitleSaved;
    if (!renamedDatabaseId || titleSaving || editingLocked) return;
    const nextTitle = titleDraft.trim();
    if (nextTitle === savedTitle) return;
    titleSaving = true;
    titleError = null;
    try {
      titleDraft = await renameNotesDatabase(renamedDatabaseId, nextTitle);
      savedTitle = titleDraft;
      acknowledgeTitle(renamedDatabaseId, savedTitle);
    } catch (caught) {
      titleDraft = savedTitle;
      titleError = messageOf(caught);
    } finally {
      titleSaving = false;
    }
  }

  async function createLinkedView(): Promise<void> {
    if (linking) return;
    linking = true;
    linkedViewError = null;
    try {
      await onCreateLinkedDatabaseView(block.id);
    } catch (caught) {
      linkedViewError = t("notes.databaseLinkedViewCreateFailed", messageOf(caught));
    } finally {
      linking = false;
    }
  }

  /** Make every rendered view read its source again after a schema change. */
  function reloadViews(): void {
    tableReloadKey += 1;
    boardReloadKey += 1;
    galleryReloadKey += 1;
    listReloadKey += 1;
    calendarReloadKey += 1;
    timelineReloadKey += 1;
  }

  /** Action inputs for one source, read when the queued action runs so it sees the newest loaded sources. */
  function actionContext(scope: NotesDatabaseSourceEditingScope): NotesDataSourceSchemaActionContext {
    return { scope, dataSources: availableDataSources, t };
  }

  /** Show a saved action result, making the views read it again when something changed. */
  function adoptResult(result: NotesDataSourceSchemaActionResult): void {
    if (result.dataSources) availableDataSources = result.dataSources;
    adopt(result.schema);
    if (result.changed) reloadViews();
  }

  /**
   * Apply one property edit to the newest saved schema and show the result. The update reads that saved property,
   * so edits made in quick succession build on each other; returning null saves nothing.
   */
  function updateSchemaProperty(propertyId: string, update: NotesDataSourcePropertyUpdate, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return enqueue(async () => adoptResult(await updateNotesDataSourceSchemaProperty(actionContext(scope), propertyId, update)));
  }

  /** Save a change from an Edit property menu, showing a failure beside it. */
  function editProperty(propertyId: string, update: NotesDataSourcePropertyUpdate): void {
    error = null;
    void withEditorScope((scope) => updateSchemaProperty(propertyId, update, scope))
      .catch((caught: unknown) => { error = t("collections.property.saveFailed", messageOf(caught)); });
  }

  /** Save an option edit and report its outcome, so the options editor can explain a rejected name. */
  async function editOptions(propertyId: string, edit: NotesDataSourceOptionEditor): Promise<NotesDataSourceOptionEdit["status"]> {
    let status: NotesDataSourceOptionEdit["status"] = "unchanged";
    error = null;
    try {
      await withEditorScope((scope) => updateSchemaProperty(propertyId, (target) => {
        const result = edit(target);
        status = result.status;
        return result.status === "changed" ? result.patch : null;
      }, scope));
      return status;
    } catch (caught: unknown) {
      error = t("collections.property.saveFailed", messageOf(caught));
      return "unchanged";
    }
  }

  /** Rename one property; duplicate names are rejected ignoring case. */
  function renameProperty(propertyId: string, rawName: string, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return enqueue(async () => adoptResult(await renameNotesDataSourceSchemaProperty(actionContext(scope), propertyId, rawName)));
  }

  /** Change one property's icon; null returns it to the type icon. */
  function changePropertyIcon(propertyId: string, icon: string | null, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return enqueue(async () => adoptResult(await changeNotesDataSourceSchemaPropertyIcon(actionContext(scope), propertyId, icon)));
  }

  /** Convert one property to another type, converting its row values in the same native transaction. */
  function changePropertyType(propertyId: string, propertyType: NotesDataSourcePropertyType, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return enqueue(async () => adoptResult(await changeNotesDataSourceSchemaPropertyType(actionContext(scope), propertyId, propertyType)));
  }

  /** Ask before deleting a property, answering any earlier unanswered request with no. */
  function confirmDelete(name: string): Promise<boolean> {
    pendingDelete?.resolve(false);
    return new Promise((resolve) => {
      pendingDelete = { name, resolve: (confirmed) => { pendingDelete = null; resolve(confirmed); } };
    });
  }

  /** Delete one property and its values from every row after confirmation. */
  async function deleteProperty(propertyId: string, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    const { drafts } = await loadNotesDataSourceSchemaDrafts(scope);
    const property = drafts.find((candidate) => candidate.id === propertyId);
    if (!property) throw new Error(t("notes.databaseSchemaSaveFailed", propertyId));
    if (!await confirmDelete(property.name)) return;
    await enqueue(async () => adoptResult(await deleteNotesDataSourceSchemaProperty(actionContext(scope), propertyId)));
  }

  /** Insert a new property beside another, or duplicate one with its configuration, in one native transaction. */
  function placeProperty(request: Extract<NotesDatabasePropertyActionRequest, { type: "insert" | "duplicate" }>, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return enqueue(async () => adoptResult(request.type === "duplicate"
      ? await duplicateNotesDataSourceSchemaProperty(actionContext(scope), request.propertyId)
      : await insertNotesDataSourceSchemaProperty(actionContext(scope), {
        besidePropertyId: request.propertyId,
        side: request.side,
        propertyType: request.propertyType,
        name: request.name,
        icon: request.icon,
      })));
  }

  /** Apply a property change requested from a view header. Failures are rethrown for the header to show. */
  function applyPropertyAction(request: NotesDatabasePropertyActionRequest, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    switch (request.type) {
      case "rename": return renameProperty(request.propertyId, request.name, scope);
      case "icon": return changePropertyIcon(request.propertyId, request.icon, scope);
      case "changeType": return changePropertyType(request.propertyId, request.propertyType, scope);
      case "delete": return deleteProperty(request.propertyId, scope);
      case "insert":
      case "duplicate": return placeProperty(request, scope);
    }
  }

  /** Add a property at the end of the source. Failures are rethrown for the caller to show. */
  async function addPropertyFromView(type: NotesDataSourcePropertyType, rawName: string, icon: string | null, requestedScope?: NotesDatabaseSourceEditingScope): Promise<void> {
    if (schemaLoadPromise) await schemaLoadPromise;
    const sourceId = requestedScope?.dataSourceId ?? schemaDataSourceId;
    if (!sourceId) throw new Error(t("notes.databaseSchemaLoadFailed", ""));
    const scope = requestedScope ?? { dataSourceId: sourceId, databaseId, viewId: schemaViewId };
    return enqueue(async () => adoptResult(await appendNotesDataSourceSchemaProperty(actionContext(scope), type, rawName, icon)));
  }

  /** Sources a relation can point to, with the edited source first. */
  function relationDataSources(): NotesDataSource[] {
    const currentSchema = schema;
    if (!currentSchema) return availableDataSources;
    return [
      currentSchema.data_source,
      ...availableDataSources.filter((source) => source.id !== currentSchema.data_source.id),
    ];
  }
</script>

<section
  class="my-5 min-w-0 space-y-1"
  aria-label={t("notes.blockType.childDatabase")}
>
  <div class="flex min-w-0 items-center gap-2 px-1">
    {#if reference?.is_linked && onOpenDatabase}
      <button type="button" class="inline-flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={t("notes.databaseOpenSource")} title={t("notes.databaseOpenSource")} onclick={() => void openDatabase(reference!.source_block_id)}>
        <ArrowUpRight class="size-5" strokeWidth={1.75} aria-hidden="true" />
      </button>
    {/if}
    {#if isLocalDatabase}
      <input
        bind:this={titleInput}
        data-notes-database-title
        class="min-h-10 min-w-0 flex-1 border-0 bg-transparent px-0 text-[1.4rem] font-semibold leading-tight text-foreground outline-none placeholder:text-muted-foreground/50"
        aria-label={t("notes.databaseTitle")}
        placeholder={t("notes.databaseTitlePlaceholder")}
        bind:value={titleDraft}
        disabled={titleSaving || editingLocked}
        onfocus={() => onFocusBlock(block.id)}
        onblur={() => { void saveTitle(); }}
        onkeydown={(event) => {
          if (!event.isComposing && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey
            && (event.key === "ArrowUp" || event.key === "ArrowDown")) return;
          event.stopPropagation();
          if (event.key === "Enter") { event.preventDefault(); event.currentTarget.blur(); }
        }}
      />
    {:else}
      <Database class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <button type="button" class="min-h-9 min-w-0 flex-1 truncate text-left text-base font-semibold" onkeydown={onKeydown} onclick={() => onFocusBlock(block.id)}>{title || t("notes.untitled")}</button>
      <span class="text-[0.8rem] text-muted-foreground">{t("notes.childDatabasePreserved")}</span>
    {/if}
    {#if isLocalDatabase}
      <CollectionSaveIndicator pending={saving || titleSaving || linking || viewSaving} label={t("notes.databaseSaving")} />
      <CollectionMenu label={t("notes.databaseMore")} kind="actions" iconOnly showHeader={false}>
        {#if onOpenDatabase}
          <CollectionMenuItem icon={ArrowUpRight} label={t("notes.databaseOpen")} onclick={() => void openDatabase(block.id)} />
        {/if}
        <CollectionMenuItem icon={Copy} label={t("notes.databaseCopyLink")} onclick={() => void copyDatabaseLink()} />
        {#if onDeleteDatabase}
          <CollectionMenuSeparator />
          <CollectionMenuItem icon={Trash2} label={t("notes.databaseMoveToTrash")} destructive onclick={() => void deleteDatabase()} />
        {/if}
      </CollectionMenu>
    {/if}
  </div>
  {#if titleError}
    <Notice class="px-1 text-[0.8rem]" message={titleError} />
  {/if}
  {#if linkedViewError}
    <Notice class="mt-2 text-[0.8rem]" message={linkedViewError} />
  {/if}

  {#if isLocalDatabase && dataSourceId}
    <NotesDatabaseViewSurface
      onReady={reportReady}
      onSavingChange={(pending) => { viewSaving = pending; }}
      {dataSourceId}
      {databaseId}
      initialViewId={viewId}
      {onSelectPage}
      onEditProperties={openProperties}
      {propertyEditor}
      onLoadPropertyEditor={preparePropertyEditor}
      onCreateLinkedDatabaseView={() => { void createLinkedView(); }}
      onAddProperty={addPropertyFromView}
      onPropertyAction={applyPropertyAction}
      onCopyViewLink={copyDatabaseLink}
      onOpenFullPage={onOpenDatabase ? () => void openDatabase(block.id) : undefined}
      {editingLocked}
      onEditingLockChange={(locked) => { lockRevision += 1; editingLocked = locked; if (reference) reference = { ...reference, editing_locked: locked }; }}
      reloadKeys={{
        table: tableReloadKey,
        board: boardReloadKey,
        gallery: galleryReloadKey,
        list: listReloadKey,
        calendar: calendarReloadKey,
        timeline: timelineReloadKey,
      }}
    />
  {/if}
</section>

{#snippet schemaEditor(property: NotesDataSourceSchemaPropertyDraft)}
  <NotesDatabasePropertySchemaEditor {property} {properties} dataSources={relationDataSources()} dataSourceId={schemaDataSourceId}
    disabled={loading || editingLocked}
    onUpdate={(update) => editProperty(property.id, update)}
    onOptionEdit={(edit) => editOptions(property.id, edit)} />
{/snippet}

{#snippet panelMessages()}
  {#if editingLocked}<p class="px-2 py-1 text-panel-detail text-muted-foreground">{t("notes.databaseEditingLockDescription")}</p>{/if}
  {#if error}<Notice class="px-2 py-1 text-panel-detail" message={error} />{/if}
{/snippet}

{#snippet propertyEditor(propertyId: string)}
  <div class="grid min-w-0 gap-0" data-schema-property-id={propertyId}>
    {#if schema && editorSourceId === propertyEditorSourceId}
      {#each properties.filter((property) => property.id === propertyId) as property (property.id)}
        {@render schemaEditor(property)}
      {/each}
    {:else if loading}
      <Loading layout="rows" rows={2} label={t("notes.databaseSchemaLoading")} />
    {/if}
    {@render panelMessages()}
  </div>
{/snippet}

{#snippet propertyPage(property: NotesDataSourceSchemaPropertyDraft)}
  {@const unavailable = loading || editingLocked}
  <div class="grid min-w-0 gap-0" data-schema-property-id={property.id}>
    <CollectionPropertyNameField propertyId={property.id} name={property.name} kind={notesPropertyKind(property.type)} icon={property.icon}
      editable={!editingLocked} maxLength={NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS}
      onRename={(name) => withEditorScope((scope) => renameProperty(property.id, name, scope))}
      onIconChange={(icon) => withEditorScope((scope) => changePropertyIcon(property.id, icon, scope))} />
    {#if notesPropertyHasSchemaOptions(property.type)}
      <CollectionMenu label={t("collections.property.editProperty")} kind="properties" icon={Settings2} fullWidth disabled={unavailable}>
        {@render schemaEditor(property)}
        {@render panelMessages()}
      </CollectionMenu>
    {/if}
    {#if property.type !== "title"}
      <CollectionPropertyTypeMenu types={propertyTypeOptions} current={property.type} disabled={unavailable}
        onChange={(type) => runPanelAction((scope) => changePropertyType(property.id, type, scope))} />
      <CollectionMenuSeparator />
      <CollectionMenuItem icon={Copy} label={t("collections.property.duplicate")} disabled={unavailable}
        onclick={() => runPanelAction((scope) => placeProperty({ type: "duplicate", propertyId: property.id }, scope))} />
      <CollectionMenuItem icon={Trash2} label={t("collections.property.delete")} destructive disabled={unavailable}
        onclick={() => runPanelAction((scope) => deleteProperty(property.id, scope))} />
    {/if}
    {@render panelMessages()}
  </div>
{/snippet}

{#if isLocalDatabase && expanded}
  <CollectionSettings label={t("notes.databaseViewEditProperties")} anchor={schemaAnchor} onClose={() => { expanded = false; }}>
    {#if !schema && loading}
      <Loading layout="rows" rows={3} label={t("notes.databaseSchemaLoading")} />
    {/if}
    {#each properties as property (property.id)}
      <CollectionMenu label={property.name} kind="property" fullWidth row summary={notesPropertyTypeLabel(property.type, t)}>
        {#snippet leading()}<CollectionPropertyIcon icon={property.icon} kind={notesPropertyKind(property.type)} class="size-3.5 shrink-0 text-muted-foreground" />{/snippet}
        {@render propertyPage(property)}
      </CollectionMenu>
    {/each}
    {#if schema && !editingLocked}
      <CollectionMenuSeparator />
      <CollectionPropertyCreator types={propertyTypeOptions} pending={saving}
        onCreate={(type) => runPanelAction((scope) => addPropertyFromView(type, "", null, scope))} />
    {/if}
    {@render panelMessages()}
  </CollectionSettings>
{/if}

{#if pendingDelete}
  {@const request = pendingDelete}
  <ConfirmDialog title={t("collections.property.deleteTitle")} message={t("collections.property.deleteMessage", request.name)}
    confirmLabel={t("collections.property.delete")} cancelLabel={t("common.cancel")}
    onConfirm={() => request.resolve(true)} onCancel={() => request.resolve(false)} />
{/if}
