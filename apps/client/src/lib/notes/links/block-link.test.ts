import { describe, expect, it } from "vitest";
import {
  buildNotesBlockLink,
  buildNotesDatabaseViewLink,
  buildNotesDiscussionLink,
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

  it("removes window search state from copied links", () => {
    expect(
      buildNotesBlockLink("http://localhost:1420/?ganbaruWindow=notes&view=notes&tab=%7B%7D", {
        pageId,
        blockId,
      }),
    ).toBe("http://localhost:1420/?view=notes#notes?page=11111111-1111-4111-8111-111111111111&block=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
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

  it("builds and parses discussion links with backend thread ids", () => {
    const discussionId = "0123abcd-4567-0123-c0de-0123456789ab";
    const link = buildNotesDiscussionLink("http://localhost:1420/?view=calendar", { pageId, discussionId });
    expect(link).toBe(`http://localhost:1420/?view=notes#notes?page=${pageId}&discussion=${discussionId}`);
    expect(parseNotesLinkHash(new URL(link).hash)).toEqual({ pageId, discussionId });
    expect(parseNotesBlockLinkHash(new URL(link).hash)).toBeNull();
  });

  it("rejects malformed discussion links and discussions combined with a block", () => {
    expect(() => buildNotesDiscussionLink("http://localhost:1420/", { pageId, discussionId: "thread-a" })).toThrow();
    expect(parseNotesLinkHash(`#notes?page=${pageId}&discussion=thread-a`)).toBeNull();
    expect(parseNotesLinkHash(`#notes?page=${pageId}&block=${blockId}&discussion=${viewId}`)).toBeNull();
  });

  it("validates UUID shaped ids", () => {
    expect(isNotesUuid(pageId)).toBe(true);
    expect(isNotesUuid("page-a")).toBe(false);
  });

  it("creates DOM-safe anchor ids", () => {
    expect(notesBlockAnchorId("block/a b")).toBe("notes-block-block_a_b");
  });
});
