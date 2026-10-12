import type { NotesPage } from "../core";
import type { NotesDataSource, NotesDatabaseView } from "./base";
import type { NotesDatabaseRowHierarchy } from "$lib/notes/database/row-hierarchy";

export type NotesDatabaseTableRowOpenMode = "full_page" | "side_panel";

export type NotesDatabaseTableFilterCondition =
  | "contains"
  | "equals"
  | "not_equals"
  | "greater_than"
  | "greater_than_or_equal"
  | "less_than"
  | "less_than_or_equal"
  | "before"
  | "on_or_before"
  | "after"
  | "on_or_after"
  | "is_empty"
  | "is_not_empty"
  | "checked"
  | "unchecked";

export type NotesDatabaseTableSortDirection = "ascending" | "descending";

export interface NotesDatabaseTableFilterPredicate {
  property_id: string;
  condition: NotesDatabaseTableFilterCondition;
  value?: string | number | boolean | null;
}

export interface NotesDatabaseTableFilterGroup {
  type: "and" | "or";
  filters: NotesDatabaseTableFilter[];
}

export type NotesDatabaseTableFilter = NotesDatabaseTableFilterPredicate | NotesDatabaseTableFilterGroup;

export interface NotesDatabaseTableSort {
  property_id: string;
  direction: NotesDatabaseTableSortDirection;
}

export interface NotesDatabaseTableConfiguration {
  property_order: string[];
  hidden_property_ids: string[];
  column_widths: Record<string, number>;
  row_open_mode: NotesDatabaseTableRowOpenMode;
  group_property_id?: string | null;
  group_order?: string[];
  collapsed_group_ids?: string[];
  collapsed_row_ids?: string[];
  hide_empty_groups?: boolean;
  presentation?: NotesDatabaseTablePresentation;
}

export type NotesDatabaseTableCalculation = "count_all" | "count_values" | "empty" | "unique" | "sum" | "average" | "min" | "max" | "percent_checked";

export interface NotesDatabaseTableColumnPresentation {
  wrap: boolean;
  date_format: "locale" | "iso" | "relative";
  time_format: "locale" | "12_hour" | "24_hour" | "hidden";
  calculation: NotesDatabaseTableCalculation | null;
}

export interface NotesDatabaseTablePresentation {
  frozen_property_id: string | null;
  columns: Record<string, NotesDatabaseTableColumnPresentation>;
  color_rules?: NotesDatabaseTableColorRule[];
}

export interface NotesDatabaseTableColorRule {
  id: string;
  property_id: string | null;
  color: "gray" | "brown" | "orange" | "yellow" | "green" | "blue" | "purple" | "pink" | "red";
  filters: NotesDatabaseTableFilter[];
}

export interface NotesDataSourceTableViewUpdate {
  filter: NotesDatabaseTableFilter[];
  sorts: NotesDatabaseTableSort[];
  configuration: NotesDatabaseTableConfiguration;
}

export interface NotesDataSourceTableView {
  data_source: NotesDataSource;
  view: NotesDatabaseView;
  rows: NotesPage[];
  total_row_count: number;
  next_cursor: string | null;
  has_more: boolean;
  group_counts?: Record<string, number>;
  calculations?: { overall: Record<string, number | null>; groups: Record<string, Record<string, number | null>> };
  row_hierarchy?: NotesDatabaseRowHierarchy;
}
