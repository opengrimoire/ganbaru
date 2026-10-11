import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NotesCommentCreate,
  NotesCommentParent,
  NotesCommentThread,
  NotesCommentThreadReadUpdate,
  NotesCommentUpdate,
  NotesPageNotificationLevel,
} from "$lib/notes/types";
import { createTextRichText } from "$lib/notes/rich-text/core";
import { createNotesCollaborationController } from "./collaboration.svelte";

const backend = vi.hoisted(() => ({
  threads: [] as NotesCommentThread[],
  calls: [] as Array<{ pageId: string; includeResolved: boolean }>,
  readCalls: [] as Array<{ includeResolved: boolean; ids: string[] }>,
  fullReadGate: null as Promise<void> | null,
  created: [] as NotesCommentCreate[],
  updated: [] as Array<{ commentId: string; update: NotesCommentUpdate }>,
  reactionCalls: [] as Array<{ commentId: string; emoji: string }>,
  notificationLevel: "all" as NotesPageNotificationLevel,
  failNotificationWrite: false,
}));

function threadContaining(commentId: string): NotesCommentThread {
  const thread = backend.threads.find((candidate) =>
    candidate.comments.some((comment) => comment.id === commentId));
  if (!thread) throw new Error("Missing comment fixture");
  return thread;
}

function replaceThread(updated: NotesCommentThread): NotesCommentThread {
  backend.threads = backend.threads.some((candidate) => candidate.id === updated.id)
    ? backend.threads.map((candidate) => candidate.id === updated.id ? updated : candidate)
    : [...backend.threads, updated];
  return updated;
}

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  return {
    ...actual,
    listNotesComments: async (pageId: string, includeResolved: boolean) => {
      backend.calls.push({ pageId, includeResolved });
      const matching = backend.threads.filter((thread) =>
        thread.page_id === pageId && (includeResolved || thread.status === "open"));
      if (includeResolved && backend.fullReadGate) await backend.fullReadGate;
      return matching;
    },
    markNotesCommentThreadsRead: async (request: NotesCommentThreadReadUpdate) => {
      backend.readCalls.push({
        includeResolved: request.include_resolved ?? false,
        ids: [...request.discussion_ids],
      });
      backend.threads = backend.threads.map((thread) =>
        request.discussion_ids.includes(thread.id) ? { ...thread, unread: false } : thread);
      return backend.threads.filter((thread) =>
        thread.page_id === request.page_id
        && (request.include_resolved || thread.status === "open"));
    },
    resolveNotesCommentThread: async (id: string, resolved: boolean) => {
      const thread = backend.threads.find((candidate) => candidate.id === id);
      if (!thread) throw new Error("Missing comment thread fixture");
      const updated: NotesCommentThread = {
        ...thread,
        status: resolved ? "resolved" : "open",
        resolved_at: resolved ? "2026-01-02T00:00:00.000Z" : null,
      };
      backend.threads = backend.threads.map((candidate) => candidate.id === id ? updated : candidate);
      return updated;
    },
    createNotesComment: async (request: NotesCommentCreate) => {
      backend.created.push(request);
      const existing = request.discussion_id
        ? backend.threads.find((candidate) => candidate.id === request.discussion_id)
        : undefined;
      const parent: NotesCommentParent = request.parent ?? existing?.parent
        ?? { type: "page_id", page_id: "page-a" };
      const thread = existing ?? commentThread(`thread-${request.id}`, "page-a", "open", parent);
      const comment = {
        ...commentThread("unused", "page-a", "open", parent).comments[0],
        id: request.id,
        discussion_id: thread.id,
        rich_text: request.rich_text,
        attachments: request.attachments ?? [],
      };
      return replaceThread({
        ...thread,
        comments: existing ? [...thread.comments, comment] : [comment],
      });
    },
    updateNotesComment: async (commentId: string, update: NotesCommentUpdate) => {
      backend.updated.push({ commentId, update });
      const thread = threadContaining(commentId);
      return replaceThread({
        ...thread,
        comments: thread.comments.map((comment) => comment.id === commentId
          ? { ...comment, rich_text: update.rich_text, attachments: update.attachments ?? [] }
          : comment),
      });
    },
    toggleNotesCommentReaction: async (commentId: string, emoji: string) => {
      backend.reactionCalls.push({ commentId, emoji });
      const thread = threadContaining(commentId);
      return replaceThread({
        ...thread,
        comments: thread.comments.map((comment) => comment.id === commentId
          ? { ...comment, reactions: [{ emoji, count: 1, reacted_by_me: true, names: ["Local user"] }] }
          : comment),
      });
    },
    setNotesCommentThreadMuted: async (discussionId: string, muted: boolean) => {
      const thread = backend.threads.find((candidate) => candidate.id === discussionId);
      if (!thread) throw new Error("Missing comment thread fixture");
      return replaceThread({ ...thread, muted });
    },
    getNotesPageNotificationSettings: async (pageId: string) => ({
      object: "page_notification_settings" as const,
      page_id: pageId,
      level: backend.notificationLevel,
    }),
    setNotesPageNotificationLevel: async (pageId: string, level: NotesPageNotificationLevel) => {
      if (backend.failNotificationWrite) throw new Error("write refused");
      backend.notificationLevel = level;
      return { object: "page_notification_settings" as const, page_id: pageId, level };
    },
  };
});

