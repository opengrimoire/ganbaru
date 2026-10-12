import type { NotesDataSourceGalleryView } from "$lib/notes/contracts/database";
import { isNotesGalleryCoverSource } from ".././assets";
import { isNotesGalleryCardSize, isNotesGalleryRowOpenMode } from ".././blocks";
import { readRecord, readString, readStringArray } from ".././readers";
import { parseNotesPage } from ".././workspace";
import { parseDataSourceWindowMetadata, parseNotesDataSource, parseNotesDatabaseView } from "./base";
import { validateNotesTableFilter, validateNotesTableSorts } from "./table";

function validateNotesGalleryConfiguration(value: Record<string, unknown> | null): void {
  if (value === null) return;
  if (value.type !== undefined && value.type !== "gallery") {
    throw new Error("database gallery configuration.type must be gallery");
  }
  const gallery = readRecord(value.gallery, "database gallery configuration.gallery");
  if (!isNotesGalleryCoverSource(gallery.cover_source ?? "page_cover")) {
    throw new Error("database gallery configuration.cover_source must be supported");
  }
  if (gallery.cover_property_id !== null && gallery.cover_property_id !== undefined) {
    readString(gallery.cover_property_id, "database gallery configuration.cover_property_id");
  }
  readStringArray(
    gallery.visible_property_ids,
    "database gallery configuration.visible_property_ids",
  );
  if (!isNotesGalleryCardSize(gallery.card_size ?? "medium")) {
    throw new Error("database gallery configuration.card_size must be supported");
  }
  if (typeof (gallery.fit_image ?? false) !== "boolean") {
    throw new Error("database gallery configuration.fit_image must be boolean");
  }
  if (!isNotesGalleryRowOpenMode(gallery.row_open_mode ?? "full_page")) {
    throw new Error("database gallery configuration.row_open_mode must be supported");
  }
}

export function parseNotesDataSourceGalleryView(value: unknown): NotesDataSourceGalleryView {
  const record = readRecord(value, "data source gallery view");
  const dataSource = parseNotesDataSource(record.data_source);
  const view = parseNotesDatabaseView(record.view);
  if (view.type !== "gallery") {
    throw new Error("data source gallery view.view.type must be gallery");
  }
  if (view.data_source_id !== dataSource.id) {
    throw new Error("data source gallery view ids must match");
  }
  validateNotesGalleryConfiguration(view.configuration);
  validateNotesTableFilter(view.filter);
  validateNotesTableSorts(view.sorts);
  if (!Array.isArray(record.rows)) {
    throw new Error("data source gallery view.rows must be an array");
  }
  return {
    data_source: dataSource,
    view,
    rows: record.rows.map(parseNotesPage),
    ...parseDataSourceWindowMetadata(record, "data source gallery view"),
  };
}
