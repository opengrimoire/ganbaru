// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatMessageActionTarget } from "$lib/chat/timeline/message-action-target";
import ChatMessageActionToolbar from "./ChatMessageActionToolbar.svelte";

describe("ChatMessageActionToolbar", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.unstubAllGlobals();
  });

  it("keeps the shared action order and reports copy completion", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const actionTarget: ChatMessageActionTarget = {
      kind: "execution",
      sourceId: "answer-1",
      reactionKey: "timeline:answer-1",
      copyText: "Completed answer",
    };
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageActionToolbar, {
      target,
      props: { target: actionTarget, visible: true },
    });

    const actions = [...target.querySelectorAll<HTMLButtonElement>("button")];
    expect(actions.map((button) => button.getAttribute("aria-label"))).toEqual([
      "Add reaction",
      "Copy",
      "More Chat actions",
    ]);
    expect(actions[1]?.querySelector(".lucide-copy")).not.toBeNull();
    expect(actions[1]?.dataset.appTooltipKeepOnClick).toBe("true");
    actions[1]?.click();

    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("Completed answer");
      expect(actions[1]?.getAttribute("aria-label")).toBe("Copied");
      expect(actions[1]?.getAttribute("title")).toBe("Copied");
      expect(actions[1]?.querySelector(".lucide-check")).not.toBeNull();
    });
  });
});
