import { describe, expect, it } from "vitest";
import { parseNotesBlock, parseNotesFolder, parseNotesHtmlExportResult, parseNotesLocalUser, parseNotesPage } from "./validation";
import { baseBlock, basePage, baseRichText } from "./validation.fixtures";
import { applyBlockUpdate, blockIndent, blockUpdateFromBlock, blockUpdateWithIndent, blockWithCodeLanguage, blockWithMedia, createBlockWrite } from "./factory";
import { NOTES_BLOCK_TYPES } from "$lib/notes/types";

describe("notes core-block boundary validation", () => {
  it.each(NOTES_BLOCK_TYPES)("retains %s layout through payload writes, parsing, and history", (type) => {
    const original = parseNotesBlock({ ...baseBlock, ...createBlockWrite(baseBlock.id, type, "") });
    const indented = applyBlockUpdate(original, blockUpdateWithIndent(blockUpdateFromBlock(original), 3));
    const parsed = parseNotesBlock(JSON.parse(JSON.stringify(indented)));
    expect(blockIndent(parsed)).toBe(3);
    expect(blockUpdateFromBlock(parsed)).toEqual(blockUpdateFromBlock(indented));
    expect(blockIndent(parseNotesBlock(applyBlockUpdate(parsed, blockUpdateWithIndent(blockUpdateFromBlock(parsed), 0))))).toBe(0);
    const record: Record<string, unknown> = { ...indented };
    const payload = record[type];
    if (typeof payload !== "object" || payload === null) throw new Error("Expected block payload");
    for (const invalid of [-1, 0.5, "1", null, Number.MAX_SAFE_INTEGER + 1]) {
      expect(() => parseNotesBlock({ ...indented, [type]: { ...payload, ganbaru_indent: invalid } })).toThrow();
    }
  });

  it("keeps embedded content at its level when its media or code settings change", () => {
    const image = parseNotesBlock({ ...baseBlock, ...createBlockWrite(baseBlock.id, "image", "https://example.com/image.png") });
    const indented = applyBlockUpdate(image, blockUpdateWithIndent(blockUpdateFromBlock(image), 2));
    expect(blockIndent(applyBlockUpdate(indented, blockWithMedia(indented, "https://example.com/new.png", "Caption")))).toBe(2);
    const code = parseNotesBlock({ ...baseBlock, ...createBlockWrite(baseBlock.id, "code", "value") });
    const indentedCode = applyBlockUpdate(code, blockUpdateWithIndent(blockUpdateFromBlock(code), 2));
    expect(blockIndent(applyBlockUpdate(indentedCode, blockWithCodeLanguage(indentedCode, "javascript")))).toBe(2);
  });

  it("parses folder DTOs and page folder membership", () => {
      const folder = parseNotesFolder({
        object: "folder",
        id: "folder-a",
        project_id: "project-a",
        parent_folder_id: null,
        name: "Research",
        created_time: "2026-07-10T12:00:00.000Z",
        last_edited_time: "2026-07-10T12:00:00.000Z",
      });
      const page = parseNotesPage({ ...basePage, folder_id: folder.id });

      expect(folder).toMatchObject({
        object: "folder",
        id: "folder-a",
        parent_folder_id: null,
        name: "Research",
      });
      expect(page.folder_id).toBe("folder-a");
    });

  it("rejects malformed folder DTOs", () => {
      expect(() => parseNotesFolder({
        object: "folder",
        id: "folder-a",
        project_id: "project-a",
        parent_folder_id: null,
        name: " ",
        created_time: "2026-07-10T12:00:00.000Z",
        last_edited_time: "2026-07-10T12:00:00.000Z",
      })).toThrow("folder.name must not be empty");
    });

  it("parses local Notes user DTOs", () => {
      const user = parseNotesLocalUser({
        object: "user",
        id: "70707070-7070-4070-8070-707070707070",
        display_name: "Alice",
        created_time: "2026-07-01T12:00:00.000Z",
        last_edited_time: "2026-07-01T12:01:00.000Z",
      });

      expect(user.display_name).toBe("Alice");
    });

  it("parses toggleable heading payload state", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "heading_2",
        heading_2: {
          rich_text: [baseRichText],
          color: "default",
          is_toggleable: true,
          ganbaru_open: false,
        },
      });

      expect(block).toMatchObject({
        type: "heading_2",
        heading_2: {
          is_toggleable: true,
          ganbaru_open: false,
        },
      });
    });

  it("parses heading 4 payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "heading_4",
        heading_4: {
          rich_text: [baseRichText],
          color: "blue",
          is_toggleable: true,
          ganbaru_open: true,
        },
      });

      expect(block.type).toBe("heading_4");
      if (block.type === "heading_4") {
        expect(block.heading_4.color).toBe("blue");
        expect(block.heading_4.is_toggleable).toBe(true);
        expect(block.heading_4.ganbaru_open).toBe(true);
      }
    });

  it("rejects invalid toggleable heading flags", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "heading_1",
          heading_1: {
            rich_text: [baseRichText],
            color: "default",
            is_toggleable: "yes",
          },
        }),
      ).toThrow("block.heading_1.is_toggleable must be a boolean");
    });

  it("parses tab block payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "tab",
        tab: {},
      });

      expect(block.type).toBe("tab");
      if (block.type === "tab") {
        expect(block.tab).toEqual({});
      }
    });

  it("rejects tab block payload properties", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "tab",
          tab: { title: "Bad" },
        }),
      ).toThrow("block.tab must be an empty object");
    });

  it("parses original synced block payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "synced_block",
        synced_block: {
          synced_from: null,
        },
      });

      expect(block.type).toBe("synced_block");
      if (block.type === "synced_block") {
        expect(block.synced_block.synced_from).toBeNull();
      }
    });

  it("parses duplicate synced block references", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "synced_block",
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

  it("rejects invalid synced block references", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "synced_block",
          synced_block: {
            synced_from: {
              type: "page_id",
              block_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            },
          },
        }),
      ).toThrow("block.synced_block.synced_from.type must be block_id");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "synced_block",
          synced_block: {
            synced_from: {
              type: "block_id",
              block_id: "bad",
            },
          },
        }),
      ).toThrow("block.synced_block.synced_from.block_id must be a UUID");
    });

  it("parses button block payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "button",
        button: {
          rich_text: [baseRichText],
          icon: { type: "icon", icon: { name: "mouse-pointer-click", color: "gray" } },
          actions: [{ type: "insert_blocks", source: "children", position: "below_button" }],
        },
      });

      expect(block.type).toBe("button");
      if (block.type === "button") {
        expect(block.button.rich_text[0]?.plain_text).toBe("Heading");
        expect(block.button.actions[0]?.position).toBe("below_button");
      }
    });

  it("rejects invalid button action payloads", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "button",
          button: {
            rich_text: [baseRichText],
            icon: null,
            actions: [],
          },
        }),
      ).toThrow("block.button.actions must include between 1 and 10 actions");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "button",
          button: {
            rich_text: [baseRichText],
            icon: null,
            actions: [{ type: "send_webhook", source: "children", position: "below_button" }],
          },
        }),
      ).toThrow("block.button.actions[0].type must be insert_blocks");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "button",
          button: {
            rich_text: [baseRichText],
            icon: null,
            children: [],
            actions: [{ type: "insert_blocks", source: "children", position: "below_button" }],
          },
        }),
      ).toThrow("block.button.children must be stored as child blocks");
    });

  it("parses HTML archive export result DTOs", () => {
      const result = parseNotesHtmlExportResult({
        object: "notes_html_archive_export",
        root_page_id: "11111111-1111-4111-8111-111111111111",
        files: [
          {
            path: "index.html",
            content_type: "text/html; charset=utf-8",
            contents: "<!doctype html>",
            byte_size: 15,
          },
        ],
        assets: [
          {
            id: "notes/files/local.txt",
            archive_path: "assets/notes/files/local.txt",
            source_path: "notes/files/local.txt",
            content_type: "text/plain",
            byte_size: 12,
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            storage_state: "available",
            exported: true,
          },
        ],
        diagnostics: [
          {
            code: "html_export_local_asset_not_included",
            severity: "warning",
            page_id: "11111111-1111-4111-8111-111111111111",
            block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            asset_id: null,
            comment_id: null,
            message: "Local managed asset was not included in this archive",
          },
        ],
        manifest_json: "{\"object\":\"notes_html_archive_manifest\"}",
        exported_page_count: 1,
        exported_block_count: 2,
        exported_asset_count: 1,
        exported_comment_count: 0,
        exported_database_view_count: 0,
      });

      expect(result.object).toBe("notes_html_archive_export");
      expect(result.files[0]?.path).toBe("index.html");
      expect(result.assets[0]?.exported).toBe(true);
      expect(result.diagnostics[0]?.block_id).toBe("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
      expect(result.exported_asset_count).toBe(1);
    });
});
