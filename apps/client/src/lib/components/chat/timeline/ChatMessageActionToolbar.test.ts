// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatMessageActionTarget } from "$lib/chat/timeline/message-action-target";
import ChatMessageActionToolbar from "./ChatMessageActionToolbar.svelte";

const { loadChatMessageProvenance } = vi.hoisted(() => ({ loadChatMessageProvenance: vi.fn() }));

vi.mock("$lib/api/chat/provenance", () => ({ loadChatMessageProvenance }));

const ORGANIZATIONAL_TARGET: ChatMessageActionTarget = {
  kind: "organizational",
  sourceId: "item-1",
  reactionKey: "organization:item-1",
  copyText: "Posted message",
};

describe("ChatMessageActionToolbar", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.unstubAllGlobals();
    loadChatMessageProvenance.mockReset();
  });

  function mountToolbar(
    actionTarget: ChatMessageActionTarget,
    onError?: (error: unknown) => void,
  ): HTMLButtonElement {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageActionToolbar, { target, props: { target: actionTarget, visible: true, onError } });
    const more = target.querySelector<HTMLButtonElement>("button[aria-label='More Chat actions']");
    if (!more) throw new Error("the more actions button is missing");
    return more;
  }

  it("shows each device that sealed an organizational message", async () => {
    loadChatMessageProvenance.mockResolvedValue({
      devices: [
        { deviceId: "a", deviceLabel: "Desk", ownDevice: false },
        { deviceId: "b", deviceLabel: "Laptop", ownDevice: true },
        { deviceId: "c", deviceLabel: null, ownDevice: false },
      ],
      lastEditedBy: [],
    });

    mountToolbar(ORGANIZATIONAL_TARGET).click();

    await vi.waitFor(() => {
      const panel = document.querySelector("[role='dialog'][aria-label='Sent from']");
      expect([...(panel?.querySelectorAll("li") ?? [])].map((row) => row.textContent)).toEqual([
        "Desk",
        "This device",
        "Another device",
      ]);
      expect(panel?.textContent).not.toContain("Last edited from");
    });
    expect(loadChatMessageProvenance).toHaveBeenCalledWith("item-1");
  });

  it("names a device that edited the message after another created it", async () => {
    loadChatMessageProvenance.mockResolvedValue({
      devices: [{ deviceId: "a", deviceLabel: "Desk", ownDevice: false }],
      lastEditedBy: [{ deviceId: "b", deviceLabel: "Phone", ownDevice: false }],
    });

    mountToolbar(ORGANIZATIONAL_TARGET).click();

    await vi.waitFor(() => {
      const panel = document.querySelector("[role='dialog'][aria-label='Sent from']");
      expect([...(panel?.querySelectorAll(".menu-label") ?? [])].map((label) => label.textContent)).toEqual([
        "Sent from",
        "Last edited from",
      ]);
      expect([...(panel?.querySelectorAll("li") ?? [])].map((row) => row.textContent)).toEqual(["Desk", "Phone"]);
    });
  });

  it("shows only the editing device when the creation has no sync record", async () => {
    loadChatMessageProvenance.mockResolvedValue({
      devices: [],
      lastEditedBy: [{ deviceId: "b", deviceLabel: "Phone", ownDevice: false }],
    });

    mountToolbar(ORGANIZATIONAL_TARGET).click();

    await vi.waitFor(() => {
      const panel = document.querySelector("[role='dialog']");
      expect([...(panel?.querySelectorAll(".menu-label") ?? [])].map((label) => label.textContent)).toEqual([
        "Last edited from",
      ]);
    });
  });

  it("opens nothing for a message without a sync record", async () => {
    loadChatMessageProvenance.mockResolvedValue({ devices: [], lastEditedBy: [] });

    const more = mountToolbar(ORGANIZATIONAL_TARGET);
    more.click();

    await vi.waitFor(() => expect(loadChatMessageProvenance).toHaveBeenCalledOnce());
    await Promise.resolve();
    expect(document.querySelector("[role='dialog']")).toBeNull();
    expect(more.getAttribute("aria-expanded")).toBe("false");
  });

  it("reports a failed lookup in the reader's language and opens nothing", async () => {
    const cause = { code: "persistence", message: "Read Chat message provenance", retryable: true };
    loadChatMessageProvenance.mockRejectedValue(cause);
    const onError = vi.fn();

    const more = mountToolbar(ORGANIZATIONAL_TARGET, onError);
    more.click();

    await vi.waitFor(() => expect(onError).toHaveBeenCalledOnce());
    const reported: unknown = onError.mock.calls[0]?.[0];
    expect(reported).toBeInstanceOf(Error);
    expect((reported as Error).message).toBe("Could not read which devices sent this message");
    expect((reported as Error).cause).toBe(cause);
    expect(document.querySelector("[role='dialog']")).toBeNull();
    expect(more.getAttribute("aria-expanded")).toBe("false");
  });

  it("drops the failure of a lookup that a later press replaced", async () => {
    let rejectFirst: (reason: unknown) => void = () => undefined;
    loadChatMessageProvenance.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectFirst = reject; }));
    loadChatMessageProvenance.mockResolvedValueOnce({
      devices: [{ deviceId: "a", deviceLabel: "Desk", ownDevice: false }],
      lastEditedBy: [],
    });
    const onError = vi.fn();

    const more = mountToolbar(ORGANIZATIONAL_TARGET, onError);
    more.click();
    more.click();
    await vi.waitFor(() => expect(document.querySelector("[role='dialog']")).not.toBeNull());
    rejectFirst(new Error("the first lookup failed"));
    await tick();
    await tick();

    expect(onError).not.toHaveBeenCalled();
  });

  it("drops the failure of a lookup that outlives the toolbar", async () => {
    let rejectLookup: (reason: unknown) => void = () => undefined;
    loadChatMessageProvenance.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectLookup = reject; }));
    const onError = vi.fn();

    mountToolbar(ORGANIZATIONAL_TARGET, onError).click();
    await vi.waitFor(() => expect(loadChatMessageProvenance).toHaveBeenCalledOnce());
    if (component) await unmount(component);
    component = undefined;
    rejectLookup(new Error("the lookup failed after the channel changed"));
    await tick();
    await tick();

    expect(onError).not.toHaveBeenCalled();
  });

  it("keeps a run answer out of the sync log lookup", () => {
    mountToolbar({ ...ORGANIZATIONAL_TARGET, kind: "execution" }).click();

    expect(loadChatMessageProvenance).not.toHaveBeenCalled();
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
