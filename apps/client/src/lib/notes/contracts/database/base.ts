import type { NotesPageCover } from "../assets";
import type { NotesPageIcon, NotesParent, NotesRichText, NotesRichTextAnnotations } from "../core";

export interface NotesDatabaseMentionRichText {
  type: "mention";
  mention: {
    type: "database";
    database: {
      id: string;
    };
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export interface NotesDataSourceTemplate {
  object: "data_source_template";
  id: string;
  data_source_id: string;
  source_page_id: string | null;
  name: string;
  properties: Record<string, unknown>;
  is_default: boolean;
  block_count: number;
  created_time: string;
  last_edited_time: string;
}

export interface NotesDatabaseDataSourceSummary {
  id: string;
  name: string;
}

export interface NotesDatabase {
  object: "database";
  id: string;
  parent: NotesParent;
  title: string;
  title_rich_text: NotesRichText[];
  description: NotesRichText[];
  icon: NotesPageIcon | null;
  cover: NotesPageCover | null;
  in_trash: boolean;
  is_inline: boolean;
  data_sources: NotesDatabaseDataSourceSummary[];
  url: string | null;
  public_url: string | null;
  source_provider: string | null;
  source_object_id: string | null;
  source_workspace_id: string | null;
  source_last_edited_time: string | null;
  created_time: string;
  last_edited_time: string;
}

export interface NotesDataSourceParent {
  type: "database_id";
  database_id: string;
}

export interface NotesDataSource {
  object: "data_source";
  id: string;
  parent: NotesDataSourceParent;
  database_parent: NotesParent;
  title: string;
  title_rich_text: NotesRichText[];
  description: NotesRichText[];
  icon: NotesPageIcon | null;
  properties: Record<string, unknown>;
  in_trash: boolean;
  source_provider: string | null;
  source_object_id: string | null;
  source_workspace_id: string | null;
  source_last_edited_time: string | null;
  created_time: string;
  last_edited_time: string;
}

/** A data source in the vault-wide list, with where its database lives. */
export interface NotesDataSourceListItem extends NotesDataSource {
  /** Outermost first: project group and project names, folder names, then the titles of the pages (and row databases) leading to the page that holds the database. Page titles may be empty. */
  location: string[];
}

export const NOTES_DATA_SOURCE_PROPERTY_TYPES = [
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
  "files",
  "people",
  "created_time",
  "created_by",
  "last_edited_time",
  "last_edited_by",
  "unique_id",
  "place",
  "relation",
  "rollup",
  "formula",
  "button",
] as const;

export type NotesDataSourcePropertyType = (typeof NOTES_DATA_SOURCE_PROPERTY_TYPES)[number];

export const NOTES_DATA_SOURCE_ROLLUP_FUNCTIONS = [
  "average",
  "checked",
  "count",
  "count_values",
  "date_range",
  "earliest_date",
  "empty",
  "latest_date",
  "max",
  "median",
  "min",
  "not_empty",
  "percent_checked",
  "percent_empty",
  "percent_not_empty",
  "percent_unchecked",
  "range",
  "show_original",
  "show_unique",
  "sum",
  "unchecked",
  "unique",
] as const;

export type NotesDataSourceRollupFunction = (typeof NOTES_DATA_SOURCE_ROLLUP_FUNCTIONS)[number];

export const NOTES_DATA_SOURCE_SELECT_COLORS = [
  "default",
  "gray",
  "brown",
  "orange",
  "yellow",
  "green",
  "blue",
  "purple",
  "pink",
  "red",
] as const;

export type NotesDataSourceSelectColor = (typeof NOTES_DATA_SOURCE_SELECT_COLORS)[number];

export const NOTES_DATA_SOURCE_NUMBER_FORMATS = [
  "number",
  "number_with_commas",
  "percent",
  "dollar",
  "euro",
  "pound",
  "yen",
  "yuan",
  "won",
  "ruble",
  "rupee",
  "franc",
  "real",
  "lira",
  "krona",
  "ringgit",
] as const;

export type NotesDataSourceNumberFormat = (typeof NOTES_DATA_SOURCE_NUMBER_FORMATS)[number];

export const NOTES_DATA_SOURCE_STATUS_GROUPS = [
  "To-do",
  "In progress",
  "Complete",
] as const;

export type NotesDataSourceStatusGroup = (typeof NOTES_DATA_SOURCE_STATUS_GROUPS)[number];

export interface NotesDataSourceSchema {
  data_source: NotesDataSource;
  view: NotesDatabaseView;
}

export interface NotesDatabaseViewScope {
  databaseId?: string | null;
  viewId?: string | null;
}

export interface NotesDataSourceSchemaUpdate {
  properties: Record<string, unknown>;
}

export interface NotesDataSourceCreateRequest {
  id: string;
  database_id: string;
  view_id: string;
  title: string;
  view_name: string;
}

export interface NotesDataSourceAttachRequest {
  data_source_id: string;
  database_id: string;
  view_id: string;
  view_name: string;
}

export type NotesDataSourcePropertyAction =
  | { type: "insert"; property_id: string; side: "left" | "right"; property: Record<string, unknown> }
  | { type: "duplicate"; property_id: string; name: string }
  | { type: "change_type"; property_id: string; property_type: NotesDataSourcePropertyType; config?: Record<string, unknown> | null }
  | { type: "delete"; property_id: string };

export interface NotesDataSourcePropertyActionResult {
  property_id: string;
  schema: NotesDataSourceSchema;
}

export interface NotesDataSourceRowPageCreateRequest {
  id: string;
  title: string;
  first_block_id: string;
  properties?: Record<string, unknown> | null;
}

export interface NotesDataSourceTemplateCreateFromRowRequest {
  id: string;
  source_page_id: string;
  name: string;
  is_default?: boolean | null;
}

export interface NotesDataSourceTemplateApplyRequest {
  /** Reserve the resulting page identity while template body block IDs are generated. */
  id?: string;
  title?: string | null;
}

export interface NotesDataSourceButtonClickRequest {
  property_id: string;
  confirmed?: boolean | null;
}

export interface NotesDataSourceRowPropertyUpdate {
  property_id: string;
  value: unknown;
}

export interface NotesDataSourceViewWindowRequest {
  start_cursor?: string | null;
  page_size?: number | null;
  range_start?: string | null;
  range_end?: string | null;
}

export const NOTES_DATABASE_VIEW_TYPES = [
  "table",
  "board",
  "list",
  "calendar",
  "timeline",
  "gallery",
  "form",
  "chart",
  "map",
  "dashboard",
] as const;

export type NotesDatabaseViewType = (typeof NOTES_DATABASE_VIEW_TYPES)[number];

export interface NotesDatabaseViewParent {
  type: "database_id";
  database_id: string;
}

export interface NotesDatabaseView {
  object: "view";
  id: string;
  parent: NotesDatabaseViewParent;
  data_source_id: string;
  name: string;
  type: NotesDatabaseViewType;
  filter: Record<string, unknown> | null;
  sorts: Record<string, unknown>[];
  configuration: Record<string, unknown> | null;
  url: string | null;
  source_provider: string | null;
  source_object_id: string | null;
  source_workspace_id: string | null;
  source_last_edited_time: string | null;
  created_time: string;
  last_edited_time: string;
}

export interface NotesDatabaseCreateRequest {
  id: string;
  data_source_id: string;
  view_id: string;
  title: string;
  parent?: NotesParent | null;
  after_block_id?: string | null;
  replace_block_id?: string | null;
  icon?: NotesPageIcon | null;
  cover?: NotesPageCover | null;
}

export interface NotesDatabaseViewDuplicateRequest {
  id: string;
  database_id: string;
  source_view_id: string;
  name: string;
}

export const NOTES_DATABASE_VIEW_KINDS = [
  "table",
  "board",
  "gallery",
  "list",
  "calendar",
  "timeline",
] as const;

export type NotesDatabaseViewKind = (typeof NOTES_DATABASE_VIEW_KINDS)[number];
