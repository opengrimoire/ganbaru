// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatPendingRequestRead, ChatUserInputDraftRead, UserInputAnswer, VersionedJson } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatRequestPanel from "./ChatRequestPanel.svelte";

const api = vi.hoisted(() => ({
  readDraft: vi.fn<() => Promise<ChatUserInputDraftRead | null>>(async () => null),
  saveDraft: vi.fn<(requestId: string, answers: VersionedJson) => Promise<ChatUserInputDraftRead>>(async (requestId, answers) => ({
    requestId,
    answers,
    updatedAt: "2026-07-21T12:00:00.000Z",
  })),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatUserInputDraft: api.readDraft,
  saveChatUserInputDraft: api.saveDraft,
}));

describe("ChatRequestPanel", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    api.readDraft.mockClear();
    api.saveDraft.mockClear();
  });

  afterEach(async () => {
    vi.useRealTimers();
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  function setup(pending: ChatPendingRequestRead): HTMLDivElement {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatRequestPanel, { target, props: { pending } });
    return target;
  }

  it("renders only provider-offered approval choices without focusing the permissive action", async () => {
    const resolve = vi.spyOn(getChat(), "resolveApproval").mockResolvedValue();
    const body = setup(approval());
    await tick();
    await Promise.resolve();
    await tick();
    const buttons = [...body.querySelectorAll<HTMLButtonElement>(".request-actions button")];
    expect(buttons.map((button) => button.textContent)).toEqual(["Allow once", "Deny"]);
    expect(document.activeElement).not.toBe(buttons[0]);
    expect(body.querySelector("section")?.contains(document.activeElement)).toBe(true);
    buttons[1]?.click();
    await tick();
    expect(resolve).toHaveBeenCalledWith({
      kind: "deny",
      providerOptionId: "deny",
      updatedToolInput: null,
    });
  });

  it("shows the message from a structured approval error", async () => {
    vi.spyOn(getChat(), "resolveApproval").mockRejectedValue({
      code: "stale_revision",
      message: "Chat thread revision is stale",
      recoverable: true,
    });
    const body = setup(approval());
    await tick();
    body.querySelector<HTMLButtonElement>(".request-actions button")?.click();
    await tick();
    await Promise.resolve();
    await tick();

    expect(body.querySelector('[role="alert"]')?.textContent?.trim()).toBe("Chat thread revision is stale");
    expect(body.textContent).not.toContain("[object Object]");
    expect(body.querySelector<HTMLButtonElement>(".request-actions button")?.disabled).toBe(false);
  });

  it("restores, validates, previews, and durably saves multi-step answers", async () => {
    vi.useFakeTimers();
    api.readDraft.mockResolvedValueOnce({
      requestId: "request-question",
      answers: {
        schemaVersion: 1,
        value: [{ questionId: "scope", selectedOptionIds: ["tests"], freeFormText: null }],
      },
      updatedAt: "2026-07-21T12:00:00.000Z",
    });
    const submitted: UserInputAnswer[][] = [];
    vi.spyOn(getChat(), "resolveUserInput").mockImplementation(async (answers) => { submitted.push(answers); });
    const body = setup(question());
    await tick();
    await Promise.resolve();
    await tick();
    expect(document.activeElement).toBe(body.querySelector('input[type="radio"]'));

    const tests = body.querySelector<HTMLInputElement>('input[value="tests"]')
      ?? [...body.querySelectorAll<HTMLInputElement>('input[type="radio"]')][0];
    expect(tests?.checked).toBe(true);
    const next = body.querySelector<HTMLButtonElement>(".request-actions button:last-child");
    next?.click();
    await tick();
    expect(body.textContent).toContain("Question 2 of 2");

    const option = body.querySelector<HTMLInputElement>('input[type="checkbox"]');
    option?.click();
    const freeForm = body.querySelector<HTMLTextAreaElement>(".free-form textarea");
    if (!freeForm) throw new Error("Free-form answer did not render");
    freeForm.value = "Include edge cases";
    freeForm.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(300);
    expect(api.saveDraft).toHaveBeenCalled();
    expect(body.textContent).toContain("Include edge cases");

    body.querySelector<HTMLButtonElement>(".request-actions button:last-child")?.click();
    await tick();
    expect(submitted[0]).toEqual([
      { questionId: "scope", selectedOptionIds: ["tests"], freeFormText: null },
      { questionId: "detail", selectedOptionIds: ["edge"], freeFormText: "Include edge cases" },
    ]);
  });
});

function approval(): ChatPendingRequestRead {
  return {
    id: "request-approval",
    turnId: "turn-1",
    providerRequestId: "provider-request-1",
    requestKind: "approval",
    safeDisplay: { schemaVersion: 1, value: { title: "Run tests", detail: "pnpm test", payload: null } },
    allowedDecisions: {
      schemaVersion: 1,
      value: [
        { id: "allow", label: "Allow once", decisionKind: "allow_once", description: null },
        { id: "deny", label: "Deny", decisionKind: "deny", description: null },
      ],
    },
    openedAt: "2026-07-21T12:00:00.000Z",
  };
}

function question(): ChatPendingRequestRead {
  return {
    id: "request-question",
    turnId: "turn-1",
    providerRequestId: "provider-request-2",
    requestKind: "user_input",
    safeDisplay: {
      schemaVersion: 1,
      value: [
        {
          id: "scope",
          header: "Scope",
          question: "What should be covered?",
          options: [{ id: "tests", label: "Tests", description: null }],
          multiple: false,
          freeFormAllowed: false,
          required: true,
        },
        {
          id: "detail",
          header: "Detail",
          question: "Which details?",
          options: [{ id: "edge", label: "Edge cases", description: null }],
          multiple: true,
          freeFormAllowed: true,
          required: true,
        },
      ],
    },
    allowedDecisions: { schemaVersion: 1, value: [] },
    openedAt: "2026-07-21T12:00:00.000Z",
  };
}
