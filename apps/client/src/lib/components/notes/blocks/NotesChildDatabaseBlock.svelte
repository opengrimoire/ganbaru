<script lang="ts">
  import CollectionSaveIndicator from "$lib/components/collections/CollectionSaveIndicator.svelte";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { tick, untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    applyNotesDataSourcePropertyAction,
    getNotesDataSourceSchema,
    getNotesDatabaseReference,
    listNotesDatabaseViews,
    listNotesDataSources,
    renameNotesDatabase,
    updateNotesDataSourceSchema,
  } from "$lib/api/notes";
  import NotesDatabaseRollupSchemaControls from "$lib/components/notes/database/NotesDatabaseRollupSchemaControls.svelte";
  import NotesDatabaseViewSurface from "$lib/components/notes/database/NotesDatabaseViewSurface.svelte";
  import { notesPropertyTypeLabel } from "$lib/components/notes/database/property-kinds";
  import {
    createNotesDataSourcePropertyDraft,
    defaultNotesDataSourcePropertyName,
    notesDataSourceButtonTargetOptions,
    notesDataSourceDefaultButtonPatch,
    notesDataSourceDefaultRollupPatch,
    notesDataSourceRollupRelationOptions,
    notesDataSourceRollupTargetOptions,
    notesDataSourceRollupTargetOptionsForRelation,
    notesDataSourceSchemaDraftFromDto,
    notesDataSourceSchemaUpdateFromDraft,
    notesDataSourceDuplicatePropertyName,
    notesDataSourceRenameProperty,
    notesDataSourceSyncPropertyReferences,
    type NotesDataSourceSchemaOptionDraft,
    type NotesDataSourceSchemaPropertyDraft,
    type NotesDatabasePropertyActionRequest,
    type NotesDatabaseSourceEditingScope,
  } from "$lib/notes/database/data-source-schema";
  import {
    NOTES_DATA_SOURCE_NUMBER_FORMATS,
    NOTES_DATA_SOURCE_PROPERTY_TYPES,
    NOTES_DATA_SOURCE_SELECT_COLORS,
    NOTES_DATA_SOURCE_STATUS_GROUPS,
    type NotesChildDatabaseBlock,
    type NotesDataSource,
    type NotesDataSourceNumberFormat,
    type NotesDataSourcePropertyType,
    type NotesDataSourcePropertyAction,
    type NotesDataSourceSchema,
    type NotesDataSourceSelectColor,
    type NotesDataSourceStatusGroup,
    type NotesDatabaseReference,
  } from "$lib/notes/types";
  import Database from "@lucide/svelte/icons/database";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import Copy from "@lucide/svelte/icons/copy";
  import { buildNotesBlockLink } from "$lib/notes/links/block-link";
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
  let selectedPropertyId = $state<string | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let linking = $state(false);
  let error = $state<string | null>(null);
  let linkedViewError = $state<string | null>(null);
  let saved = $state(false);
  let dirty = $state(false);
  let schema = $state<NotesDataSourceSchema | null>(null);
  let schemaViewId = $state<string | null>(null);
  let editorSourceId = $state<string | null>(null);
  /** The data source whose property a column's Edit property submenu shows. */
  let propertyEditorSourceId = $state<string | null>(null);
  let editingLocked = $state(untrack(() => block.child_database.editing_locked ?? false));
  const retainedSchemaDrafts = new Map<string, { properties: NotesDataSourceSchemaPropertyDraft[]; dirty: boolean; selectedPropertyId: string | null }>();
  let schemaLoadPromise: Promise<void> | null = null;
  let schemaRequestId = 0;
  let lockRevision = 0;
  let incomingLock = untrack(() => block.child_database.editing_locked ?? false);
  let availableDataSources = $state<NotesDataSource[]>([]);
  let properties = $state<NotesDataSourceSchemaPropertyDraft[]>([]);
  let newPropertyType = $state<NotesDataSourcePropertyType>("rich_text");
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
  const propertyCount = $derived(properties.length);
  const schemaDataSourceId = $derived(editorSourceId ?? dataSourceId);

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

  async function openDatabase(id: string): Promise<void> {
    try { await onOpenDatabase?.(id); }
    catch (caught: unknown) { titleError = caught instanceof Error ? caught.message : String(caught); }
  }

  async function deleteDatabase(): Promise<void> {
    try { await onDeleteDatabase?.(block.id); }
    catch (caught: unknown) { titleError = caught instanceof Error ? caught.message : String(caught); }
  }

  async function copyDatabaseLink(): Promise<void> {
    try {
      const destination = reference ?? await getNotesDatabaseReference(block.id);
      await navigator.clipboard.writeText(buildNotesBlockLink(window.location.href, { pageId: destination.page_id, blockId: destination.block_id }));
    } catch (caught: unknown) { titleError = caught instanceof Error ? caught.message : String(caught); }
  }

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (!contentReady || focusBlockId !== block.id) return;
    void tick().then(() => {
      if (focusBlockId === block.id && _focusRequestId === focusRequestId
        && !titleInput?.closest("[inert], [hidden]")) titleInput?.focus({ preventScroll: true });
    });
  });

  function loadSchema(scope?: NotesDatabaseSourceEditingScope, request = ++schemaRequestId): Promise<void> {
    if (request !== schemaRequestId) return Promise.resolve();
    if (schemaLoadPromise) return schemaLoadPromise.then(() => request === schemaRequestId ? loadSchema(scope, request) : undefined);
    const sourceId = scope?.dataSourceId ?? schemaDataSourceId;
    if (!sourceId) return Promise.resolve();
    schemaLoadPromise = performLoadSchema(sourceId, scope?.viewId ?? null, request).finally(() => { schemaLoadPromise = null; });
    return schemaLoadPromise;
  }

  /** Reveal the full schema editor from view settings. */
  function openProperties(anchor?: HTMLElement | null, scope?: NotesDatabaseSourceEditingScope): void {
    if (editingLocked || saving) return;
    schemaAnchor = anchor ?? titleInput;
    expanded = true;
    selectSchemaProperty(undefined, scope);
  }

  /** Load the schema a column's Edit property submenu edits and select that column's property. */
  function preparePropertyEditor(propertyId: string, scope?: NotesDatabaseSourceEditingScope): void {
    propertyEditorSourceId = scope?.dataSourceId ?? schemaDataSourceId;
    if (!editingLocked && !saving) selectSchemaProperty(propertyId, scope);
  }

  /** Load the scope's schema when it is not the one being edited, then select the requested property. */
  function selectSchemaProperty(propertyId: string | undefined, scope?: NotesDatabaseSourceEditingScope): void {
    const request = ++schemaRequestId;
    if (schema && (!scope || scope.dataSourceId === editorSourceId)) {
      loading = false;
      if (propertyId && properties.some((property) => property.id === propertyId)) selectedPropertyId = propertyId;
      if (availableDataSources.length === 0) void loadSourceMetadata(request);
      return;
    }
    void loadSchema(scope, request).then(() => {
      if (request !== schemaRequestId || (scope && scope.dataSourceId !== editorSourceId)) return;
      if (propertyId && properties.some((property) => property.id === propertyId)) selectedPropertyId = propertyId;
    });
  }

  /** Load relation target sources when the schema was cached by a header addition before the editor first opened. */
  async function loadSourceMetadata(request: number): Promise<void> {
    loading = true;
    try {
      const sources = await listNotesDataSources();
      if (request === schemaRequestId) availableDataSources = sources;
    } catch (caught: unknown) {
      if (request === schemaRequestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (request === schemaRequestId) loading = false;
    }
  }

  async function performLoadSchema(sourceId: string, requestedViewId: string | null, request: number): Promise<void> {
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
      if (editorSourceId && editorSourceId !== sourceId) retainedSchemaDrafts.set(editorSourceId, { properties, dirty, selectedPropertyId });
      const retained = sourceId !== editorSourceId ? retainedSchemaDrafts.get(sourceId) : undefined;
      schema = loaded;
      editorSourceId = sourceId;
      schemaViewId = loaded.view.id;
      availableDataSources = dataSources;
      properties = retained?.dirty ? retained.properties : notesDataSourceSchemaDraftFromDto(loaded.data_source, loaded.view);
      selectedPropertyId = retained?.selectedPropertyId ?? properties[0]?.id ?? null;
      dirty = retained?.dirty ?? false;
      saved = false;
      reloadViews();
    } catch (caught) {
      if (request === schemaRequestId) error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (request === schemaRequestId) loading = false;
    }
  }

  async function saveSchema(nextProperties: NotesDataSourceSchemaPropertyDraft[] = properties): Promise<boolean> {
    if (!schemaDataSourceId || editingLocked || saving) return false;
    saving = true;
    error = null;
    try {
      const update = notesDataSourceSchemaUpdateFromDraft(nextProperties);
      const updated = await updateNotesDataSourceSchema(schemaDataSourceId, update, { databaseId, viewId: schemaViewId });
      schema = updated;
      properties = notesDataSourceSchemaDraftFromDto(updated.data_source, updated.view);
      dirty = false;
      saved = true;
      reloadViews();
      return true;
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
      return false;
    } finally {
      saving = false;
    }
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
      titleError = caught instanceof Error ? caught.message : String(caught);
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
      const message = caught instanceof Error ? caught.message : String(caught);
      linkedViewError = t("notes.databaseLinkedViewCreateFailed", message);
    } finally {
      linking = false;
    }
  }

  function markDirty(next: NotesDataSourceSchemaPropertyDraft[]): void {
    properties = next;
    dirty = true;
    saved = false;
  }

  function updateProperty(
    propertyId: string,
    patch: Partial<NotesDataSourceSchemaPropertyDraft>,
  ): void {
    const nextProperties = properties.map((property) => {
      if (property.id !== propertyId) return property;
      const nextType = patch.type ?? property.type;
      const next: NotesDataSourceSchemaPropertyDraft = {
        ...property,
        ...patch,
        type: nextType,
      };
      if (nextType === "status" && next.options.length === 0) {
        next.options = createNotesDataSourcePropertyDraft("status", "Status").options;
      }
      if (nextType !== "select" && nextType !== "multi_select" && nextType !== "status") {
        next.options = [];
      }
      if (nextType === "relation" && !next.relationDataSourceId) {
        next.relationDataSourceId = schemaDataSourceId ?? "";
      }
      if (nextType !== "relation") {
        next.relationDataSourceId = "";
        next.relationSyncedPropertyId = "";
        next.relationSyncedPropertyName = "";
      }
      if (nextType === "rollup" && !next.rollupRelationPropertyId) {
        Object.assign(next, notesDataSourceDefaultRollupPatch(
          properties,
          property.id,
          schemaDataSourceId,
          rollupDataSources(),
        ));
      }
      if (nextType === "formula" && !next.formulaExpression.trim()) {
        next.formulaExpression = defaultFormulaExpression(property.id);
      }
      if (nextType === "button") {
        if (!next.buttonLabel.trim()) next.buttonLabel = next.name || t("notes.databaseSchemaButtonDefaultLabel");
        if (!next.buttonActionPropertyId) {
          Object.assign(next, notesDataSourceDefaultButtonPatch(properties, property.id));
        }
      }
      if (nextType !== "rollup") {
        next.rollupRelationPropertyId = "";
        next.rollupRelationPropertyName = "";
        next.rollupPropertyId = "";
        next.rollupPropertyName = "";
        next.rollupFunction = "count";
      }
      if (nextType !== "formula") {
        next.formulaExpression = "";
      }
      if (nextType !== "button") {
        next.buttonLabel = "";
        next.buttonRequiresConfirmation = false;
        next.buttonActionPropertyId = "";
        next.buttonActionPropertyName = "";
        next.buttonActionPropertyType = "checkbox";
        next.buttonActionValue = true;
      }
      return next;
    });
    markDirty(notesDataSourceSyncPropertyReferences(nextProperties, schemaDataSourceId, rollupDataSources()));
  }

  function addProperty(): void {
    const name = defaultNotesDataSourcePropertyName(newPropertyType, properties);
    const property = createNotesDataSourcePropertyDraft(newPropertyType, name);
    if (newPropertyType === "relation") {
      property.relationDataSourceId = schemaDataSourceId ?? "";
    }
    if (newPropertyType === "rollup") {
      Object.assign(property, notesDataSourceDefaultRollupPatch(
        properties,
        property.id,
        schemaDataSourceId,
        rollupDataSources(),
      ));
    }
    if (newPropertyType === "formula") {
      property.formulaExpression = defaultFormulaExpression(property.id);
    }
    if (newPropertyType === "button") {
      Object.assign(property, notesDataSourceDefaultButtonPatch(properties, property.id));
    }
    markDirty([...properties, property]);
    selectedPropertyId = property.id;
  }

  /** Add to canonical schema without committing or discarding unsaved property editor drafts. */
  async function addPropertyFromView(type: NotesDataSourcePropertyType, rawName: string, icon: string | null, requestedScope?: NotesDatabaseSourceEditingScope): Promise<void> {
    if (schemaLoadPromise) await schemaLoadPromise;
    const sourceId = requestedScope?.dataSourceId ?? schemaDataSourceId;
    if (!sourceId) throw new Error(t("notes.databaseSchemaLoadFailed", ""));
    if (saving || editingLocked) throw new Error(t("notes.databaseSaving"));
    saving = true;
    error = null;
    try {
      const scope = requestedScope ?? { databaseId, viewId: schemaViewId };
      const canonical = await getNotesDataSourceSchema(sourceId, scope);
      if (type === "rollup") availableDataSources = await listNotesDataSources();
      const canonicalProperties = notesDataSourceSchemaDraftFromDto(canonical.data_source, canonical.view);
      const name = rawName.trim() || defaultNotesDataSourcePropertyName(type, canonicalProperties);
      const property = createNotesDataSourcePropertyDraft(type, name, undefined, icon);
      if (type === "relation") property.relationDataSourceId = sourceId;
      if (type === "rollup") Object.assign(property, notesDataSourceDefaultRollupPatch(canonicalProperties, property.id, sourceId, availableDataSources));
      if (type === "formula") property.formulaExpression = defaultFormulaExpression(property.id, canonicalProperties);
      if (type === "button") Object.assign(property, notesDataSourceDefaultButtonPatch(canonicalProperties, property.id));
      const update = notesDataSourceSchemaUpdateFromDraft([...canonicalProperties, property]);
      const updated = await updateNotesDataSourceSchema(sourceId, update, scope);
      const canonicalDrafts = notesDataSourceSchemaDraftFromDto(updated.data_source, updated.view);
      const created = canonicalDrafts.find((candidate) => candidate.id === property.id);
      if (!created) throw new Error(t("notes.databaseSchemaSaveFailed", property.name));
      mergeCanonicalSchema(updated, (drafts) => [...drafts, created]);
      reloadViews();
    } catch (caught: unknown) {
      error = caught instanceof Error ? caught.message : String(caught);
      throw new Error(error);
    } finally {
      saving = false;
    }
  }

  /** Adopt a canonical schema change in the matching source draft, applying it to unsaved local drafts instead of discarding them. */
  function mergeCanonicalSchema(updated: NotesDataSourceSchema, applyToDrafts: (drafts: NotesDataSourceSchemaPropertyDraft[]) => NotesDataSourceSchemaPropertyDraft[]): void {
    const sourceId = updated.data_source.id;
    if (!editorSourceId || editorSourceId === sourceId) {
      const shouldRetainDrafts = editorSourceId === sourceId && dirty;
      schema = updated;
      editorSourceId = sourceId;
      schemaViewId = updated.view.id;
      properties = shouldRetainDrafts ? applyToDrafts(properties) : notesDataSourceSchemaDraftFromDto(updated.data_source, updated.view);
      dirty = shouldRetainDrafts;
      saved = !shouldRetainDrafts;
    } else {
      const retained = retainedSchemaDrafts.get(sourceId);
      if (retained?.dirty) retainedSchemaDrafts.set(sourceId, { ...retained, properties: applyToDrafts(retained.properties) });
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

  /**
   * Change one canonical property from its header, keeping its ID and any unsaved property editor drafts.
   * `edit` returns the change for the canonical drafts, or null when nothing changes.
   * Failures are rethrown without a block error because the header shows them beside the control.
   */
  async function editPropertyFromView(propertyId: string, scope: NotesDatabaseSourceEditingScope,
    edit: (drafts: NotesDataSourceSchemaPropertyDraft[]) => Partial<Pick<NotesDataSourceSchemaPropertyDraft, "name" | "icon">> | null): Promise<void> {
    if (saving || editingLocked) throw new Error(t("notes.databaseSaving"));
    if (schemaLoadPromise) await schemaLoadPromise;
    saving = true;
    try {
      const canonical = await getNotesDataSourceSchema(scope.dataSourceId, scope);
      const drafts = notesDataSourceSchemaDraftFromDto(canonical.data_source, canonical.view);
      const patch = edit(drafts);
      if (!patch) return;
      const apply = (list: NotesDataSourceSchemaPropertyDraft[]) => list.map((property) => property.id === propertyId ? { ...property, ...patch } : property);
      const sources = [canonical.data_source, ...availableDataSources.filter((source) => source.id !== canonical.data_source.id)];
      const synced = notesDataSourceSyncPropertyReferences(apply(drafts), scope.dataSourceId, sources);
      const updated = await updateNotesDataSourceSchema(scope.dataSourceId, notesDataSourceSchemaUpdateFromDraft(synced), scope);
      mergeCanonicalSchema(updated, (current) => notesDataSourceSyncPropertyReferences(apply(current), scope.dataSourceId, sources));
      reloadViews();
    } finally {
      saving = false;
    }
  }

  /** Rename one property from its header; duplicate names are rejected ignoring case. */
  function renamePropertyFromView(propertyId: string, rawName: string, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return editPropertyFromView(propertyId, scope, (drafts) => {
      const renamed = notesDataSourceRenameProperty(drafts, propertyId, rawName);
      if (renamed.status === "unchanged") return null;
      if (renamed.status !== "renamed") throw new Error(renamed.status === "duplicate" ? t("collections.property.nameExists") : t("notes.databaseSchemaSaveFailed", propertyId));
      return { name: renamed.properties.find((property) => property.id === propertyId)?.name ?? rawName.trim() };
    });
  }

  /** Change one property's icon from its header; null returns it to the type icon. */
  function changePropertyIconFromView(propertyId: string, icon: string | null, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    return editPropertyFromView(propertyId, scope, (drafts) => {
      const target = drafts.find((property) => property.id === propertyId);
      if (!target) throw new Error(t("notes.databaseSchemaSaveFailed", propertyId));
      return target.icon === icon ? null : { icon };
    });
  }

  /** Apply a header schema change and its requested placement in one native transaction. */
  async function applyPropertyAction(request: NotesDatabasePropertyActionRequest, scope: NotesDatabaseSourceEditingScope): Promise<void> {
    if (request.type === "rename") return renamePropertyFromView(request.propertyId, request.name, scope);
    if (request.type === "icon") return changePropertyIconFromView(request.propertyId, request.icon, scope);
    if (saving || editingLocked) throw new Error(t("notes.databaseSaving"));
    if (schemaLoadPromise) await schemaLoadPromise;
    saving = true;
    error = null;
    try {
      const canonical = await getNotesDataSourceSchema(scope.dataSourceId, scope);
      if (request.type === "insert" && request.propertyType === "rollup") availableDataSources = await listNotesDataSources();
      const drafts = notesDataSourceSchemaDraftFromDto(canonical.data_source, canonical.view);
      let action: NotesDataSourcePropertyAction;
      if (request.type === "duplicate") {
        const original = drafts.find((property) => property.id === request.propertyId);
        if (!original) throw new Error(t("notes.databaseSchemaSaveFailed", request.propertyId));
        const name = notesDataSourceDuplicatePropertyName(original.name, drafts,
          (propertyName) => t("notes.databaseSchemaDuplicateName", propertyName));
        action = { type: "duplicate", property_id: request.propertyId, name };
      } else {
        const property = createNotesDataSourcePropertyDraft(request.propertyType, request.name.trim() || defaultNotesDataSourcePropertyName(request.propertyType, drafts), undefined, request.icon);
        if (property.type === "relation") property.relationDataSourceId = scope.dataSourceId;
        if (property.type === "rollup") Object.assign(property, notesDataSourceDefaultRollupPatch(drafts, property.id, scope.dataSourceId, availableDataSources));
        if (property.type === "formula") property.formulaExpression = defaultFormulaExpression(property.id, drafts);
        if (property.type === "button") Object.assign(property, notesDataSourceDefaultButtonPatch(drafts, property.id));
        const serialized = notesDataSourceSchemaUpdateFromDraft([property]);
        const definition = serialized.properties[property.name];
        if (typeof definition !== "object" || definition === null || Array.isArray(definition)) throw new Error(t("notes.databaseSchemaSaveFailed", property.name));
        action = { type: "insert", property_id: request.propertyId, side: request.side, property: definition as Record<string, unknown> };
      }
      if (!scope.databaseId || !scope.viewId) throw new Error(t("notes.databaseSchemaLoadFailed", ""));
      const result = await applyNotesDataSourcePropertyAction(scope.dataSourceId, scope.databaseId, scope.viewId, action);
      const created = notesDataSourceSchemaDraftFromDto(result.schema.data_source, result.schema.view).find((property) => property.id === result.property_id);
      if (!created) throw new Error(t("notes.databaseSchemaSaveFailed", result.property_id));
      mergeCanonicalSchema(result.schema, (drafts) => [...drafts, created]);
      reloadViews();
    } catch (caught: unknown) {
      error = caught instanceof Error ? caught.message : String(caught);
      throw new Error(error);
    } finally {
      saving = false;
    }
  }

  function deleteProperty(propertyId: string): void {
    const next = properties.filter((property) => property.id !== propertyId || property.type === "title");
    markDirty(next);
    if (selectedPropertyId === propertyId) selectedPropertyId = next[0]?.id ?? null;
  }

  function addOption(propertyId: string): void {
    const property = properties.find((item) => item.id === propertyId);
    if (!property) return;
    const option: NotesDataSourceSchemaOptionDraft = {
      id: crypto.randomUUID(),
      name: `Option ${property.options.length + 1}`,
      color: "default",
      group: "To-do",
    };
    updateProperty(propertyId, { options: [...property.options, option] });
  }

  function updateOption(
    propertyId: string,
    optionId: string,
    patch: Partial<NotesDataSourceSchemaOptionDraft>,
  ): void {
    const property = properties.find((item) => item.id === propertyId);
    if (!property) return;
    updateProperty(propertyId, {
      options: property.options.map((option) =>
        option.id === optionId ? { ...option, ...patch } : option,
      ),
    });
  }

  function deleteOption(propertyId: string, optionId: string): void {
    const property = properties.find((item) => item.id === propertyId);
    if (!property) return;
    updateProperty(propertyId, {
      options: property.options.filter((option) => option.id !== optionId),
    });
  }

  function propertyTypeLabel(type: NotesDataSourcePropertyType): string {
    return notesPropertyTypeLabel(type, t);
  }

  function updateButtonTarget(
    property: NotesDataSourceSchemaPropertyDraft,
    targetId: string,
  ): void {
    const target = notesDataSourceButtonTargetOptions(properties, property.id)
      .find((candidate) => candidate.id === targetId) ?? null;
    updateProperty(property.id, {
      buttonActionPropertyId: target?.id ?? "",
      buttonActionPropertyName: target?.name ?? "",
      buttonActionPropertyType: target?.type ?? "checkbox",
      buttonActionValue: target ? defaultButtonValue(target.type) : true,
    });
  }

  function updateButtonValue(
    property: NotesDataSourceSchemaPropertyDraft,
    value: string | boolean,
  ): void {
    updateProperty(property.id, { buttonActionValue: value });
  }

  function defaultButtonValue(type: NotesDataSourcePropertyType): string | boolean {
    return type === "checkbox" ? true : "";
  }

  function buttonActionValueText(value: unknown): string {
    return typeof value === "string" || typeof value === "number" ? String(value) : "";
  }

  function defaultFormulaExpression(excludePropertyId: string, sourceProperties: readonly NotesDataSourceSchemaPropertyDraft[] = properties): string {
    const target = sourceProperties.find((property) =>
      property.id !== excludePropertyId
      && property.type !== "formula"
      && property.type !== "rollup"
    );
    return target ? `prop("${escapeFormulaPropertyName(target.name)}")` : "\"\"";
  }

  function escapeFormulaPropertyName(name: string): string {
    return name.replace(/\\/g, "\\\\").replace(/"/g, "\\\"");
  }

  function dataSourceTitle(source: NotesDataSource): string {
    return source.title.trim() || t("notes.untitled");
  }

  function rollupDataSources(): NotesDataSource[] {
    const currentSchema = schema;
    if (!currentSchema) return availableDataSources;
    return [
      currentSchema.data_source,
      ...availableDataSources.filter((source) => source.id !== currentSchema.data_source.id),
    ];
  }

  function updateRollupRelation(property: NotesDataSourceSchemaPropertyDraft, relationId: string): void {
    const relation = properties.find((candidate) =>
      candidate.id === relationId && candidate.type === "relation"
    ) ?? null;
    const target = relation
      ? notesDataSourceRollupTargetOptionsForRelation(
          relation,
          properties,
          schemaDataSourceId,
          rollupDataSources(),
        )[0] ?? null
      : null;
    updateProperty(property.id, {
      rollupRelationPropertyId: relation?.id ?? "",
      rollupRelationPropertyName: relation?.name ?? "",
      rollupPropertyId: target?.id ?? "",
      rollupPropertyName: target?.name ?? "",
    });
  }

  function updateRollupTarget(property: NotesDataSourceSchemaPropertyDraft, targetId: string): void {
    const target = notesDataSourceRollupTargetOptions(
      property,
      properties,
      schemaDataSourceId,
      rollupDataSources(),
    ).find((option) => option.id === targetId) ?? null;
    updateProperty(property.id, {
      rollupPropertyId: target?.id ?? "",
      rollupPropertyName: target?.name ?? "",
    });
  }

  function statusMessage(): string {
    if (loading) return t("notes.databaseSchemaLoading");
    if (saving) return t("notes.databaseSchemaSave");
    if (dirty) return t("notes.databaseSchemaUnsaved");
    if (saved) return t("notes.databaseSchemaSaved");
    return t("notes.databaseSchemaPropertyCount", propertyCount);
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
    <p class="px-1 text-[0.8rem] text-destructive" role="alert">{titleError}</p>
  {/if}
  {#if linkedViewError}
    <p class="mt-2 text-[0.8rem] text-destructive">{linkedViewError}</p>
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

{#snippet schemaStatus()}
  <div class="flex min-w-0 flex-wrap items-center gap-2 text-panel-detail text-muted-foreground">
    <span class="min-w-0 flex-1 truncate" role="status">
      {#if error}
        {schema
          ? t("notes.databaseSchemaSaveFailed", error)
          : t("notes.databaseSchemaLoadFailed", error)}
      {:else}
        {statusMessage()}
      {/if}
    </span>
    <button
      type="button"
      class="inline-flex h-8 items-center gap-1 rounded-md px-2 hover:bg-accent disabled:pointer-events-none"
      disabled={loading || saving}
      aria-label={t("notes.databaseSchemaReload")}
      title={t("notes.databaseSchemaReload")}
      onclick={() => {
        void loadSchema();
      }}
    >
      <RefreshCw class="size-3.5" aria-hidden="true" />
    </button>
    <button
      type="button"
      class="inline-flex h-8 items-center gap-1 rounded-md bg-primary px-2 text-primary-foreground disabled:pointer-events-none"
      disabled={loading || saving || !dirty}
      onclick={() => {
        void saveSchema();
      }}
    >
      <Save class="size-3.5" aria-hidden="true" />
      <span>{t("notes.databaseSchemaSave")}</span>
    </button>
  </div>
{/snippet}

{#snippet propertyFields(property: NotesDataSourceSchemaPropertyDraft, standalone: boolean)}
  <div class="grid min-w-0 gap-3 @container">
    <div class={["grid min-w-0 gap-2", standalone ? "@lg:grid-cols-[minmax(7rem,1fr)_minmax(7rem,12rem)_auto]" : "grid-cols-[minmax(0,1fr)_auto]"]}>
      {#if standalone}
        <label class="min-w-0 text-panel-detail text-muted-foreground">
          <span class="mb-1 block">{t("notes.databaseSchemaName")}</span>
          <input
            class="field w-full min-w-0 text-foreground"
            value={property.name}
            aria-label={t("notes.databaseSchemaName")}
            disabled={loading || saving || editingLocked}
            oninput={(event) => {
              updateProperty(property.id, {
                name: event.currentTarget.value,
              });
            }}
          />
        </label>
      {/if}
      <div class="min-w-0 text-panel-detail text-muted-foreground">
        <span class="mb-1 block">{t("notes.databaseSchemaType")}</span>
        <Select textSize="collection"
          inline
          appearance="quiet"
          contentAlign="start"
          class="w-full min-w-0"
          ariaLabel={t("notes.databaseSchemaType")}
          value={String(property.type ?? "")}
          disabled={loading || saving || editingLocked || property.type === "title"}
          options={NOTES_DATA_SOURCE_PROPERTY_TYPES.filter((type) => property.type === "title" ? type === "title" : type !== "title").map((type) => ({ value: type, label: propertyTypeLabel(type) }))}
          onChange={(nextValue) => {
            updateProperty(property.id, {
                type: nextValue as NotesDataSourcePropertyType,
            });
          }}
        />
      </div>
      <div class="flex min-w-0 items-end justify-end gap-1">
        <button
          type="button"
          class="inline-flex size-8 items-center justify-center rounded-md text-destructive hover:bg-destructive/10 disabled:pointer-events-none"
          disabled={loading || saving || editingLocked || property.type === "title"}
          aria-label={t("notes.databaseSchemaDelete")}
          title={t("notes.databaseSchemaDelete")}
          onclick={() => deleteProperty(property.id)}
        >
          <Trash2 class="size-3.5" aria-hidden="true" />
        </button>
      </div>
    </div>

    <label class="min-w-0 text-panel-detail text-muted-foreground">
      <span class="mb-1 block">{t("notes.databaseSchemaDescription")}</span>
      <input
        class="field w-full min-w-0 text-foreground"
        value={property.description}
        placeholder={t("notes.databaseSchemaDescriptionPlaceholder")}
        disabled={loading || saving || editingLocked}
        oninput={(event) => {
          updateProperty(property.id, {
            description: event.currentTarget.value,
          });
        }}
      />
    </label>

    {#if property.type === "number"}
      <div class="min-w-0 text-panel-detail text-muted-foreground">
        <span class="mb-1 block">{t("notes.databaseSchemaNumberFormat")}</span>
        <Select textSize="collection"
          inline
          appearance="quiet"
          contentAlign="start"
          class="w-full min-w-0"
          ariaLabel={t("notes.databaseSchemaNumberFormat")}
          value={String(property.numberFormat ?? "")}
          options={[...(NOTES_DATA_SOURCE_NUMBER_FORMATS).map((format) => ({ value: String(format), label: String(format) }))]}
          onChange={(nextValue) => {
            updateProperty(property.id, {
                numberFormat: nextValue as NotesDataSourceNumberFormat,
            });
          }}
        />
      </div>
    {:else if property.type === "unique_id"}
      <label class="min-w-0 text-panel-detail text-muted-foreground">
        <span class="mb-1 block">{t("notes.databaseSchemaUniquePrefix")}</span>
        <input
          class="field w-full min-w-0 text-foreground"
          value={property.uniquePrefix}
          placeholder={t("notes.databaseSchemaUniquePrefixPlaceholder")}
          oninput={(event) => {
            updateProperty(property.id, {
              uniquePrefix: event.currentTarget.value,
            });
          }}
        />
      </label>
    {:else if property.type === "relation"}
      <div class="grid min-w-0 gap-2 @lg:grid-cols-3">
        <div class="min-w-0 text-panel-detail text-muted-foreground">
          <span class="mb-1 block">{t("notes.databaseSchemaRelationTarget")}</span>
          <Select textSize="collection"
            inline
            appearance="quiet"
            contentAlign="start"
            class="w-full min-w-0"
            ariaLabel={t("notes.databaseSchemaRelationTarget")}
            value={String(property.relationDataSourceId ?? "")}
            options={[...(property.relationDataSourceId && !availableDataSources.some((source) => source.id === property.relationDataSourceId) ? [{ value: String(property.relationDataSourceId), label: String(property.relationDataSourceId) }] : []),
              ...(availableDataSources).map((source) => ({ value: String(source.id), label: String(dataSourceTitle(source)) }))]}
            onChange={(nextValue) => {
              updateProperty(property.id, {
                  relationDataSourceId: nextValue,
              });
            }}
          />
        </div>
        <label class="min-w-0 text-panel-detail text-muted-foreground">
          <span class="mb-1 block">{t("notes.databaseSchemaRelationSyncedPropertyId")}</span>
          <input
            class="field w-full min-w-0 text-foreground"
            value={property.relationSyncedPropertyId}
            placeholder={t("notes.databaseSchemaRelationSyncedPropertyIdPlaceholder")}
            oninput={(event) => {
              updateProperty(property.id, {
                relationSyncedPropertyId: event.currentTarget.value,
              });
            }}
          />
        </label>
        <label class="min-w-0 text-panel-detail text-muted-foreground">
          <span class="mb-1 block">{t("notes.databaseSchemaRelationSyncedPropertyName")}</span>
          <input
            class="field w-full min-w-0 text-foreground"
            value={property.relationSyncedPropertyName}
            placeholder={t("notes.databaseSchemaRelationSyncedPropertyNamePlaceholder")}
            oninput={(event) => {
              updateProperty(property.id, {
                relationSyncedPropertyName: event.currentTarget.value,
              });
            }}
          />
        </label>
      </div>
    {:else if property.type === "rollup"}
      <NotesDatabaseRollupSchemaControls
        {property}
        relations={notesDataSourceRollupRelationOptions(properties, property.id)}
        targets={notesDataSourceRollupTargetOptions(
          property,
          properties,
          schemaDataSourceId,
          rollupDataSources(),
        )}
        {saving}
        onRelationChange={(relationId) => updateRollupRelation(property, relationId)}
        onTargetChange={(targetId) => updateRollupTarget(property, targetId)}
        onFunctionChange={(rollupFunction) => updateProperty(property.id, { rollupFunction })}
      />
    {:else if property.type === "formula"}
      <label class="min-w-0 text-panel-detail text-muted-foreground">
        <span class="mb-1 block">{t("notes.databaseSchemaFormulaExpression")}</span>
        <textarea
          class="field min-h-20 w-full min-w-0 resize-y font-mono text-foreground"
          value={property.formulaExpression}
          placeholder={t("notes.databaseSchemaFormulaExpressionPlaceholder")}
          disabled={loading || saving || editingLocked}
          oninput={(event) => {
            updateProperty(property.id, {
              formulaExpression: event.currentTarget.value,
            });
          }}
        ></textarea>
      </label>
    {:else if property.type === "button"}
      <div class="grid min-w-0 gap-2 @lg:grid-cols-3">
        <label class="min-w-0 text-panel-detail text-muted-foreground">
          <span class="mb-1 block">{t("notes.databaseSchemaButtonLabel")}</span>
          <input
            class="field w-full min-w-0 text-foreground"
            value={property.buttonLabel}
            placeholder={t("notes.databaseSchemaButtonDefaultLabel")}
            oninput={(event) => {
              updateProperty(property.id, {
                buttonLabel: event.currentTarget.value,
              });
            }}
          />
        </label>
        <div class="min-w-0 text-panel-detail text-muted-foreground">
          <span class="mb-1 block">{t("notes.databaseSchemaButtonTarget")}</span>
          <Select textSize="collection"
            inline
            appearance="quiet"
            contentAlign="start"
            class="w-full min-w-0"
            ariaLabel={t("notes.databaseSchemaButtonTarget")}
            value={String(property.buttonActionPropertyId ?? "")}
            options={[{ value: "", label: t("notes.databaseSchemaButtonNoAction") },
              ...(notesDataSourceButtonTargetOptions(properties, property.id)).map((target) => ({ value: String(target.id), label: String(target.name) }))]}
            onChange={(nextValue) => updateButtonTarget(property, nextValue)}
          />
        </div>
        <label class="flex min-w-0 items-end gap-2 text-panel-detail text-muted-foreground">
          <Checkbox
            class="mb-2"
            checked={property.buttonRequiresConfirmation}
            onChange={(checked) => {
              updateProperty(property.id, {
                buttonRequiresConfirmation: checked,
              });
            }}
          />
          <span class="pb-1">{t("notes.databaseSchemaButtonConfirm")}</span>
        </label>
        {#if property.buttonActionPropertyId}
          {#if property.buttonActionPropertyType === "checkbox"}
            <div class="min-w-0 text-panel-detail text-muted-foreground">
              <span class="mb-1 block">{t("notes.databaseSchemaButtonValue")}</span>
              <Select textSize="collection"
                inline
                appearance="quiet"
                contentAlign="start"
                class="w-full min-w-0"
                ariaLabel={t("notes.databaseSchemaButtonValue")}
                value={String(property.buttonActionValue === false ? "false" : "true")}
                options={[{ value: "true", label: t("notes.databaseSchemaButtonValueChecked") },
                  { value: "false", label: t("notes.databaseSchemaButtonValueUnchecked") }]}
                onChange={(nextValue) => updateButtonValue(property, nextValue === "true")}
              />
            </div>
          {:else}
            <label class="min-w-0 text-panel-detail text-muted-foreground @lg:col-span-2">
              <span class="mb-1 block">{t("notes.databaseSchemaButtonValue")}</span>
              <input
                class="field w-full min-w-0 text-foreground"
                value={buttonActionValueText(property.buttonActionValue)}
                placeholder={t("notes.databaseSchemaButtonValuePlaceholder")}
                oninput={(event) => updateButtonValue(property, event.currentTarget.value)}
              />
            </label>
          {/if}
        {/if}
      </div>
    {:else if property.type === "select" || property.type === "multi_select" || property.type === "status"}
      <div class="space-y-2">
        {#each property.options as option (option.id)}
          <div class="grid min-w-0 gap-2 @lg:grid-cols-[minmax(7rem,1fr)_minmax(7rem,10rem)_minmax(7rem,10rem)_auto]">
            <label class="min-w-0 text-panel-detail text-muted-foreground">
              <span class="mb-1 block">{t("notes.databaseSchemaOptionName")}</span>
              <input
                class="field w-full min-w-0 text-foreground"
                value={option.name}
                oninput={(event) => {
                  updateOption(property.id, option.id, {
                    name: event.currentTarget.value,
                  });
                }}
              />
            </label>
            <div class="min-w-0 text-panel-detail text-muted-foreground">
              <span class="mb-1 block">{t("notes.databaseSchemaOptionColor")}</span>
              <Select textSize="collection"
                inline
                appearance="quiet"
                contentAlign="start"
                class="w-full min-w-0"
                ariaLabel={t("notes.databaseSchemaOptionColor")}
                value={String(option.color ?? "")}
                options={[...(NOTES_DATA_SOURCE_SELECT_COLORS).map((color) => ({ value: String(color), label: String(color) }))]}
                onChange={(nextValue) => {
                  updateOption(property.id, option.id, {
                      color: nextValue as NotesDataSourceSelectColor,
                  });
                }}
              />
            </div>
            {#if property.type === "status"}
              <div class="min-w-0 text-panel-detail text-muted-foreground">
                <span class="mb-1 block">{t("notes.databaseSchemaOptionGroup")}</span>
                <Select textSize="collection"
                  inline
                  appearance="quiet"
                  contentAlign="start"
                  class="w-full min-w-0"
                  ariaLabel={t("notes.databaseSchemaOptionGroup")}
                  value={String(option.group ?? "")}
                  options={[...(NOTES_DATA_SOURCE_STATUS_GROUPS).map((group) => ({ value: String(group), label: String(group) }))]}
                  onChange={(nextValue) => {
                    updateOption(property.id, option.id, {
                        group: nextValue as NotesDataSourceStatusGroup,
                    });
                  }}
                />
              </div>
            {/if}
            <div class="flex items-end justify-end">
              <button
                type="button"
                class="inline-flex size-8 items-center justify-center rounded-md text-destructive hover:bg-destructive/10"
                aria-label={t("notes.databaseSchemaDeleteOption")}
                title={t("notes.databaseSchemaDeleteOption")}
                onclick={() => deleteOption(property.id, option.id)}
              >
                <Trash2 class="size-3.5" aria-hidden="true" />
              </button>
            </div>
          </div>
        {/each}
        <button
          type="button"
          class="inline-flex h-8 items-center gap-1 rounded-md px-2 hover:bg-accent"
          onclick={() => addOption(property.id)}
        >
          <Plus class="size-3.5" aria-hidden="true" />
          <span>{t("notes.databaseSchemaAddOption")}</span>
        </button>
      </div>
    {:else if property.type === "title"}
      <p class="text-panel-detail text-muted-foreground">{t("notes.databaseSchemaReadOnlyTitle")}</p>
    {:else}
      <p class="text-panel-detail text-muted-foreground">
        {t("notes.databaseSchemaReadonlyPlaceholder")}
      </p>
    {/if}
  </div>
{/snippet}

{#snippet propertyEditor(propertyId: string)}
  <fieldset disabled={loading || saving || editingLocked} class="m-0 min-w-0 space-y-2 border-0 p-0" data-schema-property-id={propertyId}>
    <legend class="sr-only">{t("collections.property.editProperty")}</legend>
    {#if editingLocked}<p class="text-muted-foreground">{t("notes.databaseEditingLockDescription")}</p>{/if}
    {#if schema && editorSourceId === propertyEditorSourceId}
      {#each properties.filter((property) => property.id === propertyId) as property (property.id)}
        {@render propertyFields(property, false)}
      {/each}
    {/if}
    {@render schemaStatus()}
  </fieldset>
{/snippet}

{#if isLocalDatabase && expanded}
  <CollectionSettings label={t("notes.databaseViewEditProperties")} anchor={schemaAnchor} preferredWidth={384} onClose={() => { expanded = false; }}>
      {#if editingLocked}<p class="text-muted-foreground">{t("notes.databaseEditingLockDescription")}</p>{/if}
      <fieldset disabled={loading || saving || editingLocked} class="m-0 min-w-0 space-y-2 border-0 p-0">
      <legend class="sr-only">{t("notes.databaseViewEditProperties")}</legend>
      {@render schemaStatus()}

      <Select textSize="collection" inline appearance="quiet" class="w-full min-w-0" ariaLabel={t("notes.databaseSchemaToggle")}
        value={selectedPropertyId ?? ""} options={properties.map((property) => ({ value: property.id, label: property.name || t("notes.databaseSchemaName") }))}
        onChange={(propertyId) => { selectedPropertyId = propertyId; }} />
      <div class="space-y-2">
        {#each properties.filter((property) => property.id === selectedPropertyId) as property (property.id)}
          {@render propertyFields(property, true)}
        {/each}
      </div>

      <div class="flex min-w-0 flex-wrap items-center gap-2 border-t border-border pt-3">
        <Select textSize="collection"
          inline
          appearance="quiet"
          contentAlign="start"
          class="w-full min-w-0"
          ariaLabel={t("notes.databaseSchemaType")}
          value={String(newPropertyType ?? "")}
          options={[...(NOTES_DATA_SOURCE_PROPERTY_TYPES.filter((type) => type !== "title")).map((type) => ({ value: String(type), label: String(propertyTypeLabel(type)) }))]}
          onChange={(nextValue) => {
            newPropertyType = nextValue as NotesDataSourcePropertyType;
          }}
        />
        <button
          type="button"
          class="inline-flex h-8 items-center gap-1 rounded-md px-2 hover:bg-accent"
          onclick={addProperty}
        >
          <Plus class="size-3.5" aria-hidden="true" />
          <span>{t("collections.property.add")}</span>
        </button>
      </div>

      </fieldset>
  </CollectionSettings>
{/if}
