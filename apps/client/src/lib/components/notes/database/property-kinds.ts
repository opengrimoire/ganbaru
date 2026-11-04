import type { CollectionPropertyKind, CollectionPropertyTypeOption } from "$lib/components/collections/property-icons";
import type { Translate } from "$lib/i18n/translator.svelte";
import { NOTES_DATA_SOURCE_PROPERTY_TYPES, type NotesDataSourcePropertyType } from "$lib/notes/types";

const PROPERTY_KINDS = {
  title: "title",
  rich_text: "text",
  number: "number",
  select: "select",
  multi_select: "multi_select",
  status: "status",
  date: "date",
  checkbox: "checkbox",
  url: "url",
  email: "email",
  phone_number: "phone",
  files: "files",
  people: "person",
  created_time: "created_time",
  created_by: "created_by",
  last_edited_time: "last_edited_time",
  last_edited_by: "last_edited_by",
  unique_id: "unique_id",
  place: "place",
  relation: "relation",
  rollup: "rollup",
  formula: "formula",
  button: "button",
} as const satisfies Record<NotesDataSourcePropertyType, CollectionPropertyKind>;

const PROPERTY_LABEL_KEYS = {
  title: "notes.databaseSchemaPropertyType.title",
  rich_text: "notes.databaseSchemaPropertyType.richText",
  number: "notes.databaseSchemaPropertyType.number",
  select: "notes.databaseSchemaPropertyType.select",
  multi_select: "notes.databaseSchemaPropertyType.multiSelect",
  status: "notes.databaseSchemaPropertyType.status",
  date: "notes.databaseSchemaPropertyType.date",
  checkbox: "notes.databaseSchemaPropertyType.checkbox",
  url: "notes.databaseSchemaPropertyType.url",
  email: "notes.databaseSchemaPropertyType.email",
  phone_number: "notes.databaseSchemaPropertyType.phoneNumber",
  files: "notes.databaseSchemaPropertyType.files",
  people: "notes.databaseSchemaPropertyType.people",
  created_time: "notes.databaseSchemaPropertyType.createdTime",
  created_by: "notes.databaseSchemaPropertyType.createdBy",
  last_edited_time: "notes.databaseSchemaPropertyType.lastEditedTime",
  last_edited_by: "notes.databaseSchemaPropertyType.lastEditedBy",
  unique_id: "notes.databaseSchemaPropertyType.uniqueId",
  place: "notes.databaseSchemaPropertyType.place",
  relation: "notes.databaseSchemaPropertyType.relation",
  rollup: "notes.databaseSchemaPropertyType.rollup",
  formula: "notes.databaseSchemaPropertyType.formula",
  button: "notes.databaseSchemaPropertyType.button",
} as const satisfies Record<NotesDataSourcePropertyType, string>;

/** Picker sections in display order: values people enter, values derived from other data, and values the app records automatically. */
const PROPERTY_TYPE_SECTIONS = {
  basic: ["rich_text", "number", "select", "multi_select", "status", "date", "people", "files", "checkbox", "url", "email", "phone_number"],
  advanced: ["relation", "rollup", "formula", "button", "unique_id", "place"],
  automatic: ["created_time", "created_by", "last_edited_time", "last_edited_by"],
} as const satisfies Record<string, readonly Exclude<NotesDataSourcePropertyType, "title">[]>;

/** Return the shared property kind for a Notes property type, so its header and menu show the icon of its value type. */
export function notesPropertyKind(type: NotesDataSourcePropertyType): CollectionPropertyKind {
  return PROPERTY_KINDS[type];
}

/** Return the localized name of a Notes property type. */
export function notesPropertyTypeLabel(type: NotesDataSourcePropertyType, t: Translate): string {
  return t(PROPERTY_LABEL_KEYS[type]);
}

/**
 * Return the localized name of a type read from stored data, such as a row value written under another type than
 * its property's. A type this app does not know is shown as stored.
 */
export function notesStoredPropertyTypeLabel(type: string, t: Translate): string {
  const known = NOTES_DATA_SOURCE_PROPERTY_TYPES.find((candidate) => candidate === type);
  return known ? notesPropertyTypeLabel(known, t) : type;
}

/** List every type a new Notes property can use, grouped into picker sections; the title property is unique per source. */
export function notesPropertyTypeOptions(t: Translate): CollectionPropertyTypeOption<NotesDataSourcePropertyType>[] {
  return Object.entries(PROPERTY_TYPE_SECTIONS).flatMap(([section, types]) => types.map((type) => ({
    value: type,
    label: notesPropertyTypeLabel(type, t),
    kind: PROPERTY_KINDS[type],
    section,
  })));
}

/** Property types whose schema carries configuration that people edit after creation, such as options, formats, or references. */
const SCHEMA_OPTION_TYPES = new Set<NotesDataSourcePropertyType>(["number", "select", "multi_select", "status", "unique_id", "relation", "rollup", "formula", "button"]);

/** Return whether a Notes property type has schema configuration to show under Edit property. */
export function notesPropertyHasSchemaOptions(type: NotesDataSourcePropertyType): boolean {
  return SCHEMA_OPTION_TYPES.has(type);
}