function commentThread(
  id: string,
  pageId: string,
  status: "open" | "resolved",
  parent: NotesCommentParent = { type: "page_id", page_id: pageId },
): NotesCommentThread {
  return {
    object: "comment_thread",
    id,
    parent,
    page_id: pageId,
    block_id: parent.type === "block_id" ? parent.block_id : null,
    status,
    resolved_at: status === "resolved" ? "2026-01-02T00:00:00.000Z" : null,
    resolved_by: null,
    anchor: null,
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    unread: false,
    muted: false,
    comments: [{
      object: "comment",
      id: `${id}-comment`,
      parent,
      discussion_id: id,
      created_time: "2026-01-01T00:00:00.000Z",
      last_edited_time: "2026-01-01T00:00:00.000Z",
      created_by: { object: "user", id: "local-user" },
      rich_text: [],
      attachments: [],
      display_name: { type: "user", resolved_name: "Local user" },
      deleted_at: null,
      reactions: [],
    }],
  };
}

function controllerForPage(readPageId: () => string) {
  return createNotesCollaborationController({
    readSelectedPageId: readPageId,
    readBlocksById: () => ({}),
    flushBlockSave: async () => undefined,
    requestBlockFocus: () => undefined,
    updateBlockRichText: async () => undefined,
    isPanelOpen: () => true,
  });
}

beforeEach(() => {
  backend.threads = [
    commentThread("open-a", "page-a", "open"),
    commentThread("resolved-a", "page-a", "resolved"),
    commentThread("open-b", "page-b", "open"),
    commentThread("resolved-b", "page-b", "resolved"),
  ];
  backend.calls = [];
  backend.readCalls = [];
  backend.fullReadGate = null;
  backend.created = [];
  backend.updated = [];
  backend.reactionCalls = [];
  backend.notificationLevel = "all";
  backend.failNotificationWrite = false;
});

