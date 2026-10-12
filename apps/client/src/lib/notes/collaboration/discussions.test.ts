import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesComment, NotesCommentAnchor, NotesCommentThread } from "$lib/notes/types";
import {
  filterNotesDiscussions,
  groupNotesDiscussionsByDay,
  notesDiscussionLastActivity,
  notesDiscussionQuote,
  notesDiscussionQuoteText,
  notesPageDiscussionsInReadingOrder,
} from "./discussions";

const pageId = "11111111-1111-4111-8111-111111111111";
const blockId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

function comment(id: string, created: string, edited = created): NotesComment {
  return {
    object: "comment",
    id,
    parent: { type: "page_id", page_id: pageId },
    discussion_id: id,
    created_time: created,
    last_edited_time: edited,
    created_by: { object: "user", id: "local-user" },
    rich_text: [createRichText(`Comment ${id}`)],
    attachments: [],
    display_name: { type: "user", resolved_name: "You" },
    deleted_at: null,
    reactions: [],
  };
}

function thread(
  id: string,
  options: {
    created?: string;
    comments?: NotesComment[];
    status?: "open" | "resolved";
    resolvedAt?: string | null;
    block?: boolean;
    anchor?: NotesCommentAnchor | null;
  } = {},
): NotesCommentThread {
  const created = options.created ?? "2026-10-10T12:00:00.000Z";
  return {
    object: "comment_thread",
    id,
    parent: options.block ? { type: "block_id", block_id: blockId } : { type: "page_id", page_id: pageId },
    page_id: pageId,
    block_id: options.block ? blockId : null,
    status: options.status ?? "open",
    resolved_at: options.resolvedAt ?? null,
    resolved_by: null,
    anchor: options.anchor ?? null,
    created_time: created,
    last_edited_time: created,
    unread: false,
    muted: false,
    comments: options.comments ?? [comment(`${id}-c1`, created)],
  };
}

describe("notes discussions", () => {
  it("measures activity from posts, edits, and resolution", () => {
    const edited = thread("edited", {
      comments: [
        comment("a", "2026-10-09T08:00:00.000Z", "2026-10-11T07:30:00.000Z"),
        comment("b", "2026-10-10T08:00:00.000Z"),
      ],
    });
    const resolved = thread("resolved", { status: "resolved", resolvedAt: "2026-10-11T09:00:00.000Z" });

    expect(notesDiscussionLastActivity(edited)).toBe(Date.parse("2026-10-11T07:30:00.000Z"));
    expect(notesDiscussionLastActivity(resolved)).toBe(Date.parse("2026-10-11T09:00:00.000Z"));
  });

  it("filters by status, drops empty threads, and sorts by latest activity", () => {
    const older = thread("older", { created: "2026-10-01T10:00:00.000Z" });
    const newer = thread("newer", { created: "2026-10-05T10:00:00.000Z" });
    const done = thread("done", { status: "resolved", created: "2026-10-08T10:00:00.000Z" });
    const empty = thread("empty", { comments: [] });

    expect(filterNotesDiscussions([older, done, newer, empty], "open").map((item) => item.id)).toEqual(["newer", "older"]);
    expect(filterNotesDiscussions([older, done, newer, empty], "resolved").map((item) => item.id)).toEqual(["done"]);
    expect(filterNotesDiscussions([older, done, newer, empty], "all").map((item) => item.id)).toEqual(["done", "newer", "older"]);
  });

  it("groups by local calendar day across a UTC midnight", () => {
    const today = Temporal.PlainDate.from("2026-10-11");
    const lateLocal = thread("late", { created: "2026-10-11T03:30:00.000Z" });
    const morning = thread("morning", { created: "2026-10-11T15:00:00.000Z" });
    const lastWeek = thread("last-week", { created: "2026-10-04T15:00:00.000Z" });
    const ordered = filterNotesDiscussions([lateLocal, morning, lastWeek], "all");

    const groups = groupNotesDiscussionsByDay(ordered, { today, timeZone: "America/Mexico_City" });

    expect(groups.map((group) => [group.day, group.relative, group.threads.map((item) => item.id)])).toEqual([
      ["2026-10-11", "today", ["morning"]],
      ["2026-10-10", "yesterday", ["late"]],
      ["2026-10-04", null, ["last-week"]],
    ]);
  });

  it("skips threads without a readable timestamp instead of inventing a day", () => {
    const broken = thread("broken", { created: "not a date", comments: [comment("x", "also not a date")] });

    expect(groupNotesDiscussionsByDay([broken], { today: Temporal.PlainDate.from("2026-10-11"), timeZone: "UTC" })).toEqual([]);
  });

  it("lists open page discussions oldest first for the inline section", () => {
    const second = thread("second", { created: "2026-10-10T12:00:00.000Z" });
    const first = thread("first", { created: "2026-10-09T12:00:00.000Z" });
    const onBlock = thread("block", { block: true });
    const resolved = thread("resolved", { status: "resolved" });

    expect(notesPageDiscussionsInReadingOrder([second, onBlock, resolved, first]).map((item) => item.id)).toEqual(["first", "second"]);
  });

  it("quotes the commented selection, then the block, and never a page discussion", () => {
    const anchor: NotesCommentAnchor = {
      object: "comment_anchor",
      type: "text_range",
      block_id: blockId,
      start: 0,
      end: 9,
      text: "Ship  the\nbeta",
      prefix: "",
      suffix: "",
      created_time: "2026-10-10T12:00:00.000Z",
      last_edited_time: "2026-10-10T12:00:00.000Z",
    };

    expect(notesDiscussionQuote(thread("anchored", { block: true, anchor }), "Ship the beta this week")).toBe("Ship the beta");
    expect(notesDiscussionQuote(thread("block", { block: true }), "  Whole block text ")).toBe("Whole block text");
    expect(notesDiscussionQuote(thread("block", { block: true }), null)).toBeNull();
    expect(notesDiscussionQuote(thread("page"), "Ignored")).toBeNull();
    expect(notesDiscussionQuote(thread("long", { block: true }), "x".repeat(400))?.length).toBe(280);
  });

  it("quotes a pending selection the same way and drops text with nothing readable", () => {
    expect(notesDiscussionQuoteText(" draft\n\tselection ")).toBe("draft selection");
    expect(notesDiscussionQuoteText(" \n\t ")).toBeNull();
    expect(notesDiscussionQuoteText("y".repeat(281))?.endsWith("…")).toBe(true);
  });
});
