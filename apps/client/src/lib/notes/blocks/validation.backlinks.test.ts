import { describe, expect, it } from "vitest";
import { parseNotesBacklink } from "./validation";

const page = {
  object: "page",
  id: "11111111-1111-4111-8111-111111111111",
  created_time: "2026-06-30T00:00:00.000Z",
  last_edited_time: "2026-06-30T00:00:00.000Z",
  parent: { type: "workspace", workspace: true },
  folder_id: null,
  in_trash: false,
  archived: false,
  icon: null,
  cover: null,
  properties: { title: { id: "title", type: "title", title: [] } },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

describe("notes backlinks", () => {
  it("parses backlink DTOs from the Tauri boundary", () => {
    expect(
      parseNotesBacklink({
        object: "backlink",
        id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:link",
        source_page: page,
        source_block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        source_block_type: "paragraph",
        reference_type: "link",
        snippet: "See target",
        created_time: "2026-06-30T00:00:00.000Z",
        last_edited_time: "2026-06-30T00:00:00.000Z",
      }).reference_type,
    ).toBe("link");
  });

  it("parses database relation backlinks", () => {
    expect(
      parseNotesBacklink({
        object: "backlink",
        id: "relation:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        source_page: page,
        source_block_id: "11111111-1111-4111-8111-111111111111",
        source_block_type: "database_relation",
        reference_type: "database_relation",
        snippet: "Project",
        created_time: "2026-06-30T00:00:00.000Z",
        last_edited_time: "2026-06-30T00:00:00.000Z",
      }).reference_type,
    ).toBe("database_relation");
  });

  it("parses comment backlinks", () => {
    expect(
      parseNotesBacklink({
        object: "backlink",
        id: "comment:10101010-1010-4010-8010-101010101010:page_mention",
        source_page: page,
        source_block_id: "11111111-1111-4111-8111-111111111111",
        source_block_type: "comment",
        reference_type: "comment_mention",
        snippet: "Mentioned from a comment",
        created_time: "2026-06-30T00:00:00.000Z",
        last_edited_time: "2026-06-30T00:00:00.000Z",
      }).reference_type,
    ).toBe("comment_mention");
  });

  it("rejects unsupported backlink reference types", () => {
    expect(() =>
      parseNotesBacklink({
        object: "backlink",
        id: "bad",
        source_page: page,
        source_block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        source_block_type: "paragraph",
        reference_type: "external_tool",
        snippet: "See target",
        created_time: "2026-06-30T00:00:00.000Z",
        last_edited_time: "2026-06-30T00:00:00.000Z",
      }),
    ).toThrow(
      "backlink.reference_type must be child_page, page_mention, link, database_relation, comment_mention, or comment_link",
    );
  });
});