describe("Notes resolved comment visibility", () => {
  it("loads all threads with the page and filters every toggle locally", async () => {
    let pageId = "page-a";
    const controller = controllerForPage(() => pageId);

    await controller.reloadComments();
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a"]);
    await controller.setCommentsIncludeResolved(true);
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a", "resolved-a"]);
    await controller.setCommentsIncludeResolved(false);
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a"]);
    await controller.setCommentsIncludeResolved(true);
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a", "resolved-a"]);
    expect(backend.calls).toEqual([{ pageId: "page-a", includeResolved: true }]);

    await controller.setCommentsIncludeResolved(false);
    pageId = "page-b";
    await controller.reloadComments();
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-b"]);
    expect(backend.calls.at(-1)).toEqual({ pageId: "page-b", includeResolved: true });
  });

  it("applies the latest visibility choice to a pending page read", async () => {
    const controller = controllerForPage(() => "page-a");
    let releaseRead: () => void = () => undefined;
    backend.fullReadGate = new Promise<void>((resolve) => { releaseRead = resolve; });

    const pendingRead = controller.reloadComments();
    expect(controller.commentsLoading).toBe(true);
    await controller.setCommentsIncludeResolved(true);
    await controller.setCommentsIncludeResolved(false);
    await controller.setCommentsIncludeResolved(true);
    expect(backend.calls).toEqual([{ pageId: "page-a", includeResolved: true }]);

    releaseRead();
    await pendingRead;
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a", "resolved-a"]);
    await controller.setCommentsIncludeResolved(false);
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a"]);
  });

  it("preserves resolved threads across resolution and read updates", async () => {
    backend.threads = backend.threads.map((thread) =>
      thread.id === "open-a" ? { ...thread, unread: true } : thread);
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();
    await controller.setCommentsIncludeResolved(true);
    await controller.setCommentsIncludeResolved(false);

    await controller.markVisibleCommentThreadsRead();
    expect(backend.readCalls).toEqual([{ includeResolved: true, ids: ["open-a"] }]);
    expect(controller.commentThreads[0].unread).toBe(false);
    await controller.setCommentThreadResolved("open-a", true);
    expect(controller.commentThreads).toEqual([]);
    await controller.setCommentsIncludeResolved(true);
    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a", "resolved-a"]);
    expect(backend.calls).toEqual([{ pageId: "page-a", includeResolved: true }]);
  });

  it("uses the backend read state for visible resolved threads", async () => {
    backend.threads = backend.threads.map((thread) =>
      thread.id === "resolved-a" ? { ...thread, unread: true } : thread);
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();
    await controller.setCommentsIncludeResolved(true);

    await controller.markVisibleCommentThreadsRead();
    expect(backend.readCalls).toEqual([{ includeResolved: true, ids: ["resolved-a"] }]);
    expect(controller.commentThreads.find((thread) => thread.id === "resolved-a")?.unread).toBe(false);
    await controller.setCommentsIncludeResolved(false);
    await controller.setCommentsIncludeResolved(true);
    expect(backend.calls.filter((call) => call.includeResolved)).toHaveLength(1);
  });

  it("refreshes a pending page read after a comment changes", async () => {
    const controller = controllerForPage(() => "page-a");
    let releaseRead: () => void = () => undefined;
    backend.fullReadGate = new Promise<void>((resolve) => { releaseRead = resolve; });

    const pendingRead = controller.reloadComments();
    await controller.setCommentThreadResolved("open-a", true);
    expect(backend.calls.filter((call) => call.includeResolved)).toHaveLength(2);

    releaseRead();
    await pendingRead;
    await vi.waitFor(() => {
      expect(controller.commentsLoading).toBe(false);
      expect(controller.commentThreads).toEqual([]);
    });
    await controller.setCommentsIncludeResolved(true);
    expect(controller.commentThreads.every((thread) => thread.status === "resolved")).toBe(true);
  });
});

