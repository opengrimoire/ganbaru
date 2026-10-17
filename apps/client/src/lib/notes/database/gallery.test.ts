import { describe, expect, it } from "vitest";
import {
  notesDatabaseGalleryCardCover,
  notesDatabaseGalleryCardText,
  notesDatabaseGalleryCardTitle,
  notesDatabaseGalleryColumns,
  notesDatabaseGalleryConfigurationFromView,
  notesDatabaseGalleryCoverColumns,
  notesDatabaseGalleryFiltersFromView,
  notesDatabaseGallerySortsFromView,
  notesDatabaseGalleryUpdate,
  notesDatabaseGalleryVisibleColumns,
} from "./gallery";
import type { NotesDataSource, NotesDatabaseView, NotesPage } from "$lib/notes/types";

const dataSource: NotesDataSource = {
  object: "data_source",
  id: "11111111-1111-4111-8111-111111111111",
  parent: {
    type: "database_id",
    database_id: "22222222-2222-4222-8222-222222222222",
  },
  database_parent: {
    type: "page_id",
    page_id: "33333333-3333-4333-8333-333333333333",
  },
  title: "Media tasks",
  title_rich_text: [],
  description: [],
  icon: null,
  properties: {
    Name: {
      id: "title",
      name: "Name",
      description: "",
      type: "title",
      title: {},
    },
    Cover: {
      id: "cover_files",
      name: "Cover",
      description: "",
      type: "files",
      files: {},
    },
    Status: {
      id: "status",
      name: "Status",
      description: "",
      type: "status",
      status: {
        options: [
          { id: "todo", name: "To-do", color: "gray", group: "To-do" },
          { id: "doing", name: "Doing", color: "blue", group: "In progress" },
        ],
      },
    },
    Estimate: {
      id: "estimate",
      name: "Estimate",
      description: "",
      type: "number",
      number: { format: "number" },
    },
  },
  in_trash: false,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T00:00:00.000Z",
};

const view: NotesDatabaseView = {
  object: "view",
  id: "44444444-4444-4444-8444-444444444444",
  parent: {
    type: "database_id",
    database_id: "22222222-2222-4222-8222-222222222222",
  },
  data_source_id: dataSource.id,
  name: "Gallery",
  type: "gallery",
  filter: {
    type: "and",
    filters: [{ property_id: "status", condition: "equals", value: "Doing" }],
  },
  sorts: [{ property_id: "estimate", direction: "descending" }],
  configuration: {
    type: "gallery",
    gallery: {
      cover_source: "files_property",
      cover_property_id: "cover_files",
      visible_property_ids: ["estimate", "status"],
      card_size: "large",
      fit_image: true,
      row_open_mode: "side_panel",
    },
  },
  url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T00:00:00.000Z",
};

const page: NotesPage = {
  object: "page",
  id: "55555555-5555-4555-8555-555555555555",
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T01:00:00.000Z",
  parent: {
    type: "data_source_id",
    data_source_id: dataSource.id,
  },
  folder_id: null,
  in_trash: false,
  archived: false,
  icon: null,
  cover: { type: "external", external: { url: "https://example.com/page-cover.jpg" } },
  properties: {
    Name: {
      id: "title",
      type: "title",
      title: [
        {
          type: "text",
          text: { content: "Publish gallery", link: null },
          annotations: {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default",
          },
          plain_text: "Publish gallery",
          href: null,
        },
      ],
    },
    Cover: {
      id: "cover_files",
      type: "files",
      files: [{ type: "external", external: { url: "https://example.com/file-cover.png" } }],
    },
    Status: {
      id: "status",
      type: "status",
      status: { id: "doing", name: "Doing", color: "blue" },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 5,
    },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

describe("database gallery helpers", () => {
  it("reads gallery configuration, cover columns, and visible card properties", () => {
    const configuration = notesDatabaseGalleryConfigurationFromView(view);
    const columns = notesDatabaseGalleryColumns(dataSource, view);
    const coverColumns = notesDatabaseGalleryCoverColumns(columns);
    const visibleColumns = notesDatabaseGalleryVisibleColumns(columns, configuration);

    expect(configuration.cover_source).toBe("files_property");
    expect(configuration.cover_property_id).toBe("cover_files");
    expect(configuration.card_size).toBe("large");
    expect(configuration.fit_image).toBe(true);
    expect(configuration.row_open_mode).toBe("side_panel");
    expect(coverColumns.map((column) => column.id)).toEqual(["cover_files"]);
    expect(visibleColumns.map((column) => column.id)).toEqual(["estimate", "status"]);
  });

  it("serializes card configuration, visible properties, filters, and sorts", () => {
    const configuration = notesDatabaseGalleryConfigurationFromView(view);
    const columns = notesDatabaseGalleryColumns(dataSource, view);
    const update = notesDatabaseGalleryUpdate(
      { ...configuration, cover_source: "page_cover", row_open_mode: "full_page" },
      columns.filter((column) => column.id === "status"),
      notesDatabaseGalleryFiltersFromView(view),
      notesDatabaseGallerySortsFromView(view),
    );

    expect(update.configuration.cover_source).toBe("page_cover");
    expect(update.configuration.cover_property_id).toBeNull();
    expect(update.configuration.visible_property_ids).toEqual(["status"]);
    expect(update.configuration.card_size).toBe("large");
    expect(update.configuration.fit_image).toBe(true);
    expect(update.configuration.row_open_mode).toBe("full_page");
    expect(update.filter).toEqual([{ property_id: "status", condition: "equals", value: "Doing" }]);
    expect(update.sorts).toEqual([{ property_id: "estimate", direction: "descending" }]);
  });

  it("chooses card titles, property text, and preview covers", () => {
    const configuration = notesDatabaseGalleryConfigurationFromView(view);
    const columns = notesDatabaseGalleryColumns(dataSource, view);
    const statusColumn = columns.find((column) => column.id === "status");
    const fileCover = notesDatabaseGalleryCardCover(page, configuration);
    const pageCover = notesDatabaseGalleryCardCover(page, {
      ...configuration,
      cover_source: "page_cover",
      cover_property_id: null,
    });
    const noCover = notesDatabaseGalleryCardCover(page, {
      ...configuration,
      cover_source: "none",
      cover_property_id: null,
    });

    expect(notesDatabaseGalleryCardTitle(page, columns, "Untitled")).toBe("Publish gallery");
    expect(statusColumn && notesDatabaseGalleryCardText(page, statusColumn)).toBe("Doing");
    expect(fileCover).toEqual({
      type: "external",
      external: { url: "https://example.com/file-cover.png" },
    });
    expect(pageCover).toEqual({
      type: "external",
      external: { url: "https://example.com/page-cover.jpg" },
    });
    expect(noCover).toBeNull();
  });
});
