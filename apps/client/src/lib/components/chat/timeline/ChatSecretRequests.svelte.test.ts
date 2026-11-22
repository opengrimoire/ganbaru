// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
import type { ChatReplyThreadId, ChatSecretRequestRead } from "$lib/chat/contracts";
import ChatSecretRequests from "./ChatSecretRequests.svelte";

const request: ChatSecretRequestRead = {
  id: "0a1b2c3d",
  assignmentId: "assignment:deploy",
  label: "API token",
  purpose: "Deploy the preview",
  createdAt: "2099-08-12T20:00:00.000Z",
  expiresAt: "2099-08-12T20:15:00.000Z",
};

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatSecretRequests across reply threads", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  it("drops the previous thread's requests and typed value when another thread opens, even if its list fails", async () => {
    vi.spyOn(chatApi, "listChatSecretRequests").mockImplementation((threadId: ChatReplyThreadId) => (
      threadId === "reply-thread:deploy"
        ? Promise.resolve([request])
        : Promise.reject(new Error("Could not list"))
    ));
    const props = $state({ replyThreadId: "reply-thread:deploy", refreshKey: 1, agentName: "Atlas" });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatSecretRequests, { target, props });
    await settle();

    const input = target.querySelector<HTMLInputElement>('input[type="password"]');
    if (!input) throw new Error("The request shows a value field");
    input.value = "sk-live-value";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    props.replyThreadId = "reply-thread:other";
    await settle();

    expect(target.querySelector("[data-secret-request]")).toBeNull();
    expect(target.querySelector<HTMLInputElement>('input[type="password"]')).toBeNull();

    props.replyThreadId = "reply-thread:deploy";
    await settle();
    expect(target.querySelector<HTMLInputElement>('input[type="password"]')?.value).toBe("");
  });

  it("keeps a failed answer from the previous thread out of the thread opened since", async () => {
    vi.spyOn(chatApi, "listChatSecretRequests").mockImplementation((threadId: ChatReplyThreadId) => (
      Promise.resolve(threadId === "reply-thread:deploy" ? [request] : [])
    ));
    let rejectDecline: ((cause: unknown) => void) | undefined;
    vi.spyOn(chatApi, "declineChatSecret").mockImplementation(() => new Promise<void>((_resolve, reject) => {
      rejectDecline = reject;
    }));
    const props = $state({ replyThreadId: "reply-thread:deploy", refreshKey: 1, agentName: "Atlas" });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatSecretRequests, { target, props });
    await settle();

    target.querySelector<HTMLButtonElement>(".secret-actions button[type=\"button\"]")?.click();
    await settle();
    props.replyThreadId = "reply-thread:other";
    await settle();
    rejectDecline?.(new Error("Could not decline"));
    await settle();

    expect(target.querySelector("[data-secret-requests]")).toBeNull();
  });
});
