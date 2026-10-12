// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { TimelineActivityRow } from "$lib/chat/timeline/model";
import ChatActivityDetail from "./ChatActivityDetail.svelte";

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
});

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
    detail: null,
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
