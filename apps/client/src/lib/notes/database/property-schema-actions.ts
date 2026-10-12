import {
  applyNotesDataSourcePropertyAction,
  getNotesDataSourceSchema,
  listNotesDataSources,
  updateNotesDataSourceSchema,
} from "$lib/api/notes";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { NotesDataSource, NotesDataSourcePropertyAction, NotesDataSourcePropertyType, NotesDataSourceSchema } from "$lib/notes/types";
import {
  defaultNotesDataSourcePropertyName,
  notesDataSourceDuplicatePropertyName,
  notesDataSourceRenameProperty,
  notesDataSourceSchemaDraftFromDto,
  notesDataSourceSchemaUpdateFromDraft,
  notesDataSourceSyncPropertyReferences,
  type NotesDataSourcePropertyUpdate,
  type NotesDataSourceSchemaPropertyDraft,
  type NotesDatabaseSourceEditingScope,
} from "./data-source-schema";
import {
  notesDataSourcePropertyDefinition,
  planNewNotesDataSourceProperty,
  planNotesDataSourceTypeChange,
  type NotesDataSourcePropertyPlan,
} from "./property-planning";

/** What every property schema action reads besides its own arguments. */
export interface NotesDataSourceSchemaActionContext {
  /** Source, database, and view the change is saved in. */
  scope: NotesDatabaseSourceEditingScope;
  /** Sources a relation or rollup can point to, as last loaded by the caller. */
  dataSources: readonly NotesDataSource[];
  t: Translate;
}

/** A saved schema, with the source list when the action had to reload it. */
export interface NotesDataSourceSchemaActionResult {
  schema: NotesDataSourceSchema;
  /** Whether a change was saved; false when the newest saved schema already matched the request. */
  changed: boolean;
  /** Present when the action reloaded the sources a rollup reads through. */
  dataSources?: NotesDataSource[];
}

/** Insert request placing a new property beside another one in the view. */
export interface NotesDataSourcePropertyInsert {
  besidePropertyId: string;
  side: "left" | "right";
  propertyType: NotesDataSourcePropertyType;
  name: string;
  icon: string | null;
}

/**
 * Read the newest saved schema and its property drafts. Every action starts from it, so edits made in quick
 * succession build on each other instead of on a stale copy.
 */
export async function loadNotesDataSourceSchemaDrafts(
  scope: NotesDatabaseSourceEditingScope,
): Promise<{ schema: NotesDataSourceSchema; drafts: NotesDataSourceSchemaPropertyDraft[] }> {
  const schema = await getNotesDataSourceSchema(scope.dataSourceId, scope);
  return { schema, drafts: notesDataSourceSchemaDraftFromDto(schema.data_source, schema.view) };
}

/**
 * Save a whole-schema change built from the newest saved drafts. References that name other properties, such as
 * rollup and button targets, are refreshed before saving.
 *
 * @param transform Returns the next drafts, or null to save nothing.
 */
export async function updateNotesDataSourceSchemaDrafts(
  context: NotesDataSourceSchemaActionContext,
  transform: (drafts: readonly NotesDataSourceSchemaPropertyDraft[]) => NotesDataSourceSchemaPropertyDraft[] | null,
): Promise<NotesDataSourceSchemaActionResult> {
  const { scope } = context;
  const { schema, drafts } = await loadNotesDataSourceSchemaDrafts(scope);
  const next = transform(drafts);
  if (!next) return { schema, changed: false };
  const sources = [schema.data_source, ...context.dataSources.filter((source) => source.id !== schema.data_source.id)];
  const synced = notesDataSourceSyncPropertyReferences(next, scope.dataSourceId, sources);
  const updated = await updateNotesDataSourceSchema(scope.dataSourceId, notesDataSourceSchemaUpdateFromDraft(synced), scope);
  return { schema: updated, changed: true };
}

/**
 * Apply one property edit to the newest saved schema.
 *
 * @param update Reads the saved property and returns the fields to change, or null to save nothing.
 */
