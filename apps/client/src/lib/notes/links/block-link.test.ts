import { describe, expect, it } from "vitest";
import {
  buildNotesBlockLink,
  buildNotesDatabaseViewLink,
  buildNotesPageLink,
  isNotesUuid,
  notesBlockAnchorId,
  parseNotesBlockLinkHash,
  parseNotesLinkHash,
} from "./block-link";

const pageId = "11111111-1111-4111-8111-111111111111";
const blockId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const viewId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

describe("notes block links", () => {
  it("builds local notes links with page and block anchors", () => {
    expect(
      buildNotesBlockLink("http://localhost:1420/?view=calendar", { pageId, blockId }),
    ).toBe(
      "http://localhost:1420/?view=notes#notes?page=11111111-1111-4111-8111-111111111111&block=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    );
  });

  it("builds local notes page links", () => {
    expect(buildNotesPageLink("http://localhost:1420/?view=calendar", { pageId })).toBe(
      "http://localhost:1420/?view=notes#notes?page=11111111-1111-4111-8111-111111111111",
    );
  });

  it("removes detached window search state from copied links", () => {
    expect(
      buildNotesBlockLink("http://localhost:1420/?ganbaruWindow=notes&view=notes", {
        pageId,
        blockId,
      }),
    ).not.toContain("ganbaruWindow");
  });

  it("parses valid notes block hashes", () => {
    expect(
      parseNotesBlockLinkHash(
        "#notes?page=11111111-1111-4111-8111-111111111111&block=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      ),
    ).toEqual({ pageId, blockId });
  });

  it("parses valid notes page hashes", () => {
    expect(parseNotesLinkHash("#notes?page=11111111-1111-4111-8111-111111111111")).toEqual({
      pageId,
    });
  });

  it("rejects malformed notes block hashes", () => {
    expect(parseNotesBlockLinkHash("#notes?page=bad&block=bad")).toBeNull();
    expect(parseNotesBlockLinkHash("#notes?page=11111111-1111-4111-8111-111111111111")).toBeNull();
    expect(parseNotesBlockLinkHash("#calendar")).toBeNull();
  });

  it("builds and parses database view links", () => {
    const link = buildNotesDatabaseViewLink("http://localhost:1420/?view=calendar", { pageId, blockId, viewId });
    expect(link).toBe(`http://localhost:1420/?view=notes#notes?page=${pageId}&block=${blockId}&view=${viewId}`);
    expect(parseNotesLinkHash(new URL(link).hash)).toEqual({ pageId, blockId, viewId });
    expect(parseNotesBlockLinkHash(new URL(link).hash)).toMatchObject({ pageId, blockId });
  });

  it("rejects database view links without a block or with a malformed view", () => {
    expect(() => buildNotesDatabaseViewLink("http://localhost:1420/", { pageId, blockId, viewId: "view-a" })).toThrow();
    expect(parseNotesLinkHash(`#notes?page=${pageId}&view=${viewId}`)).toBeNull();
    expect(parseNotesLinkHash(`#notes?page=${pageId}&block=${blockId}&view=view-a`)).toBeNull();
  });

  it("validates UUID shaped ids", () => {
    expect(isNotesUuid(pageId)).toBe(true);
    expect(isNotesUuid("page-a")).toBe(false);
  });

  it("creates DOM-safe anchor ids", () => {
    expect(notesBlockAnchorId("block/a b")).toBe("notes-block-block_a_b");
  });
});
