// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesComment, NotesCommentThread, NotesPage, NotesRichText } from "$lib/notes/types";
import NotesCommentItem from "./NotesCommentItem.svelte";

const created = "2026-10-10T12:00:00.000Z";

const comment: NotesComment = {
  object: "comment",
  id: "comment-1",
  parent: { type: "page_id", page_id: "page-1" },
  discussion_id: "thread-1",
  created_time: created,
  last_edited_time: created,
  created_by: { object: "user", id: "local-user" },
  rich_text: [createRichText("Original")],
  attachments: [],
  display_name: { type: "user", resolved_name: "You" },
  deleted_at: null,
  reactions: [],
};

const thread: NotesCommentThread = {
  object: "comment_thread",
  id: "thread-1",
  parent: { type: "page_id", page_id: "page-1" },
  page_id: "page-1",
  block_id: null,
  status: "open",
  resolved_at: null,
  resolved_by: null,
  anchor: null,
  created_time: created,
  last_edited_time: created,
  unread: false,
  muted: false,
  comments: [comment],
};

const editor = vi.hoisted(() => ({
  store: {
    allPages: [] as NotesPage[],
    localUser: { id: "local-user" },
    updateComment: (_commentId: string, _richText: NotesRichText[], _attachments: Record<string, unknown>[]) => Promise.resolve(true),
    toggleCommentReaction: (_commentId: string, _emoji: string) => Promise.resolve(),
    setCommentThreadMuted: (_threadId: string, _muted: boolean) => Promise.resolve(),
    setCommentThreadResolved: (_threadId: string, _resolved: boolean) => Promise.resolve(),
    deleteComment: (_commentId: string) => Promise.resolve(),
  },
}));

vi.mock("$lib/components/notes/editor-context", () => ({
  getNotesEditor: () => editor.store,
}));

describe("NotesCommentItem", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    vi.restoreAllMocks();
  });

  function buttonNamed(name: string): HTMLButtonElement {
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.getAttribute("aria-label") === name || candidate.textContent?.trim() === name);
    if (!button) throw new Error(`Button ${name} is missing`);
    return button;
  }

  function editField(): HTMLElement | null {
    return target?.querySelector<HTMLElement>('[role="textbox"][aria-label="Edit comment text"]') ?? null;
  }

  async function startEdit(): Promise<void> {
    buttonNamed("More comment actions").click();
    await tick();
    buttonNamed("Edit comment").click();
    await tick();
  }

  it("keeps an edit reopened while an earlier save was still running", async () => {
    let finishSave: (saved: boolean) => void = () => undefined;
    vi.spyOn(editor.store, "updateComment").mockImplementation(() => new Promise<boolean>((resolve) => { finishSave = resolve; }));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesCommentItem, {
      target,
      props: {
        thread,
        comment,
        now: new Date(created),
        timeLabels: { justNow: "now", minutesAgo: (minutes) => `${minutes}m`, hoursAgo: (hours) => `${hours}h` },
      },
    });
    await tick();

    await startEdit();
    expect(editField()).not.toBeNull();
    buttonNamed("Save").click();
    await tick();
    buttonNamed("Cancel").click();
    await tick();
    expect(editField()).toBeNull();

    await startEdit();
    expect(editField()).not.toBeNull();
    finishSave(true);
    await tick();
    await tick();

    expect(editField()).not.toBeNull();
  });
});
