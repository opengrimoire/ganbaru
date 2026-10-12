import { describe, expect, it } from "vitest";
import { parseNotesPageAlias, parseNotesSearchResult, parseNotesUnresolvedLink } from "./validation";
import { basePage } from "./validation.fixtures";

describe("notes knowledge boundary validation", () => {
  it("parses search result DTOs", () => {
      const result = parseNotesSearchResult({
        object: "search_result",
        id: "block:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        type: "block",
        page: { ...basePage, icon: null },
        block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        block_type: "paragraph",
        comment_id: null,
        discussion_id: null,
        comment_status: null,
        comment_author: null,
        comment_anchor: null,
        snippet: "Target block",
        last_edited_time: "2026-06-30T12:00:00.000Z",
      });

      expect(result.type).toBe("block");
      expect(result.block_type).toBe("paragraph");
    });

  it("parses page alias and unresolved link DTOs", () => {
      const alias = parseNotesPageAlias({
        object: "page_alias",
        id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        page_id: basePage.id,
        alias: "Legacy Target",
        normalized_alias: "legacy target",
        created_time: "2026-06-30T12:00:00.000Z",
        last_edited_time: "2026-06-30T12:00:00.000Z",
      });
      const unresolvedLink = parseNotesUnresolvedLink({
        object: "unresolved_link",
        id: "unresolved:block:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:abc",
        source_type: "block",
        source_page_id: basePage.id,
        source_block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        source_comment_id: null,
        raw_url: "http://localhost:1420/?view=notes#notes?alias=Legacy%20Target",
        raw_target: "Legacy Target",
        normalized_target: "legacy target",
        link_text: "Legacy Target",
        snippet: "Legacy Target",
        created_time: "2026-06-30T12:00:00.000Z",
        last_edited_time: "2026-06-30T12:00:00.000Z",
      });

      expect(alias.alias).toBe("Legacy Target");
      expect(unresolvedLink.source_type).toBe("block");
      expect(unresolvedLink.raw_target).toBe("Legacy Target");
    });

  it("rejects unsupported unresolved link source types", () => {
      expect(() =>
        parseNotesUnresolvedLink({
          object: "unresolved_link",
          id: "bad",
          source_type: "page",
          source_page_id: basePage.id,
          source_block_id: null,
          source_comment_id: null,
          raw_url: "http://localhost:1420/?view=notes#notes?alias=Legacy%20Target",
          raw_target: "Legacy Target",
          normalized_target: "legacy target",
          link_text: "",
          snippet: "",
          created_time: "2026-06-30T12:00:00.000Z",
          last_edited_time: "2026-06-30T12:00:00.000Z",
        }),
      ).toThrow("unresolved_link.source_type must be block or comment");
    });

  it("parses comment search metadata", () => {
      const result = parseNotesSearchResult({
        object: "search_result",
        id: "comment:10101010-1010-4010-8010-101010101010",
        type: "comment",
        page: { ...basePage, icon: null },
        block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        block_type: null,
        comment_id: "10101010-1010-4010-8010-101010101010",
        discussion_id: "90909090-9090-4090-8090-909090909090",
        comment_status: "resolved",
        comment_author: { type: "user", resolved_name: "Reviewer" },
        comment_anchor: {
          object: "comment_anchor",
          type: "text_range",
          block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          start: 6,
          end: 10,
          text: "beta",
          prefix: "Alpha ",
          suffix: " gamma",
          created_time: "2026-06-30T12:00:00.000Z",
          last_edited_time: "2026-06-30T12:00:00.000Z",
        },
        snippet: "Reviewer beta comment",
        last_edited_time: "2026-06-30T12:00:00.000Z",
      });

      expect(result.comment_status).toBe("resolved");
      expect(result.comment_author?.resolved_name).toBe("Reviewer");
      expect(result.comment_anchor?.text).toBe("beta");
    });

  it("parses property-backed page search result DTOs", () => {
      const result = parseNotesSearchResult({
        object: "search_result",
        id: "page:11111111-1111-4111-8111-111111111111",
        type: "page",
        page: { ...basePage, icon: null },
        block_id: null,
        block_type: null,
        comment_id: null,
        discussion_id: null,
        comment_status: null,
        comment_author: null,
        comment_anchor: null,
        snippet: "Formula state Ready Searchable",
        last_edited_time: "2026-06-30T12:00:00.000Z",
      });

      expect(result.type).toBe("page");
      expect(result.block_id).toBeNull();
      expect(result.snippet).toBe("Formula state Ready Searchable");
    });

  it("rejects unsupported search result types", () => {
      expect(() =>
        parseNotesSearchResult({
          object: "search_result",
          id: "bad",
          type: "database",
          page: { ...basePage, icon: null },
          block_id: null,
          block_type: null,
          comment_id: null,
          discussion_id: null,
          comment_status: null,
          comment_author: null,
          comment_anchor: null,
          snippet: "",
          last_edited_time: "2026-06-30T12:00:00.000Z",
        }),
      ).toThrow("search_result.type must be page, block, or comment");
    });
});
