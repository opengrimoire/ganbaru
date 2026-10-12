import { notesDatabaseSerializeFilters } from "./filters";
import {
  notesDatabaseTableCellText,
  notesDatabaseTableColumns,
  notesDatabaseTableFiltersFromView,
  notesDatabaseTableSortsFromView,
  type NotesDatabaseTableColumn,
} from "./table";
import type {
  NotesDatabaseGalleryCardSize,
  NotesDatabaseGalleryConfiguration,
  NotesDatabaseGalleryCoverSource,
  NotesDatabaseGalleryRowOpenMode,
  NotesDatabaseTableFilter,
  NotesDatabaseTableSort,
  NotesDatabaseView,
  NotesDataSource,
  NotesDataSourceGalleryViewUpdate,
  NotesFileObject,
  NotesPage,
  NotesPageCover,
} from "$lib/notes/types";

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function galleryConfig(view: NotesDatabaseView): UnknownRecord {
  const configuration = view.configuration;
  if (!isRecord(configuration)) return {};
  const gallery = configuration.gallery;
  return isRecord(gallery) ? gallery : {};
}

/** Read the persisted gallery configuration with local defaults. */
export function notesDatabaseGalleryConfigurationFromView(
  view: NotesDatabaseView,
): NotesDatabaseGalleryConfiguration {
  const gallery = galleryConfig(view);
  const coverSource: NotesDatabaseGalleryCoverSource =
    gallery.cover_source === "files_property" || gallery.cover_source === "none"
      ? gallery.cover_source
      : "page_cover";
  const cardSize: NotesDatabaseGalleryCardSize =
    gallery.card_size === "small" || gallery.card_size === "large" ? gallery.card_size : "medium";
  const rowOpenMode: NotesDatabaseGalleryRowOpenMode =
    gallery.row_open_mode === "side_panel" ? "side_panel" : "full_page";
  return {
    cover_source: coverSource,
    cover_property_id: typeof gallery.cover_property_id === "string" ? gallery.cover_property_id : null,
    visible_property_ids: readStringArray(gallery.visible_property_ids),
    card_size: cardSize,
    fit_image: gallery.fit_image === true,
    row_open_mode: rowOpenMode,
  };
}

/** Build gallery columns from the current data source schema. */
export function notesDatabaseGalleryColumns(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDatabaseTableColumn[] {
  return notesDatabaseTableColumns(dataSource, view);
}

/** Return files properties that can feed a gallery preview image. */
export function notesDatabaseGalleryCoverColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => column.type === "files");
}

/** Return the card properties displayed under the card title. */
export function notesDatabaseGalleryVisibleColumns(
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseGalleryConfiguration,
): NotesDatabaseTableColumn[] {
  const byId = new Map(columns.map((column) => [column.id, column]));
  const configured = configuration.visible_property_ids
    .map((id) => byId.get(id))
    .filter((column): column is NotesDatabaseTableColumn =>
      column !== undefined && column.type !== "title"
    );
  if (configured.length > 0) return configured;
  return columns.filter((column) => column.type !== "title").slice(0, 4);
}

/** Build the update payload expected by the local gallery Tauri command. */
export function notesDatabaseGalleryUpdate(
  configuration: NotesDatabaseGalleryConfiguration,
  visibleColumns: readonly NotesDatabaseTableColumn[],
  filters: readonly NotesDatabaseTableFilter[],
  sorts: readonly NotesDatabaseTableSort[],
): NotesDataSourceGalleryViewUpdate {
  return {
    filter: notesDatabaseSerializeFilters(filters),
    sorts: sorts.map((sort) => ({
      property_id: sort.property_id,
      direction: sort.direction,
    })),
    configuration: {
      cover_source: configuration.cover_source,
      cover_property_id: configuration.cover_source === "files_property"
        ? configuration.cover_property_id
        : null,
      visible_property_ids: visibleColumns.map((column) => column.id),
      card_size: configuration.card_size,
      fit_image: configuration.fit_image,
      row_open_mode: configuration.row_open_mode,
    },
  };
}

/** Return filters stored on a gallery view. */
export function notesDatabaseGalleryFiltersFromView(view: NotesDatabaseView): NotesDatabaseTableFilter[] {
  return notesDatabaseTableFiltersFromView(view);
}

/** Return sorts stored on a gallery view. */
export function notesDatabaseGallerySortsFromView(view: NotesDatabaseView): NotesDatabaseTableSort[] {
  return notesDatabaseTableSortsFromView(view);
}

/** Return user-facing text for a card property. */
export function notesDatabaseGalleryCardText(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): string {
  return notesDatabaseTableCellText(page, column);
}

/** Return the title displayed on a gallery card. */
export function notesDatabaseGalleryCardTitle(
  page: NotesPage,
  columns: readonly NotesDatabaseTableColumn[],
  fallback: string,
): string {
  const titleColumn = columns.find((column) => column.type === "title");
  const title = titleColumn ? notesDatabaseTableCellText(page, titleColumn).trim() : "";
  return title || fallback;
}

/** Return the Notion-style cover object that should render in the gallery card preview. */
export function notesDatabaseGalleryCardCover(
  page: NotesPage,
  configuration: NotesDatabaseGalleryConfiguration,
): NotesPageCover | null {
  if (configuration.cover_source === "none") return null;
  if (configuration.cover_source === "page_cover") return page.cover;
  if (!configuration.cover_property_id) return null;
  return firstFilesPropertyCover(page, configuration.cover_property_id);
}

function firstFilesPropertyCover(page: NotesPage, propertyId: string): NotesPageCover | null {
  const property = Object.values(page.properties).find((value) =>
    isRecord(value) && value.id === propertyId && value.type === "files"
  );
  if (!isRecord(property) || !Array.isArray(property.files)) return null;
  const firstFile = property.files.find(isNotesFileObject);
  return firstFile ?? null;
}

function isNotesFileObject(value: unknown): value is NotesFileObject {
  if (!isRecord(value)) return false;
  if (value.type === "external") {
    const external = value.external;
    return isRecord(external) && typeof external.url === "string";
  }
  if (value.type === "file") {
    const file = value.file;
    return isRecord(file) && typeof file.url === "string";
  }
  if (value.type === "file_upload") {
    const fileUpload = value.file_upload;
    return isRecord(fileUpload) && typeof fileUpload.id === "string";
  }
  return false;
}
