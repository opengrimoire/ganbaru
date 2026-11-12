// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
import type { ChatSecretRequestRead } from "$lib/chat/contracts";
import ChatSecretRequests from "./ChatSecretRequests.svelte";

const request: ChatSecretRequestRead = {
  id: "0a1b2c3d",
  assignmentId: "assignment:deploy",
  label: "API token",
  purpose: "Deploy the preview",
  createdAt: "2026-08-12T20:00:00.000Z",
  expiresAt: "2026-08-12T20:15:00.000Z",
};

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatSecretRequests", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"] });
    vi.setSystemTime(new Date("2026-08-12T20:05:00.000Z"));
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  async function render(): Promise<HTMLDivElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatSecretRequests, {
      target,
      props: { replyThreadId: "reply-thread:deploy", refreshKey: 1, teammateName: "Atlas" },
    });
    await settle();
    return target;
  }

  function typeValue(root: HTMLElement, value: string): HTMLInputElement {
    const input = root.querySelector<HTMLInputElement>('input[type="password"]');
    if (!input) throw new Error("The request shows a value field");
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    return input;
  }

  it("provides the typed value, then clears it and drops the answered request", async () => {
    vi.spyOn(chatApi, "listChatSecretRequests").mockResolvedValue([request]);
    const provide = vi.spyOn(chatApi, "provideChatSecret").mockResolvedValue();
    const root = await render();

    expect(root.textContent).toContain("Atlas asks for a secret");
    expect(root.textContent).toContain("Deploy the preview");
    const input = typeValue(root, "sk-live-value");
    root.querySelector<HTMLFormElement>("form")?.requestSubmit();
    flushSync();
    expect(input.value).toBe("");
    await settle();

    expect(provide).toHaveBeenCalledWith(request.id, "sk-live-value");
    expect(root.querySelector("[data-secret-request]")).toBeNull();
  });

  it("shows a refusal in words and still clears the value", async () => {
    vi.spyOn(chatApi, "listChatSecretRequests").mockResolvedValue([request]);
    vi.spyOn(chatApi, "provideChatSecret").mockRejectedValue({ code: "validation", message: "secret_value_too_short" });
    const root = await render();

    const input = typeValue(root, "abc");
    root.querySelector<HTMLFormElement>("form")?.requestSubmit();
    flushSync();
    await settle();

    expect(input.value).toBe("");
    expect(root.textContent).toContain("The value is too short to keep out of Chat history.");
    expect(root.querySelector("[data-secret-request]")).not.toBeNull();
  });

  it("reads the requests again when one was answered elsewhere", async () => {
    const list = vi.spyOn(chatApi, "listChatSecretRequests")
      .mockResolvedValueOnce([request])
      .mockResolvedValueOnce([]);
    vi.spyOn(chatApi, "declineChatSecret").mockRejectedValue({ code: "conflict", message: "secret_request_not_pending" });
    const root = await render();

    root.querySelector<HTMLButtonElement>(".secret-actions button[type=button]")?.click();
    await settle();

    expect(list).toHaveBeenCalledTimes(2);
    expect(root.querySelector("[data-secret-requests]")).toBeNull();
  });

  it("drops a request when it expires", async () => {
    vi.spyOn(chatApi, "listChatSecretRequests").mockResolvedValue([request]);
    const root = await render();
    expect(root.querySelector("[data-secret-request]")).not.toBeNull();

    vi.advanceTimersByTime(10 * 60 * 1_000);
    flushSync();

    expect(root.querySelector("[data-secret-request]")).toBeNull();
  });
});
