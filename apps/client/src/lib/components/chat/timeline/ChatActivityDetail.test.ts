// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TimelineActivityRow } from "$lib/chat/timeline/model";
import ChatActivityDetail from "./ChatActivityDetail.svelte";

const { readChatActivityDetail } = vi.hoisted(() => ({ readChatActivityDetail: vi.fn() }));
vi.mock("$lib/api/chat/session", () => ({ readChatActivityDetail }));

describe("ChatActivityDetail", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
  });

  it("keeps file detail mounted while exposing an accessible animated disclosure state", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatActivityDetail, {
      target,
      props: { activity: fileChangeActivity(), detail: null },
    });
    mounted.push({ target, component });

    const trigger = target.querySelector<HTMLButtonElement>(".file-change-trigger");
    const region = target.querySelector<HTMLElement>(".file-change-region");
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(region?.getAttribute("aria-hidden")).toBe("true");
    expect(region?.inert).toBe(true);
    expect(region?.textContent).toContain("+new");

    trigger?.click();
    await tick();

    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    expect(region?.classList.contains("expanded")).toBe(true);
    expect(region?.getAttribute("aria-hidden")).toBe("false");
    expect(region?.inert).toBe(false);
  });

  it("shows a shortened command tail with a notice and loads the whole output once opened", async () => {
    readChatActivityDetail.mockResolvedValue({
      activityId: "command-1",
      detail: "first line\nlast line",
      updatedAt: "2026-07-28T17:00:05.000Z",
    });
    const target = document.createElement("div");
    document.body.append(target);
    const collapsed = mount(ChatActivityDetail, { target, props: { activity: truncatedCommandActivity(), detail: "last line", threadId: "thread-1", expanded: false } });
    mounted.push({ target, component: collapsed });
    await tick();

    expect(target.querySelector(".detail-notice")?.textContent).toBe("Showing the last 4 KB");
    expect(target.querySelector(".command-output")?.textContent).toBe("last line");
    expect(readChatActivityDetail).not.toHaveBeenCalled();

    const openTarget = document.createElement("div");
    document.body.append(openTarget);
    const opened = mount(ChatActivityDetail, { target: openTarget, props: { activity: truncatedCommandActivity(), detail: "last line", threadId: "thread-1", expanded: true } });
    mounted.push({ target: openTarget, component: opened });
    await vi.waitFor(() => {
      expect(openTarget.querySelector(".command-output")?.textContent).toBe("first line\nlast line");
    });
    expect(readChatActivityDetail).toHaveBeenCalledWith("thread-1", "command-1");
    expect(openTarget.querySelector(".detail-notice")).toBeNull();
  });

  it("keeps the preview and its notice when the whole output cannot be read", async () => {
    readChatActivityDetail.mockRejectedValue(new Error("Chat activity was not found"));
    const target = document.createElement("div");
    document.body.append(target);
    const activity = { ...truncatedCommandActivity(), id: "command-missing" };
    const component = mount(ChatActivityDetail, { target, props: { activity, detail: "last line", threadId: "thread-1", expanded: true } });
    mounted.push({ target, component });

    await vi.waitFor(() => expect(readChatActivityDetail).toHaveBeenCalledWith("thread-1", "command-missing"));
    await tick();
    expect(target.querySelector(".command-output")?.textContent).toBe("last line");
    expect(target.querySelector(".detail-notice")?.textContent).toBe("Showing the last 4 KB");
  });
});

function truncatedCommandActivity(): TimelineActivityRow {
  return {
    id: "command-1",
    kind: "activity",
    turnId: "turn-1",
    sequence: 1,
    createdAt: "2026-07-28T17:00:00.000Z",
    activityKind: "command_execution",
    status: "completed",
    title: "pnpm test",
    detailPreview: "last line",
    detailBytes: 9_000,
    detailTruncated: true,
    updatedAt: "2026-07-28T17:00:05.000Z",
    metadata: null,
  };
}

function fileChangeActivity(): TimelineActivityRow {
  return {
    id: "file-change-1",
    kind: "activity",
    turnId: "turn-1",
    sequence: 1,
    createdAt: "2026-07-28T17:00:00.000Z",
    activityKind: "file_change",
    status: "completed",
    title: "Edit file",
    detailPreview: null,
    detailBytes: 0,
    detailTruncated: false,
    metadata: {
      schemaVersion: 1,
      value: {
        changes: [{
          path: "src/app.ts",
          kind: "update",
          diff: "--- a/src/app.ts\n+++ b/src/app.ts\n-old\n+new",
        }],
      },
    },
  };
}