describe("Notes comment authoring", () => {
  it("keeps every loaded discussion available regardless of the resolved filter", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();

    expect(controller.commentThreads.map((thread) => thread.id)).toEqual(["open-a"]);
    expect(controller.allCommentThreads.map((thread) => thread.id)).toEqual(["open-a", "resolved-a"]);
  });

  it("trims comment bodies and refuses comments with nothing to post", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();

    expect(await controller.createComment([createTextRichText("   ")])).toBe(false);
    expect(backend.created).toEqual([]);
    expect(await controller.createComment([createTextRichText("  Ship it  ")])).toBe(true);
    expect(backend.created[0].parent).toEqual({ type: "page_id", page_id: "page-a" });
    expect(backend.created[0].rich_text.map((item) => item.plain_text)).toEqual(["Ship it"]);
  });

  it("posts an attachment without text", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();
    const attachment = { category: "file", name: "brief.pdf", file: { url: "https://example.com/brief.pdf" } };

    expect(await controller.createComment([], [attachment])).toBe(true);
    expect(backend.created[0]).toMatchObject({ rich_text: [], attachments: [attachment] });
  });

  it("leaves a pending block comment in place when the page composer posts", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();
    const blockParent: NotesCommentParent = { type: "block_id", block_id: "block-1" };
    controller.setActiveCommentParent(blockParent);

    await controller.createComment(
      [createTextRichText("Page note")],
      [],
      { type: "page_id", page_id: "page-a" },
    );
    expect(controller.activeCommentParent).toEqual(blockParent);

    await controller.createComment([createTextRichText("Block note")]);
    expect(backend.created.at(-1)?.parent).toEqual(blockParent);
    expect(controller.activeCommentParent).toBeNull();
  });

  it("replies and edits with attachments and keeps the thread list current", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();

    expect(await controller.replyToCommentThread("open-a", [createTextRichText(" ")])).toBe(false);
    expect(await controller.replyToCommentThread("open-a", [createTextRichText("Agreed")])).toBe(true);
    expect(backend.created[0].discussion_id).toBe("open-a");
    expect(controller.allCommentThreads.find((thread) => thread.id === "open-a")?.comments).toHaveLength(2);

    expect(await controller.updateComment("open-a-comment", [createTextRichText("Edited")], [])).toBe(true);
    expect(backend.updated[0]).toEqual({
      commentId: "open-a-comment",
      update: { rich_text: [expect.objectContaining({ plain_text: "Edited" })], attachments: [] },
    });
    expect(await controller.updateComment("open-a-comment", [], [])).toBe(false);
  });

  it("applies reactions and replies muting from the returned thread", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.reloadComments();

    await controller.toggleCommentReaction("open-a-comment", "👍");
    expect(backend.reactionCalls).toEqual([{ commentId: "open-a-comment", emoji: "👍" }]);
    expect(controller.commentThreads[0].comments[0].reactions).toEqual([
      { emoji: "👍", count: 1, reacted_by_me: true, names: ["Local user"] },
    ]);

    await controller.setCommentThreadMuted("open-a", true);
    expect(controller.commentThreads[0].muted).toBe(true);
  });
});

describe("Notes page notification level", () => {
  it("loads the level for the selected page and clears it on reset", async () => {
    backend.notificationLevel = "mentions";
    const controller = controllerForPage(() => "page-a");

    await controller.loadPageNotificationSettings();
    expect(controller.pageNotificationLevel).toBe("mentions");
    controller.resetPageState();
    expect(controller.pageNotificationLevel).toBeNull();
  });

  it("ignores a level that arrives after the page changed", async () => {
    let pageId = "page-a";
    const controller = controllerForPage(() => pageId);

    const pending = controller.loadPageNotificationSettings();
    pageId = "page-b";
    await pending;
    expect(controller.pageNotificationLevel).toBeNull();
  });

  it("saves a new level and restores the previous one when the write fails", async () => {
    const controller = controllerForPage(() => "page-a");
    await controller.loadPageNotificationSettings();

    await controller.setPageNotificationLevel("none");
    expect(controller.pageNotificationLevel).toBe("none");
    expect(backend.notificationLevel).toBe("none");

    backend.failNotificationWrite = true;
    await expect(controller.setPageNotificationLevel("all")).rejects.toThrow("write refused");
    expect(controller.pageNotificationLevel).toBe("none");
  });
});
