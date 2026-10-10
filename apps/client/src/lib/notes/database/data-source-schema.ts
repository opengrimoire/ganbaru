import {
  NOTES_DATA_SOURCE_NUMBER_FORMATS,
  NOTES_DATA_SOURCE_PROPERTY_TYPES,
  NOTES_DATA_SOURCE_ROLLUP_FUNCTIONS,
  NOTES_DATA_SOURCE_SELECT_COLORS,
  NOTES_DATA_SOURCE_STATUS_GROUPS,
  type NotesDataSource,
  type NotesDataSourceNumberFormat,
  type NotesDataSourcePropertyType,
  type NotesDataSourceRollupFunction,
  type NotesDataSourceSchemaUpdate,
  type NotesDataSourceSelectColor,
  type NotesDataSourceStatusGroup,
  type NotesDatabaseView,
} from "$lib/notes/types";
import { notesPageIconFromPickerValue, notesPageIconPickerValue } from "$lib/notes/pages/icon-picker";
import { isNotesIconColor } from "$lib/notes/validation/assets";

type UnknownRecord = Record<string, unknown>;

/** Match the native schema property's Unicode character limit. */
export const NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS = 120;

/** Contextual schema action requested by a property header in the active table. */
export type NotesDatabasePropertyActionRequest =
  | { type: "insert"; propertyId: string; side: "left" | "right"; propertyType: NotesDataSourcePropertyType; name: string; icon: string | null }
  | { type: "duplicate"; propertyId: string }
  | { type: "rename"; propertyId: string; name: string }
  | { type: "icon"; propertyId: string; icon: string | null };

/** Outcome of renaming one property draft from a table header. */
export type NotesDataSourcePropertyRename =
  | { status: "unchanged" | "missing" | "duplicate" }
  | { status: "renamed"; properties: NotesDataSourceSchemaPropertyDraft[] };

/** Source and saved view that originated a nonmodal property action. */
export interface NotesDatabaseSourceEditingScope {
  dataSourceId: string;
  databaseId: string | null;
  viewId: string | null;
}

export interface NotesDataSourceSchemaOptionDraft {
  id: string;
  name: string;
  color: NotesDataSourceSelectColor;
  group: NotesDataSourceStatusGroup;
}

export interface NotesDataSourceSchemaPropertyDraft {
  id: string;
  name: string;
  /** A `lucide:` picker value, or null to show the type icon. */
  icon: string | null;
  description: string;
  type: NotesDataSourcePropertyType;
  numberFormat: NotesDataSourceNumberFormat;
  uniquePrefix: string;
  relationDataSourceId: string;
  relationSyncedPropertyId: string;
  relationSyncedPropertyName: string;
  rollupRelationPropertyId: string;
  rollupRelationPropertyName: string;
  rollupPropertyId: string;
  rollupPropertyName: string;
  rollupFunction: NotesDataSourceRollupFunction;
  formulaExpression: string;
  buttonLabel: string;
  buttonRequiresConfirmation: boolean;
  buttonActionPropertyId: string;
  buttonActionPropertyName: string;
  buttonActionPropertyType: NotesDataSourcePropertyType;
  buttonActionValue: unknown;
  options: NotesDataSourceSchemaOptionDraft[];
}

export interface NotesDataSourceRollupTargetOption {
  id: string;
  name: string;
  type: NotesDataSourcePropertyType;
}

const DEFAULT_OPTION_COLORS: NotesDataSourceSelectColor[] = [
  "default",
  "blue",
  "green",
  "yellow",
  "orange",
  "purple",
  "pink",
  "red",
  "gray",
  "brown",
];

const EMPTY_OBJECT_TYPES = new Set<NotesDataSourcePropertyType>([
  "title",
  "rich_text",
  "date",
  "checkbox",
  "url",
  "email",
  "phone_number",
  "files",
  "people",
  "created_time",
  "created_by",
  "last_edited_time",
  "last_edited_by",
  "place",
]);

const BUTTON_EDITABLE_PROPERTY_TYPES = new Set<NotesDataSourcePropertyType>([
  "title",
  "rich_text",
  "number",
  "select",
  "multi_select",
  "status",
  "date",
  "checkbox",
  "url",
  "email",
  "phone_number",
  "place",
  "relation",
]);

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readString(value: unknown, fallback: string): string {
  return typeof value === "string" ? value : fallback;
}