export function updateNotesDataSourceSchemaProperty(
  context: NotesDataSourceSchemaActionContext,
  propertyId: string,
  update: NotesDataSourcePropertyUpdate,
): Promise<NotesDataSourceSchemaActionResult> {
  return updateNotesDataSourceSchemaDrafts(context, (drafts) => {
    const target = drafts.find((property) => property.id === propertyId);
    if (!target) throw new Error(context.t("notes.databaseSchemaSaveFailed", propertyId));
    const patch = update(target, drafts);
    return patch ? drafts.map((property) => property.id === propertyId ? { ...property, ...patch } : property) : null;
  });
}

/** Rename one property; names that already exist, ignoring case, are rejected. */
export function renameNotesDataSourceSchemaProperty(
  context: NotesDataSourceSchemaActionContext,
  propertyId: string,
  rawName: string,
): Promise<NotesDataSourceSchemaActionResult> {
  const { t } = context;
  return updateNotesDataSourceSchemaProperty(context, propertyId, (_target, drafts) => {
    const renamed = notesDataSourceRenameProperty(drafts, propertyId, rawName);
    if (renamed.status === "unchanged") return null;
    if (renamed.status !== "renamed") {
      throw new Error(renamed.status === "duplicate" ? t("collections.property.nameExists") : t("notes.databaseSchemaSaveFailed", propertyId));
    }
    return { name: renamed.properties.find((property) => property.id === propertyId)?.name ?? rawName.trim() };
  });
}

/** Change one property's icon; null returns it to its type icon. */
export function changeNotesDataSourceSchemaPropertyIcon(
  context: NotesDataSourceSchemaActionContext,
  propertyId: string,
  icon: string | null,
): Promise<NotesDataSourceSchemaActionResult> {
  return updateNotesDataSourceSchemaProperty(context, propertyId, (target) => target.icon === icon ? null : { icon });
}

/**
 * Convert one property to another type in a native transaction that also converts its row values. Select-like types
 * collect their options from the existing values, so they send no configuration.
 */
export async function changeNotesDataSourceSchemaPropertyType(
  context: NotesDataSourceSchemaActionContext,
  propertyId: string,
  propertyType: NotesDataSourcePropertyType,
): Promise<NotesDataSourceSchemaActionResult> {
  const { scope, t } = context;
  const target = notesDataSourceSchemaActionTarget(context);
  const { drafts } = await loadNotesDataSourceSchemaDrafts(scope);
  const current = drafts.find((property) => property.id === propertyId);
  if (!current) throw new Error(t("notes.databaseSchemaSaveFailed", propertyId));
  const dataSources = propertyType === "rollup" ? await listNotesDataSources() : undefined;
  const plan = planNotesDataSourceTypeChange(current, propertyType, {
    dataSourceId: scope.dataSourceId,
    drafts,
    dataSources: dataSources ?? context.dataSources,
  });
  if (plan.status !== "ready") throw new Error(t("notes.databaseSchemaRollupNoRelations"));
  const result = await applyNotesDataSourcePropertyAction(scope.dataSourceId, target.databaseId, target.viewId,
    { type: "change_type", property_id: propertyId, property_type: propertyType, config: plan.config });
  return { schema: result.schema, changed: true, dataSources };
}

/** Delete one property and its values from every row. The caller asks for confirmation first. */
export async function deleteNotesDataSourceSchemaProperty(
  context: NotesDataSourceSchemaActionContext,
  propertyId: string,
): Promise<NotesDataSourceSchemaActionResult> {
  const target = notesDataSourceSchemaActionTarget(context);
  const result = await applyNotesDataSourcePropertyAction(context.scope.dataSourceId, target.databaseId, target.viewId,
    { type: "delete", property_id: propertyId });
  return { schema: result.schema, changed: true };
}

/**
 * Duplicate one property with its configuration under a localized copy name. The copy lands right after the
 * original, in the view and on row pages.
 */
