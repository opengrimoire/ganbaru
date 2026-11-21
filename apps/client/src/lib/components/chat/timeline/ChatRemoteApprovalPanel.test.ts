// @vitest-environment jsdom
import { en } from "$lib/i18n/messages/en";

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatRemoteApprovalOutcome } from "$lib/chat/contracts";
import type { AnswerableRemoteApproval } from "$lib/chat/timeline/agent-update";
import ChatRemoteApprovalPanel from "./ChatRemoteApprovalPanel.svelte";

const api = vi.hoisted(() => ({
  resolve: vi.fn<(request: {
    assignmentId: string;
    requestId: string;
    decision: string;
    providerOptionId: string;
    commandId: string;
  }) => Promise<ChatRemoteApprovalOutcome>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  resolveChatRemoteApproval: api.resolve,
}));

const approval: AnswerableRemoteApproval = {
  messageItemId: "item:approval",
  assignmentId: "assignment:1",
  requestId: "request:1",
  subject: { kind: "command", text: "pnpm test", location: null, truncated: false },
  choices: [
    { id: "allow_once", label: "Allow once", decision: "allow_once" },
    { id: "deny", label: "Deny", decision: "deny" },
  ],
};

describe("ChatRemoteApprovalPanel", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    api.resolve.mockReset();
  });

  function setup(shown: AnswerableRemoteApproval, onAnswered = vi.fn<(requestId: string) => void>()) {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatRemoteApprovalPanel, { target, props: { approval: shown, agentName: "Atlas", onAnswered } });
    return { root: target, onAnswered };
  }

  function button(root: HTMLElement, label: string): HTMLButtonElement {
    const found = [...root.querySelectorAll("button")].find((candidate) => candidate.textContent === label);
    if (!found) throw new Error(`no ${label} button`);
    return found;
  }

  async function settle(): Promise<void> {
    for (let index = 0; index < 4; index += 1) await tick();
  }

  it("shows what Allow approves and sends the chosen decision to the desktop", async () => {
    api.resolve.mockResolvedValue({ kind: "resolved" });
    const { root, onAnswered } = setup(approval);

    expect(root.textContent).toContain(en.chat.remoteApproval.title("Atlas"));
    expect(root.querySelector("[data-testid='approval-subject'] pre")?.textContent).toBe("pnpm test");
    button(root, "Allow once").click();
    await settle();

    expect(api.resolve).toHaveBeenCalledWith(expect.objectContaining({
      assignmentId: "assignment:1",
      requestId: "request:1",
      decision: "allow_once",
      providerOptionId: "allow_once",
    }));
    expect(onAnswered).toHaveBeenCalledWith("request:1");
  });

  it("treats an approval answered elsewhere as answered", async () => {
    api.resolve.mockResolvedValue({ kind: "stale" });
    const { root, onAnswered } = setup(approval);
    button(root, "Deny").click();
    await settle();
    expect(onAnswered).toHaveBeenCalledWith("request:1");
  });

  it("keeps the decisions with an explanation when the desktop cannot take the answer", async () => {
    api.resolve
      .mockResolvedValueOnce({ kind: "unreachable" })
      .mockResolvedValueOnce({ kind: "refused" })
      .mockResolvedValueOnce({ kind: "notServed" });
    const { root, onAnswered } = setup(approval);

    button(root, "Deny").click();
    await settle();
    expect(root.textContent).toContain(en.chat.remoteApproval.unreachable);
    expect(button(root, "Deny").disabled).toBe(false);

    button(root, "Deny").click();
    await settle();
    expect(root.textContent).toContain(en.chat.remoteApproval.refused);

    button(root, "Deny").click();
    await settle();
    expect(root.textContent).toContain(en.chat.remoteApproval.notServed);
    expect(new Set(api.resolve.mock.calls.map(([request]) => request.commandId)).size).toBe(3);
    expect(onAnswered).not.toHaveBeenCalled();
  });

  it("explains a missing subject instead of showing an empty target", () => {
    const { root } = setup({ ...approval, subject: null, choices: [{ id: "deny", label: "Deny", decision: "deny" }] });
    expect(root.querySelector("[data-testid='approval-subject']")).toBeNull();
    expect(root.textContent).toContain(en.chat.remoteApproval.noSubject);
  });

  it("explains why a shortened target can only be denied here", () => {
    const subject = { kind: "command", text: "pnpm test", location: null, truncated: true } as const;
    const { root } = setup({ ...approval, subject, choices: [{ id: "deny", label: "Deny", decision: "deny" }] });
    expect(root.querySelector("[data-testid='approval-subject'] pre")?.textContent).toBe("pnpm test");
    expect(root.textContent).toContain(en.chat.remoteApproval.truncatedSubject);
  });
});