function isPropertyType(value: string): value is NotesDataSourcePropertyType {
  return NOTES_DATA_SOURCE_PROPERTY_TYPES.includes(value as NotesDataSourcePropertyType);
}

function isSelectColor(value: string): value is NotesDataSourceSelectColor {
  return NOTES_DATA_SOURCE_SELECT_COLORS.includes(value as NotesDataSourceSelectColor);
}

function isNumberFormat(value: string): value is NotesDataSourceNumberFormat {
  return NOTES_DATA_SOURCE_NUMBER_FORMATS.includes(value as NotesDataSourceNumberFormat);
}

function isRollupFunction(value: string): value is NotesDataSourceRollupFunction {
  return NOTES_DATA_SOURCE_ROLLUP_FUNCTIONS.includes(value as NotesDataSourceRollupFunction);
}

function isStatusGroup(value: string): value is NotesDataSourceStatusGroup {
  return NOTES_DATA_SOURCE_STATUS_GROUPS.includes(value as NotesDataSourceStatusGroup);
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function dataSourceById(
  dataSources: readonly NotesDataSource[],
  id: string,
): NotesDataSource | null {
  return dataSources.find((source) => source.id === id) ?? null;
}

function dataSourceRollupTargetOptions(
  source: NotesDataSource | null,
): NotesDataSourceRollupTargetOption[] {
  if (!source) return [];
  return Object.entries(source.properties).flatMap(([key, value]) => {
    if (!isRecord(value)) return [];
    const rawType = readString(value.type, "rich_text");
    if (!isPropertyType(rawType)) return [];
    const id = rawType === "title" ? "title" : readString(value.id, key);
    if (!id || rawType === "rollup" || rawType === "formula" || rawType === "button") return [];
    return [{
      id,
      name: readString(value.name, key),
      type: rawType,
    }];
  });
}

function draftRollupTargetOptions(
  drafts: readonly NotesDataSourceSchemaPropertyDraft[],
): NotesDataSourceRollupTargetOption[] {
  return drafts
    .filter((property) =>
      property.type !== "rollup"
      && property.type !== "formula"
      && property.type !== "button"
    )
    .map((property) => ({
      id: property.id,
      name: property.name,
      type: property.type,
    }));
}

export function notesDataSourceRollupRelationOptions(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
  excludePropertyId: string,
): NotesDataSourceSchemaPropertyDraft[] {
  return properties.filter((property) =>
    property.id !== excludePropertyId
    && property.type === "relation"
    && property.relationDataSourceId.trim().length > 0
  );
}

export function notesDataSourceRollupTargetOptions(
  property: NotesDataSourceSchemaPropertyDraft,
  drafts: readonly NotesDataSourceSchemaPropertyDraft[],
  currentDataSourceId: string | null,
  dataSources: readonly NotesDataSource[],
): NotesDataSourceRollupTargetOption[] {
  const relation = drafts.find((candidate) =>
    candidate.id === property.rollupRelationPropertyId
    && candidate.type === "relation"
  ) ?? null;
  if (!relation) return [];
  return notesDataSourceRollupTargetOptionsForRelation(
    relation,
    drafts,
    currentDataSourceId,
    dataSources,
  );
}

export function notesDataSourceRollupTargetOptionsForRelation(
  relation: NotesDataSourceSchemaPropertyDraft,
  drafts: readonly NotesDataSourceSchemaPropertyDraft[],
  currentDataSourceId: string | null,
  dataSources: readonly NotesDataSource[],
): NotesDataSourceRollupTargetOption[] {
  if (relation.relationDataSourceId === currentDataSourceId) {
    return draftRollupTargetOptions(drafts);
  }
  return dataSourceRollupTargetOptions(dataSourceById(dataSources, relation.relationDataSourceId));
}

export function notesDataSourceSyncRollupReferences(
  drafts: NotesDataSourceSchemaPropertyDraft[],
  currentDataSourceId: string | null,
  dataSources: readonly NotesDataSource[],
): NotesDataSourceSchemaPropertyDraft[] {
  return drafts.map((property) => {
    if (property.type !== "rollup") return property;
    const relation = drafts.find((candidate) =>
      candidate.id === property.rollupRelationPropertyId
      && candidate.type === "relation"
    ) ?? null;
    const target = notesDataSourceRollupTargetOptions(
      property,
      drafts,
      currentDataSourceId,
      dataSources,
    ).find((option) => option.id === property.rollupPropertyId) ?? null;
    return {
      ...property,
      rollupRelationPropertyName: relation?.name ?? property.rollupRelationPropertyName,
      rollupPropertyName: target?.name ?? property.rollupPropertyName,
    };
  });
}

export function notesDataSourceButtonTargetOptions(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
  excludePropertyId: string,
): NotesDataSourceSchemaPropertyDraft[] {
  return properties.filter((property) =>
    property.id !== excludePropertyId && BUTTON_EDITABLE_PROPERTY_TYPES.has(property.type)
  );
}

export function notesDataSourceDefaultButtonPatch(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
  excludePropertyId: string,
): Partial<NotesDataSourceSchemaPropertyDraft> {
  const target = defaultButtonTarget(properties, excludePropertyId);
  if (!target) {
    return {
      buttonActionPropertyId: "",
      buttonActionPropertyName: "",
      buttonActionPropertyType: "checkbox",
      buttonActionValue: true,
    };
  }
  return {
    buttonActionPropertyId: target.id,
    buttonActionPropertyName: target.name,
    buttonActionPropertyType: target.type,
    buttonActionValue: defaultButtonActionValue(target.type),
  };
}

export function notesDataSourceSyncPropertyReferences(
  drafts: NotesDataSourceSchemaPropertyDraft[],
  currentDataSourceId: string | null,
  dataSources: readonly NotesDataSource[],
): NotesDataSourceSchemaPropertyDraft[] {
  const rollupSynced = notesDataSourceSyncRollupReferences(
    drafts,
    currentDataSourceId,
    dataSources,
  );
  return rollupSynced.map((property) => {
    if (property.type !== "button" || !property.buttonActionPropertyId) return property;
    const target = rollupSynced.find((candidate) =>
      candidate.id === property.buttonActionPropertyId
      && BUTTON_EDITABLE_PROPERTY_TYPES.has(candidate.type)
    ) ?? null;
    if (!target) {
      return {
        ...property,
        buttonActionPropertyId: "",
        buttonActionPropertyName: "",
        buttonActionPropertyType: "checkbox",
        buttonActionValue: true,
      };
    }
    return {
      ...property,
      buttonActionPropertyName: target.name,
      buttonActionPropertyType: target.type,
      buttonActionValue: buttonActionValueForType(target.type, property.buttonActionValue),
    };
  });
}

export function notesDataSourceDefaultRollupPatch(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
  excludePropertyId: string,
  currentDataSourceId: string | null,
  dataSources: readonly NotesDataSource[],
): Partial<NotesDataSourceSchemaPropertyDraft> {
  const relation = notesDataSourceRollupRelationOptions(properties, excludePropertyId)[0] ?? null;
  if (!relation) {
    return {
      rollupRelationPropertyId: "",
      rollupRelationPropertyName: "",
      rollupPropertyId: "",
      rollupPropertyName: "",
      rollupFunction: "count",
    };
  }
  const target = notesDataSourceRollupTargetOptionsForRelation(
    relation,
    properties,
    currentDataSourceId,
    dataSources,
  )[0] ?? null;
  return {
    rollupRelationPropertyId: relation.id,
    rollupRelationPropertyName: relation.name,
    rollupPropertyId: target?.id ?? "",
    rollupPropertyName: target?.name ?? "",
    rollupFunction: "count",
  };
}

function tableConfiguration(view: NotesDatabaseView): UnknownRecord {
  if (!isRecord(view.configuration)) return {};
  const table = view.configuration.table;
  return isRecord(table) ? table : {};
}

function viewPropertyOrder(view: NotesDatabaseView): string[] {
  return readStringArray(tableConfiguration(view).property_order);
}

function optionDrafts(value: unknown): NotesDataSourceSchemaOptionDraft[] {
  const options = isRecord(value) && Array.isArray(value.options) ? value.options : [];
  return options.filter(isRecord).map((option, index) => {
    const color = readString(option.color, DEFAULT_OPTION_COLORS[index % DEFAULT_OPTION_COLORS.length]);
    const group = readString(option.group, "To-do");
    return {
      id: readString(option.id, crypto.randomUUID()),
      name: readString(option.name, `Option ${index + 1}`),
      color: isSelectColor(color) ? color : "default",
      group: isStatusGroup(group) ? group : "To-do",
    };
  });
}

function statusOptionDrafts(value: unknown): NotesDataSourceSchemaOptionDraft[] {
  if (!isRecord(value)) return defaultStatusOptions();
  const optionGroups = new Map<string, NotesDataSourceStatusGroup>();
  if (Array.isArray(value.groups)) {
    for (const group of value.groups.filter(isRecord)) {
      const groupName = readString(group.name, "To-do");
      const statusGroup = isStatusGroup(groupName) ? groupName : "To-do";
      for (const optionId of readStringArray(group.option_ids)) {
        optionGroups.set(optionId, statusGroup);
      }
    }
  }
  const options = optionDrafts(value).map((option) => ({
    ...option,
    group: optionGroups.get(option.id) ?? option.group,
  }));
  return options.length > 0 ? options : defaultStatusOptions();
}

function defaultStatusOptions(): NotesDataSourceSchemaOptionDraft[] {
  return [
    {
      id: crypto.randomUUID(),
      name: "Not started",
      color: "default",
      group: "To-do",
    },
    {
      id: crypto.randomUUID(),
      name: "In progress",
      color: "blue",
      group: "In progress",
    },
    {
      id: crypto.randomUUID(),
      name: "Done",
      color: "green",
      group: "Complete",
    },
  ];
}

function defaultButtonTarget(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
  excludePropertyId: string,
): NotesDataSourceSchemaPropertyDraft | null {
  return properties.find((property) =>
    property.id !== excludePropertyId
    && BUTTON_EDITABLE_PROPERTY_TYPES.has(property.type)
    && property.type === "checkbox"
  ) ?? null;
}

function buttonConfigAction(config: unknown): UnknownRecord | null {
  if (!isRecord(config) || !Array.isArray(config.actions)) return null;
  return config.actions.filter(isRecord).find((action) =>
    readString(action.type, "") === "update_current_row_property"
  ) ?? null;
}

function readButtonActionType(value: unknown): NotesDataSourcePropertyType {
  const type = readString(value, "checkbox");
  return isPropertyType(type) && BUTTON_EDITABLE_PROPERTY_TYPES.has(type) ? type : "checkbox";
}

function defaultButtonActionValue(type: NotesDataSourcePropertyType): unknown {
  switch (type) {
    case "checkbox":
      return true;
    case "number":
      return null;
    case "multi_select":
    case "relation":
      return [];
    case "select":
    case "status":
    case "date":
    case "url":
    case "email":
    case "phone_number":
    case "place":
      return null;
    default:
      return "";
  }
}

function buttonActionValueForType(
  type: NotesDataSourcePropertyType,
  value: unknown,
): unknown {
  switch (type) {
    case "checkbox":
      return typeof value === "boolean" ? value : true;
    case "number":
      return typeof value === "number" || value === null || typeof value === "string"
        ? value
        : null;
    case "multi_select":
    case "relation":
      return Array.isArray(value) || typeof value === "string" ? value : [];
    case "select":
    case "status":
    case "date":
    case "url":
    case "email":
    case "phone_number":
    case "place":
      return value === null || typeof value === "string" || isRecord(value) ? value : null;
    default:
      return typeof value === "string" ? value : "";
  }
}

/** Read a property's native icon as a shared picker value; other icon kinds are not property icons and show the type icon. */
export function notesDataSourcePropertyIcon(value: unknown): string | null {
  if (!isRecord(value) || value.type !== "icon" || !isRecord(value.icon)) return null;
  const name = readString(value.icon.name, "");
  if (!name) return null;
  const color = isNotesIconColor(value.icon.color) ? value.icon.color : undefined;
  return notesPageIconPickerValue({ type: "icon", icon: color === undefined ? { name } : { name, color } });
}

/** Serialize a picker value as the native property icon, or null when the property shows its type icon. */
function propertyIconRecord(icon: string | null): UnknownRecord | null {
  if (!icon) return null;
  const native = notesPageIconFromPickerValue(icon, []);
  return native?.type === "icon" ? native : null;
}

function propertyDraftFromRecord(
    key: string,
    value: unknown,
): NotesDataSourceSchemaPropertyDraft | null {
  if (!isRecord(value)) return null;
  const typeValue = readString(value.type, "rich_text");
  const type = isPropertyType(typeValue) ? typeValue : "rich_text";
  const id = type === "title" ? "title" : readString(value.id, crypto.randomUUID());
  const config = value[type];
  const numberConfig = isRecord(config) ? config : {};
  const numberFormat = readString(numberConfig.format, "number");
  const uniquePrefix = readString(isRecord(config) ? config.prefix : "", "");
  const relationDataSourceId = readString(
    isRecord(config) ? config.data_source_id : "",
    "",
  );
  const dualProperty = isRecord(config) && isRecord(config.dual_property)
    ? config.dual_property
    : {};
  const relationSyncedPropertyId = readString(dualProperty.synced_property_id, "");
  const relationSyncedPropertyName = readString(dualProperty.synced_property_name, "");
  const rollupConfig = type === "rollup" && isRecord(config) ? config : {};
  const rollupFunction = readString(rollupConfig.function, "count");
  const formulaConfig = type === "formula" && isRecord(config) ? config : {};
  const buttonConfig = type === "button" && isRecord(config) ? config : {};
  const buttonAction = buttonConfigAction(buttonConfig);
  const buttonActionType = readButtonActionType(buttonAction?.property_type);
  const options = type === "status"
    ? statusOptionDrafts(config)
    : type === "select" || type === "multi_select"
      ? optionDrafts(config)
      : [];
  return {
    id,
    name: readString(value.name, key),
    icon: notesDataSourcePropertyIcon(value.icon),
    description: readString(value.description, ""),
    type,
    numberFormat: isNumberFormat(numberFormat) ? numberFormat : "number",
    uniquePrefix,
    relationDataSourceId,
    relationSyncedPropertyId,
    relationSyncedPropertyName,
    rollupRelationPropertyId: readString(rollupConfig.relation_property_id, ""),
    rollupRelationPropertyName: readString(rollupConfig.relation_property_name, ""),
    rollupPropertyId: readString(rollupConfig.rollup_property_id, ""),
    rollupPropertyName: readString(rollupConfig.rollup_property_name, ""),
    rollupFunction: isRollupFunction(rollupFunction) ? rollupFunction : "count",
    formulaExpression: readString(formulaConfig.expression, ""),
    buttonLabel: readString(buttonConfig.label, readString(value.name, key)),
    buttonRequiresConfirmation: buttonConfig.requires_confirmation === true,
    buttonActionPropertyId: readString(buttonAction?.property_id, ""),
    buttonActionPropertyName: readString(buttonAction?.property_name, ""),
    buttonActionPropertyType: buttonActionType,
    buttonActionValue: buttonActionValueForType(buttonActionType, buttonAction?.value),
    options,
  };
}

export function notesDataSourceSchemaDraftFromDto(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDataSourceSchemaPropertyDraft[] {
  const byId = new Map<string, NotesDataSourceSchemaPropertyDraft>();
  for (const [key, value] of Object.entries(dataSource.properties)) {
    const property = propertyDraftFromRecord(key, value);
    if (property) byId.set(property.id, property);
  }
  const order = viewPropertyOrder(view);
  const sorted: NotesDataSourceSchemaPropertyDraft[] = [];
  for (const propertyId of ["title", ...order.filter((id) => id !== "title")]) {
    const property = byId.get(propertyId);
    if (!property) continue;
    sorted.push(property);
    byId.delete(propertyId);
  }
  sorted.push(...Array.from(byId.values()).sort((left, right) => left.name.localeCompare(right.name)));
  return sorted;
}

export function defaultNotesDataSourcePropertyName(
  type: NotesDataSourcePropertyType,
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
): string {
  const baseName = defaultNameForType(type);
  const existing = new Set(properties.map((property) => property.name.trim().toLocaleLowerCase()));
  if (!existing.has(baseName.toLocaleLowerCase())) return baseName;
  for (let index = 2; index < 1000; index += 1) {
    const candidate = `${baseName} ${index}`;
    if (!existing.has(candidate.toLocaleLowerCase())) return candidate;
  }
  return `${baseName} ${crypto.randomUUID().slice(0, 8)}`;
}

/** Generate a localized copy name within native length and case-insensitive uniqueness bounds. */
export function notesDataSourceDuplicatePropertyName(
  originalName: string,
  properties: readonly Pick<NotesDataSourceSchemaPropertyDraft, "name">[],
  formatCopyName: (name: string) => string,
): string {
  const originalCharacters = Array.from(originalName.trim());
  const existing = new Set(properties.map((property) => property.name.trim().toLowerCase()));
  const copyDecorationLength = Array.from(formatCopyName("")).length;
  for (let index = 1; index <= properties.length + 1; index += 1) {
    const suffix = index === 1 ? "" : ` ${index}`;
    const availableCharacters = NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS
      - copyDecorationLength - Array.from(suffix).length;
    if (availableCharacters <= 0) throw new Error("Copy name decoration exceeds the property name limit");
    const base = originalCharacters.slice(0, availableCharacters).join("");
    const candidate = `${formatCopyName(base).trim()}${suffix}`;
    if (!existing.has(candidate.toLowerCase())) return candidate;
  }
  throw new Error("Could not generate a unique property copy name");
}

function defaultNameForType(type: NotesDataSourcePropertyType): string {
  switch (type) {
    case "title":
      return "Name";
    case "rich_text":
      return "Text";
    case "number":
      return "Number";
    case "select":
      return "Select";
    case "multi_select":
      return "Multi-select";
    case "status":
      return "Status";
    case "date":
      return "Date";
    case "checkbox":
      return "Checkbox";
    case "url":
      return "URL";
    case "email":
      return "Email";
    case "phone_number":
      return "Phone";
    case "files":
      return "Files";
    case "people":
      return "People";
    case "created_time":
      return "Created time";
    case "created_by":
      return "Created by";
    case "last_edited_time":
      return "Last edited time";
    case "last_edited_by":
      return "Last edited by";
    case "unique_id":
      return "Unique ID";
    case "place":
      return "Place";
    case "relation":
      return "Relation";
    case "rollup":
      return "Rollup";
    case "formula":
      return "Formula";
    case "button":
      return "Button";
  }
}

export function createNotesDataSourcePropertyDraft(
  type: NotesDataSourcePropertyType,
  name: string,
  id: string = crypto.randomUUID(),
  icon: string | null = null,
): NotesDataSourceSchemaPropertyDraft {
  const propertyId = type === "title" ? "title" : id;
  return {
    id: propertyId,
    name,
    icon,
    description: "",
    type,
    numberFormat: "number",
    uniquePrefix: "",
    relationDataSourceId: "",
    relationSyncedPropertyId: "",
    relationSyncedPropertyName: "",
    rollupRelationPropertyId: "",
    rollupRelationPropertyName: "",
    rollupPropertyId: "",
    rollupPropertyName: "",
    rollupFunction: "count",
    formulaExpression: "",
    buttonLabel: type === "button" ? name : "",
    buttonRequiresConfirmation: false,
    buttonActionPropertyId: "",
    buttonActionPropertyName: "",
    buttonActionPropertyType: "checkbox",
    buttonActionValue: true,
    options: type === "status" ? defaultStatusOptions() : [],
  };
}

function emptyPropertyConfig(type: NotesDataSourcePropertyType): UnknownRecord {
  if (!EMPTY_OBJECT_TYPES.has(type)) return {};
  return {};
}

function propertyConfig(property: NotesDataSourceSchemaPropertyDraft): UnknownRecord {
  switch (property.type) {
    case "number":
      return { format: property.numberFormat };
    case "select":
    case "multi_select":
      return {
        options: property.options.map(({ id, name, color }) => ({ id, name, color })),
      };
    case "status":
      return {
        options: property.options.map(({ id, name, color }) => ({ id, name, color })),
        groups: NOTES_DATA_SOURCE_STATUS_GROUPS.map((group) => ({
          id: group,
          name: group,
          color: statusGroupColor(group),
          option_ids: property.options
            .filter((option) => option.group === group)
            .map((option) => option.id),
        })),
      };
    case "unique_id":
      return { prefix: property.uniquePrefix.trim() || null };
    case "relation": {
      const dataSourceId = property.relationDataSourceId.trim();
      if (!dataSourceId) throw new Error("relation target data source is required");
      const syncedPropertyId = property.relationSyncedPropertyId.trim();
      const syncedPropertyName = property.relationSyncedPropertyName.trim();
      return {
        data_source_id: dataSourceId,
        dual_property: syncedPropertyId && syncedPropertyName
          ? {
              synced_property_id: syncedPropertyId,
              synced_property_name: syncedPropertyName,
            }
          : null,
      };
    }
    case "rollup": {
      const relationPropertyId = property.rollupRelationPropertyId.trim();
      const relationPropertyName = property.rollupRelationPropertyName.trim();
      const rollupPropertyId = property.rollupPropertyId.trim();
      const rollupPropertyName = property.rollupPropertyName.trim();
      if (!relationPropertyId || !relationPropertyName) {
        throw new Error("rollup relation property is required");
      }
      if (!rollupPropertyId || !rollupPropertyName) {
        throw new Error("rollup target property is required");
      }
      return {
        relation_property_id: relationPropertyId,
        relation_property_name: relationPropertyName,
        rollup_property_id: rollupPropertyId,
        rollup_property_name: rollupPropertyName,
        function: property.rollupFunction,
      };
    }
    case "formula": {
      const expression = property.formulaExpression.trim();
      if (!expression) throw new Error("formula expression is required");
      return { expression };
    }
    case "button": {
      const label = property.buttonLabel.trim() || property.name.trim() || "Run";
      const actions = property.buttonActionPropertyId.trim()
        ? [{
            type: "update_current_row_property",
            property_id: property.buttonActionPropertyId.trim(),
            property_name: property.buttonActionPropertyName.trim(),
            property_type: property.buttonActionPropertyType,
            value: buttonActionValueForType(
              property.buttonActionPropertyType,
              property.buttonActionValue,
            ),
          }]
        : [];
      return {
        label,
        requires_confirmation: property.buttonRequiresConfirmation,
        actions,
      };
    }
    default:
      return emptyPropertyConfig(property.type);
  }
}

function statusGroupColor(group: NotesDataSourceStatusGroup): NotesDataSourceSelectColor {
  switch (group) {
    case "To-do":
      return "gray";
    case "In progress":
      return "blue";
    case "Complete":
      return "green";
  }
}

export function notesDataSourceSchemaUpdateFromDraft(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
): NotesDataSourceSchemaUpdate {
  const record: Record<string, unknown> = {};
  const names = new Set<string>();
  for (const property of properties) {
    const name = property.name.trim();
    if (!name) throw new Error("property name is required");
    const nameKey = name.toLocaleLowerCase();
    if (names.has(nameKey)) throw new Error("property names must be unique");
    names.add(nameKey);
    const icon = propertyIconRecord(property.icon);
    record[name] = {
      id: property.type === "title" ? "title" : property.id,
      name,
      ...(icon ? { icon } : {}),
      description: property.description.trim(),
      type: property.type,
      [property.type]: propertyConfig(property),
    };
  }
  return {
    properties: record,
  };
}

/**
 * Rename one property draft by ID, so views, values, and references that use the ID follow it.
 * The name is trimmed and limited to the native character limit; a blank or unchanged name leaves the schema as is,
 * and another property's name is rejected ignoring case, matching schema saves.
 */
export function notesDataSourceRenameProperty(
  properties: readonly NotesDataSourceSchemaPropertyDraft[],
  propertyId: string,
  rawName: string,
): NotesDataSourcePropertyRename {
  const target = properties.find((property) => property.id === propertyId);
  if (!target) return { status: "missing" };
  const name = Array.from(rawName.trim()).slice(0, NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS).join("").trim();
  if (!name || name === target.name) return { status: "unchanged" };
  const key = name.toLocaleLowerCase();
  if (properties.some((property) => property.id !== propertyId && property.name.trim().toLocaleLowerCase() === key)) return { status: "duplicate" };
  return { status: "renamed", properties: properties.map((property) => property.id === propertyId ? { ...property, name } : property) };
}
