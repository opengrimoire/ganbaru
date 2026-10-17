import { describe, expect, it } from "vitest";
import { createBlockWrite, createRichText } from "$lib/notes/blocks/factory";
import {
  mapNotesBlockDto,
  mapNotesBlockListDto,
  mapNotesCreatedDatabaseDto,
  mapNotesFolderDto,
  mapNotesLoadedPageDto,
  mapNotesPageDto,
} from "./response-mappers";

const now = "2026-06-30T09:00:00.000Z";

function pageDto() {
  return {
    object: "page",
    id: "page-a",
    created_time: now,
    last_edited_time: now,
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      title: {
        id: "title",
        type: "title",
        title: [createRichText("Inbox")],
      },
    },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function blockDto() {
  const write = createBlockWrite("block-a", "paragraph", "First");
  return {
    object: "block",
    id: write.id,
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: write.type,
    paragraph: {
      rich_text: [createRichText("First"), createRichText(" second")],
      color: "default",
    },
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

describe("Notion notes DTO mappers", () => {
  it("maps folder DTOs", () => {
    expect(mapNotesFolderDto({
      object: "folder",
      id: "folder-a",
      project_id: "project-a",
      parent_folder_id: null,
      name: "Research",
      created_time: now,
      last_edited_time: now,
    })).toMatchObject({
      object: "folder",
      id: "folder-a",
      name: "Research",
    });
  });

  it("maps page DTOs with public parent and title properties", () => {
    const page = mapNotesPageDto(pageDto());

    expect(page.object).toBe("page");
    expect(page.parent).toEqual({ type: "workspace", workspace: true });
    expect(page.properties.title).toBeDefined();
  });

  it("maps archived page DTOs separately from trash", () => {
    const page = mapNotesPageDto({
      ...pageDto(),
      archived: true,
    });

    expect(page.archived).toBe(true);
    expect(page.in_trash).toBe(false);
  });

  it("maps block DTOs without flattening rich text arrays", () => {
    const block = mapNotesBlockDto(blockDto());

    expect(block.type).toBe("paragraph");
    if (block.type === "paragraph") {
      expect(block.paragraph.rich_text).toHaveLength(2);
      expect(block.paragraph.rich_text.map((item) => item.plain_text).join("")).toBe("First second");
    }
  });

  it("maps toggle blocks with local open state", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "toggle",
      paragraph: undefined,
      toggle: {
        rich_text: [createRichText("Details")],
        color: "default",
        ganbaru_open: false,
      },
    });

    expect(block.type).toBe("toggle");
    if (block.type === "toggle") {
      expect(block.toggle.ganbaru_open).toBe(false);
      expect(block.toggle.rich_text[0]?.plain_text).toBe("Details");
    }
  });

  it("maps callout blocks with icon and color", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "callout",
      paragraph: undefined,
      callout: {
        rich_text: [createRichText("Remember")],
        color: "yellow_background",
        icon: {
          type: "icon",
          icon: {
            name: "info",
            color: "gray",
          },
        },
      },
    });

    expect(block.type).toBe("callout");
    if (block.type === "callout") {
      expect(block.callout.color).toBe("yellow_background");
      expect(block.callout.icon).toEqual({ type: "icon", icon: { name: "info", color: "gray" } });
      expect(block.callout.rich_text[0]?.plain_text).toBe("Remember");
    }
  });

  it("maps breadcrumb blocks with empty payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "breadcrumb",
      paragraph: undefined,
      breadcrumb: {},
    });

    expect(block.type).toBe("breadcrumb");
    if (block.type === "breadcrumb") {
      expect(block.breadcrumb).toEqual({});
    }
  });

  it("maps table of contents blocks with color payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "table_of_contents",
      paragraph: undefined,
      table_of_contents: {
        color: "blue_background",
      },
    });

    expect(block.type).toBe("table_of_contents");
    if (block.type === "table_of_contents") {
      expect(block.table_of_contents.color).toBe("blue_background");
    }
  });

  it("maps child page blocks with title payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      id: "page-child",
      type: "child_page",
      paragraph: undefined,
      child_page: {
        title: "Nested page",
      },
    });

    expect(block.type).toBe("child_page");
    if (block.type === "child_page") {
      expect(block.child_page.title).toBe("Nested page");
    }
  });

  it("maps child database blocks with title payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      type: "child_database",
      paragraph: undefined,
      child_database: {
        title: "Tasks",
        database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        data_source_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        view_id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
      },
    });

    expect(block.type).toBe("child_database");
    if (block.type === "child_database") {
      expect(block.child_database.title).toBe("Tasks");
      expect(block.child_database.database_id).toBe("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    }
  });

  it("maps local database creation DTOs", () => {
    const created = mapNotesCreatedDatabaseDto({
      database: {
        object: "database",
        id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        parent: { type: "page_id", page_id: "page-a" },
        title: "Tasks",
        title_rich_text: [createRichText("Tasks")],
        description: [],
        icon: null,
        cover: null,
        in_trash: false,
        is_inline: true,
        data_sources: [{ id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc", name: "Tasks" }],
        url: null,
        public_url: null,
        source_provider: null,
        source_object_id: null,
        source_workspace_id: null,
        source_last_edited_time: null,
        created_time: now,
        last_edited_time: now,
      },
      data_source: {
        object: "data_source",
        id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        parent: {
          type: "database_id",
          database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        },
        database_parent: { type: "page_id", page_id: "page-a" },
        title: "Tasks",
        title_rich_text: [createRichText("Tasks")],
        description: [],
        icon: null,
        properties: {
          Name: { id: "title", name: "Name", type: "title", title: {} },
        },
        in_trash: false,
        source_provider: null,
        source_object_id: null,
        source_workspace_id: null,
        source_last_edited_time: null,
        created_time: now,
        last_edited_time: now,
      },
      view: {
        object: "view",
        id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        parent: {
          type: "database_id",
          database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        },
        data_source_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        name: "Table",
        type: "table",
        filter: null,
        sorts: [],
        configuration: { type: "table" },
        url: null,
        source_provider: null,
        source_object_id: null,
        source_workspace_id: null,
        source_last_edited_time: null,
        created_time: now,
        last_edited_time: now,
      },
      block: {
        ...blockDto(),
        id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        type: "child_database",
        paragraph: undefined,
        child_database: {
          title: "Tasks",
          database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          data_source_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          view_id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        },
      },
    });

    expect(created.database.object).toBe("database");
    expect(created.data_source.parent.database_id).toBe(created.database.id);
    expect(created.block.type).toBe("child_database");
  });

  it("maps column list and column blocks with Notion-shaped layout payloads", () => {
    const columnList = mapNotesBlockDto({
      ...blockDto(),
      type: "column_list",
      paragraph: undefined,
      column_list: {},
    });
    const column = mapNotesBlockDto({
      ...blockDto(),
      id: "column-a",
      parent: { type: "block_id", block_id: "column-list-a" },
      type: "column",
      paragraph: undefined,
      column: {
        width_ratio: 0.5,
      },
    });

    expect(columnList.type).toBe("column_list");
    if (columnList.type === "column_list") {
      expect(columnList.column_list).toEqual({});
    }
    expect(column.type).toBe("column");
    if (column.type === "column") {
      expect(column.column.width_ratio).toBe(0.5);
    }
  });

  it("maps table blocks with table row cell payloads", () => {
    const table = mapNotesBlockDto({
      ...blockDto(),
      type: "table",
      paragraph: undefined,
      table: {
        table_width: 2,
        has_column_header: true,
        has_row_header: false,
      },
    });
    const row = mapNotesBlockDto({
      ...blockDto(),
      id: "row-a",
      parent: { type: "block_id", block_id: "table-a" },
      type: "table_row",
      paragraph: undefined,
      table_row: {
        cells: [[createRichText("Name")], [createRichText("Status")]],
      },
    });

    expect(table.type).toBe("table");
    if (table.type === "table") {
      expect(table.table.table_width).toBe(2);
      expect(table.table.has_column_header).toBe(true);
    }
    expect(row.type).toBe("table_row");
    if (row.type === "table_row") {
      expect(row.table_row.cells[1]?.[0]?.plain_text).toBe("Status");
    }
  });

  it("maps media and file blocks with file object payloads", () => {
    const image = mapNotesBlockDto({
      ...blockDto(),
      type: "image",
      paragraph: undefined,
      image: {
        type: "external",
        external: {
          url: "https://example.com/image.png",
        },
        caption: [createRichText("Cover")],
      },
    });
    const file = mapNotesBlockDto({
      ...blockDto(),
      id: "file-a",
      type: "file",
      paragraph: undefined,
      file: {
        type: "external",
        external: {
          url: "https://example.com/doc.txt",
        },
        caption: [],
        name: "doc.txt",
      },
    });

    expect(image.type).toBe("image");
    if (image.type === "image") {
      expect(image.image.type).toBe("external");
      if (image.image.type === "external") {
        expect(image.image.external.url).toBe("https://example.com/image.png");
      }
      expect(image.image.caption?.[0]?.plain_text).toBe("Cover");
    }
    expect(file.type).toBe("file");
    if (file.type === "file") {
      expect(file.file.name).toBe("doc.txt");
      expect(file.file.type).toBe("external");
      if (file.file.type === "external") {
        expect(file.file.external.url).toBe("https://example.com/doc.txt");
      }
    }
  });

  it("maps bookmark blocks with URL and caption payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "bookmark",
      paragraph: undefined,
      bookmark: {
        caption: [createRichText("Reference")],
        url: "https://example.com",
      },
    });

    expect(block.type).toBe("bookmark");
    if (block.type === "bookmark") {
      expect(block.bookmark.url).toBe("https://example.com");
      expect(block.bookmark.caption[0]?.plain_text).toBe("Reference");
    }
  });

  it("maps embed blocks with URL payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "embed",
      paragraph: undefined,
      embed: {
        url: "https://player.vimeo.com/video/226053498",
      },
    });

    expect(block.type).toBe("embed");
    if (block.type === "embed") {
      expect(block.embed.url).toBe("https://player.vimeo.com/video/226053498");
    }
  });

  it("maps link preview blocks with URL payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "link_preview",
      paragraph: undefined,
      link_preview: {
        url: "https://github.com/example/repo/pull/123",
      },
    });

    expect(block.type).toBe("link_preview");
    if (block.type === "link_preview") {
      expect(block.link_preview.url).toBe("https://github.com/example/repo/pull/123");
    }
  });

  it("maps original synced blocks", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "synced_block",
      paragraph: undefined,
      synced_block: {
        synced_from: null,
      },
    });

    expect(block.type).toBe("synced_block");
    if (block.type === "synced_block") {
      expect(block.synced_block.synced_from).toBeNull();
    }
  });

  it("maps duplicate synced block references", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "synced_block",
      paragraph: undefined,
      synced_block: {
        synced_from: {
          type: "block_id",
          block_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        },
      },
    });

    expect(block.type).toBe("synced_block");
    if (block.type === "synced_block") {
      expect(block.synced_block.synced_from?.block_id).toBe(
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      );
    }
  });

  it("maps equation blocks with expression payloads", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "equation",
      paragraph: undefined,
      equation: {
        expression: "e=mc^2",
      },
    });

    expect(block.type).toBe("equation");
    if (block.type === "equation") {
      expect(block.equation.expression).toBe("e=mc^2");
    }
  });

  it("maps unsupported blocks while preserving imported metadata", () => {
    const block = mapNotesBlockDto({
      ...blockDto(),
      type: "unsupported",
      paragraph: undefined,
      unsupported: {
        block_type: "form",
        source_type: "notion",
        raw: {
          type: "unsupported",
          unsupported: {
            block_type: "form",
          },
        },
        warnings: ["Content is not exposed by the source API"],
      },
    });

    expect(block.type).toBe("unsupported");
    if (block.type === "unsupported") {
      expect(block.unsupported.block_type).toBe("form");
      expect(block.unsupported.source_type).toBe("notion");
      expect(block.unsupported.raw).toEqual({
        type: "unsupported",
        unsupported: {
          block_type: "form",
        },
      });
      expect(block.unsupported.warnings).toEqual(["Content is not exposed by the source API"]);
    }
  });

  it("maps paginated block list DTOs", () => {
    const list = mapNotesBlockListDto({
      object: "list",
      type: "block",
      block: {},
      results: [blockDto()],
      next_cursor: "block-a",
      has_more: true,
    });

    expect(list.results).toHaveLength(1);
    expect(list.next_cursor).toBe("block-a");
    expect(list.has_more).toBe(true);
  });

  it("maps loaded page DTOs", () => {
    const loaded = mapNotesLoadedPageDto({
      page: pageDto(),
      blocks: {
        object: "list",
        type: "block",
        block: {},
        results: [blockDto()],
        next_cursor: null,
        has_more: false,
      },
    });

    expect(loaded.page.id).toBe("page-a");
    expect(loaded.blocks.results[0]?.id).toBe("block-a");
  });

  it("rejects unknown block payloads before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        paragraph: { rich_text: [{}] },
      })
    ).toThrow("block.paragraph.rich_text[0].type must be a string");
  });

  it("rejects unsupported Notion color values before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        paragraph: {
          rich_text: [createRichText("First")],
          color: "neon",
        },
      })
    ).toThrow("block.paragraph.color must be a supported Notion color");
  });

  it("rejects invalid callout icons before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "callout",
        paragraph: undefined,
        callout: {
          rich_text: [createRichText("Remember")],
          color: "default",
          icon: {
            type: "icon",
            icon: {
              name: "",
            },
          },
        },
      })
    ).toThrow("block.callout.icon.icon.name must not be empty");
  });

  it("rejects invalid table of contents colors before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "table_of_contents",
        paragraph: undefined,
        table_of_contents: {
          color: "neon",
        },
      })
    ).toThrow("block.table_of_contents.color must be a supported Notion color");
  });

  it("rejects invalid column width ratios before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "column",
        paragraph: undefined,
        column: {
          width_ratio: 0,
        },
      })
    ).toThrow("block.column.width_ratio must be greater than 0 and no more than 1");
  });

  it("rejects invalid media URLs before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "image",
        paragraph: undefined,
        image: {
          type: "external",
          external: {
            url: "https://example.com/image.txt",
          },
          caption: [],
        },
      })
    ).toThrow("block.image.external.url must be a supported HTTPS image URL");
  });

  it("rejects invalid child page titles before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "child_page",
        paragraph: undefined,
        child_page: {
          title: 42,
        },
      })
    ).toThrow("block.child_page.title must be a string");
  });

  it("rejects invalid child database titles before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "child_database",
        paragraph: undefined,
        child_database: {
          title: "Tasks\u0008",
        },
      })
    ).toThrow("block.child_database.title must not contain control characters");
  });

  it("rejects invalid table payloads before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "table",
        paragraph: undefined,
        table: {
          table_width: "2",
          has_column_header: false,
          has_row_header: false,
        },
      })
    ).toThrow("block.table.table_width must be an integer");
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "table_row",
        paragraph: undefined,
        table_row: {
          cells: [{}],
        },
      })
    ).toThrow("block.table_row.cells[0] must be an array");
  });

  it("rejects invalid bookmark captions before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "bookmark",
        paragraph: undefined,
        bookmark: {
          caption: [{}],
          url: "https://example.com",
        },
      })
    ).toThrow("block.bookmark.caption[0].type must be a string");
  });

  it("rejects invalid embed URLs before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "embed",
        paragraph: undefined,
        embed: {
          url: 42,
        },
      })
    ).toThrow("block.embed.url must be a string");
  });

  it("rejects invalid link preview URLs before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "link_preview",
        paragraph: undefined,
        link_preview: {
          url: 42,
        },
      })
    ).toThrow("block.link_preview.url must be a string");
  });

  it("rejects invalid synced block references before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "synced_block",
        paragraph: undefined,
        synced_block: {
          synced_from: {
            type: "block_id",
            block_id: "bad",
          },
        },
      })
    ).toThrow("block.synced_block.synced_from.block_id must be a UUID");
  });

  it("rejects invalid equation expressions before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "equation",
        paragraph: undefined,
        equation: {
          expression: 42,
        },
      })
    ).toThrow("block.equation.expression must be a string");
  });

  it("rejects invalid unsupported block metadata before typed use", () => {
    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "unsupported",
        paragraph: undefined,
        unsupported: {
          block_type: "bad\u0008type",
        },
      })
    ).toThrow("block.unsupported.block_type must not contain control characters");

    expect(() =>
      mapNotesBlockDto({
        ...blockDto(),
        type: "unsupported",
        paragraph: undefined,
        unsupported: {
          block_type: "form",
          raw: "form",
        },
      })
    ).toThrow("block.unsupported.raw must be an object");
  });
});
