import { describe, expect, it } from "vitest";
import { parseNotesCommentThread } from "$lib/notes/blocks/validation";
import {
  latestNotesComment,
  notesCommentAnchorDraft,
  notesCommentBodyRichText,
  notesCommentIsEmpty,
  notesCommentAnchorsForBlock,
  notesCommentParentKey,
  notesCommentPlainText,
  notesResolveCommentAnchor,
  notesCommentThreadSnippet,
  openNotesCommentThreadCount,
  openNotesPageCommentThreads,
  unreadNotesCommentThreadCount,
} from "./comments";
import { createRichText } from "$lib/notes/blocks/factory";

const comment = {
  object: "comment",
  id: "10101010-1010-4010-8010-101010101010",
  parent: { type: "block_id", block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" },
  discussion_id: "90909090-9090-4090-8090-909090909090",
  created_time: "2026-06-30T00:00:00.000Z",
  last_edited_time: "2026-06-30T00:00:00.000Z",
  created_by: { object: "user", id: "local-user" },
  rich_text: [createRichText("Review this")],
  attachments: [],
  display_name: { type: "user", resolved_name: "You" },
  deleted_at: null,
  reactions: [],
};

describe("notes comments", () => {
  it("parses comment thread DTOs from the Tauri boundary", () => {
    const thread = parseNotesCommentThread({
      object: "comment_thread",
      id: "90909090-9090-4090-8090-909090909090",
      parent: { type: "block_id", block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" },
      page_id: "11111111-1111-4111-8111-111111111111",
      block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      status: "open",
      resolved_at: null,
      resolved_by: null,
      anchor: null,
      created_time: "2026-06-30T00:00:00.000Z",
      last_edited_time: "2026-06-30T00:00:00.000Z",
      unread: false,
      muted: false,
      comments: [comment],
    });

    expect(thread.block_id).toBe("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    expect(notesCommentPlainText(thread.comments[0])).toBe("Review this");
  });

  it("rejects workspace parents for comments", () => {
    expect(() =>
      parseNotesCommentThread({
        object: "comment_thread",
        id: "90909090-9090-4090-8090-909090909090",
        parent: { type: "workspace", workspace: true },
        page_id: "11111111-1111-4111-8111-111111111111",
        block_id: null,
        status: "open",
        resolved_at: null,
        resolved_by: null,
        anchor: null,
        created_time: "2026-06-30T00:00:00.000Z",
        last_edited_time: "2026-06-30T00:00:00.000Z",
        unread: false,
        muted: false,
        comments: [],
      }),
    ).toThrow("comment thread.parent.type must be page_id or block_id");
  });

  it("summarizes open threads and parent keys", () => {
    const openThread = parseNotesCommentThread({
      object: "comment_thread",
      id: "90909090-9090-4090-8090-909090909090",
      parent: { type: "block_id", block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" },
      page_id: "11111111-1111-4111-8111-111111111111",
      block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      status: "open",
      resolved_at: null,
      resolved_by: null,
      anchor: null,
      created_time: "2026-06-30T00:00:00.000Z",
      last_edited_time: "2026-06-30T00:00:00.000Z",
      unread: true,
      muted: false,
      comments: [comment],
    });
    const resolvedThread = { ...openThread, id: "80808080-8080-4080-8080-808080808080", status: "resolved" as const };

    expect(openNotesCommentThreadCount([openThread, resolvedThread])).toBe(1);
    expect(unreadNotesCommentThreadCount([openThread, resolvedThread])).toBe(2);
    expect(notesCommentThreadSnippet(openThread)).toBe("Review this");
    expect(notesCommentParentKey(openThread.parent)).toBe("block:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
  });

  it("parses and resolves inline text anchors", () => {
    const thread = parseNotesCommentThread({
      object: "comment_thread",
      id: "90909090-9090-4090-8090-909090909090",
      parent: { type: "block_id", block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" },
      page_id: "11111111-1111-4111-8111-111111111111",
      block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      status: "open",
      resolved_at: null,
      resolved_by: null,
      anchor: {
        object: "comment_anchor",
        type: "text_range",
        block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        start: 6,
        end: 10,
        text: "beta",
        prefix: "Alpha ",
        suffix: " gamma",
        created_time: "2026-06-30T00:00:00.000Z",
        last_edited_time: "2026-06-30T00:00:00.000Z",
      },
      created_time: "2026-06-30T00:00:00.000Z",
      last_edited_time: "2026-06-30T00:00:00.000Z",
      unread: false,
      muted: false,
      comments: [comment],
    });

    expect(notesResolveCommentAnchor(thread, "Alpha beta gamma")).toMatchObject({
      start: 6,
      end: 10,
      text: "beta",
    });
    expect(notesResolveCommentAnchor(thread, "Intro Alpha beta gamma")).toMatchObject({
      start: 12,
      end: 16,
    });
    expect(notesCommentAnchorsForBlock(
      [thread],
      "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      "Intro Alpha beta gamma",
    )).toHaveLength(1);
    expect(notesResolveCommentAnchor(thread, "Alpha gamma")).toBeNull();
  });

  it("creates inline anchor drafts from selections", () => {
    expect(notesCommentAnchorDraft("Alpha beta gamma", 6, 10)).toEqual({
      start: 6,
      end: 10,
      text: "beta",
      prefix: "Alpha ",
      suffix: " gamma",
    });
    expect(notesCommentAnchorDraft("Alpha beta gamma", 0, 0)).toBeNull();
    expect(notesCommentAnchorDraft("Alpha beta gamma", 5, 6)).toBeNull();
  });
});

describe("notes comment bodies and discussion lists", () => {
  const pageId = "11111111-1111-4111-8111-111111111111";

  function thread(
    id: string,
    parent: { type: "page_id"; page_id: string } | { type: "block_id"; block_id: string },
    status: "open" | "resolved",
    createdTimes: string[],
  ) {
    return parseNotesCommentThread({
      object: "comment_thread",
      id,
      parent,
      page_id: pageId,
      block_id: parent.type === "block_id" ? parent.block_id : null,
      status,
      resolved_at: status === "resolved" ? "2026-07-01T00:00:00.000Z" : null,
      resolved_by: null,
      anchor: null,
      created_time: createdTimes[0],
      last_edited_time: createdTimes[0],
      unread: false,
      muted: false,
      comments: createdTimes.map((created_time, index) => ({
        ...comment,
        id: `${id.slice(0, -2)}${String(index).padStart(2, "0")}`,
        parent,
        discussion_id: id,
        created_time,
        last_edited_time: created_time,
      })),
    });
  }

  it("trims edge whitespace across rich text items and keeps inner spacing", () => {
    const body = notesCommentBodyRichText([createRichText("  Ship "), createRichText(" it  ")]);
    expect(body.map((item) => item.plain_text).join("")).toBe("Ship  it");
    expect(notesCommentBodyRichText([createRichText(" \n ")])).toEqual([]);
  });

  it("treats a comment as empty only without text and attachments", () => {
    expect(notesCommentIsEmpty([createRichText("  ")], [])).toBe(true);
    expect(notesCommentIsEmpty([], [{ category: "file" }])).toBe(false);
    expect(notesCommentIsEmpty([createRichText("Hi")], [])).toBe(false);
  });

  it("lists open page discussions and finds the latest comment across threads", () => {
    const pageThread = thread(
      "90909090-9090-4090-8090-909090909001",
      { type: "page_id", page_id: pageId },
      "open",
      ["2026-07-01T09:00:00.000Z", "2026-07-03T09:00:00.000Z"],
    );
    const resolvedPageThread = thread(
      "90909090-9090-4090-8090-909090909002",
      { type: "page_id", page_id: pageId },
      "resolved",
      ["2026-07-02T09:00:00.000Z"],
    );
    const blockThread = thread(
      "90909090-9090-4090-8090-909090909003",
      { type: "block_id", block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" },
      "open",
      ["2026-07-04T09:00:00.000Z"],
    );

    expect(openNotesPageCommentThreads([pageThread, resolvedPageThread, blockThread]))
      .toEqual([pageThread]);
    expect(latestNotesComment([pageThread, resolvedPageThread])).toEqual({
      thread: pageThread,
      comment: pageThread.comments[1],
    });
    expect(latestNotesComment([pageThread, blockThread])?.thread).toBe(blockThread);
    expect(latestNotesComment([])).toBeNull();
  });
});