export async function duplicateNotesDataSourceSchemaProperty(
  context: NotesDataSourceSchemaActionContext,
  propertyId: string,
): Promise<NotesDataSourceSchemaActionResult & { propertyId: string }> {
  const { scope, t } = context;
  const target = notesDataSourceSchemaActionTarget(context);
  const { drafts } = await loadNotesDataSourceSchemaDrafts(scope);
  const original = drafts.find((property) => property.id === propertyId);
  if (!original) throw new Error(t("notes.databaseSchemaSaveFailed", propertyId));
  const name = notesDataSourceDuplicatePropertyName(original.name, drafts,
    (propertyName) => t("notes.databaseSchemaDuplicateName", propertyName));
  const action: NotesDataSourcePropertyAction = { type: "duplicate", property_id: propertyId, name };
  const result = await applyNotesDataSourcePropertyAction(scope.dataSourceId, target.databaseId, target.viewId, action);
  return { schema: result.schema, changed: true, propertyId: result.property_id };
}

/** Insert a new property with its type defaults beside another one in the view. */
export async function insertNotesDataSourceSchemaProperty(
  context: NotesDataSourceSchemaActionContext,
  request: NotesDataSourcePropertyInsert,
): Promise<NotesDataSourceSchemaActionResult & { propertyId: string }> {
  const { scope } = context;
  const target = notesDataSourceSchemaActionTarget(context);
  const { drafts } = await loadNotesDataSourceSchemaDrafts(scope);
  const dataSources = request.propertyType === "rollup" ? await listNotesDataSources() : undefined;
  const name = request.name.trim() || defaultNotesDataSourcePropertyName(request.propertyType, drafts);
  const property = notesDataSourcePlannedProperty(context, planNewNotesDataSourceProperty(request.propertyType, name, request.icon,
    { dataSourceId: scope.dataSourceId, drafts, dataSources: dataSources ?? context.dataSources }));
  const result = await applyNotesDataSourcePropertyAction(scope.dataSourceId, target.databaseId, target.viewId, {
    type: "insert",
    property_id: request.besidePropertyId,
    side: request.side,
    property: notesDataSourcePropertyDefinition(property),
  });
  return { schema: result.schema, changed: true, dataSources, propertyId: result.property_id };
}

/**
 * Add a property with its type defaults at the end of the source.
 *
 * @param arrange Places the new property among the saved drafts, such as giving every property a row page position.
 */
export async function appendNotesDataSourceSchemaProperty(
  context: NotesDataSourceSchemaActionContext,
  type: NotesDataSourcePropertyType,
  rawName: string,
  icon: string | null,
  arrange: (drafts: readonly NotesDataSourceSchemaPropertyDraft[], property: NotesDataSourceSchemaPropertyDraft) => NotesDataSourceSchemaPropertyDraft[] =
    (drafts, property) => [...drafts, property],
): Promise<NotesDataSourceSchemaActionResult & { propertyId: string }> {
  const { scope } = context;
  const dataSources = type === "rollup" ? await listNotesDataSources() : undefined;
  let propertyId = "";
  const result = await updateNotesDataSourceSchemaDrafts({ ...context, dataSources: dataSources ?? context.dataSources }, (drafts) => {
    const name = rawName.trim() || defaultNotesDataSourcePropertyName(type, drafts);
    const property = notesDataSourcePlannedProperty(context, planNewNotesDataSourceProperty(type, name, icon,
      { dataSourceId: scope.dataSourceId, drafts, dataSources: dataSources ?? context.dataSources }));
    propertyId = property.id;
    return arrange(drafts, property);
  });
  return { ...result, dataSources, propertyId };
}

/** The database and view a native property action places its change in. */
function notesDataSourceSchemaActionTarget(context: NotesDataSourceSchemaActionContext): { databaseId: string; viewId: string } {
  const { databaseId, viewId } = context.scope;
  if (!databaseId || !viewId) throw new Error(context.t("notes.databaseSchemaLoadFailed", ""));
  return { databaseId, viewId };
}

/** The planned property, or a localized error when a rollup has no relation to read through. */
function notesDataSourcePlannedProperty(
  context: NotesDataSourceSchemaActionContext,
  plan: NotesDataSourcePropertyPlan,
): NotesDataSourceSchemaPropertyDraft {
  if (plan.status !== "ready") throw new Error(context.t("notes.databaseSchemaRollupNoRelations"));
  return plan.property;
}
