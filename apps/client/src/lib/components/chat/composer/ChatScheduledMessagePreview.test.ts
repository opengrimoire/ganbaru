// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatMessageReference, ChatScheduledMessageRead } from "$lib/chat/contracts";
import ChatScheduledMessagePreview from "./ChatScheduledMessagePreview.svelte";

function scheduledMessage(overrides: Partial<ChatScheduledMessageRead> = {}): ChatScheduledMessageRead {
  return {
    id: "scheduled:1",
    channelId: "channel:1",
    replyThreadId: null,
    normalizedMarkdown: "Ship the **release** notes",
    richContent: { schemaVersion: 1, value: {} },
    attachmentIds: [],
    references: [],
    alsoSendToChannel: false,
    state: "scheduled",
    scheduledFor: "2026-12-01T15:30:00.000Z",
    lastError: null,
    createdAt: "2026-11-14T10:00:00.000Z",
    recurrence: null,
    ...overrides,
  };
}

describe("ChatScheduledMessagePreview", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.unstubAllGlobals();
  });

  function mountPreview(
    message: ChatScheduledMessageRead,
    handlers: Partial<{ onSendNow: () => void; onRetry: () => Promise<void>; onEdit: () => void; onCancel: () => void }> = {},
    pending = false,
  ): HTMLDivElement {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatScheduledMessagePreview, {
      target,
      props: {
        message,
        pending,
        onSendNow: handlers.onSendNow ?? (() => undefined),
        onRetry: handlers.onRetry ?? (() => Promise.resolve()),
        onEdit: handlers.onEdit,
        onCancel: handlers.onCancel ?? (() => undefined),
      },
    });
    return target;
  }

  function actionLabels(root: HTMLElement): (string | null)[] {
    return [...root.querySelectorAll<HTMLButtonElement>(".message-action-controls button")]
      .map((button) => button.getAttribute("aria-label"));
  }

  it("renders the body as timeline Markdown with its references as inline text", async () => {
    const mention: ChatMessageReference = {
      kind: "participant",
      participantId: "participant:atlas",
      participantKind: "ai_teammate",
      metadata: { referenceId: "reference:atlas", labelSnapshot: "Atlas", startOffset: 0, endOffset: 6, plainTextProjection: "@Atlas" },
    };
    const root = mountPreview(scheduledMessage({ normalizedMarkdown: "@Atlas ship the **release**", references: [mention] }));

    await tick();
    const copy = root.querySelector(".message-copy");
    expect(copy?.querySelector("strong")?.textContent).toBe("release");
    expect(copy?.querySelector(".inline-reference")?.textContent).toBe("@Atlas");
    expect(copy?.textContent?.trim()).toBe("@Atlas ship the release");
  });

  it("offers send now, copy, edit, and cancel as toolbar actions that call their handlers", () => {
    const onSendNow = vi.fn();
    const onEdit = vi.fn();
    const onCancel = vi.fn();
    const root = mountPreview(scheduledMessage(), { onSendNow, onEdit, onCancel });

    expect(actionLabels(root)).toEqual(["Send now", "Copy", "Edit scheduled message", "Cancel scheduled message"]);
    expect(root.querySelector(".message-actions")).toBeNull();
    root.querySelector<HTMLButtonElement>('[data-message-action="send-now"]')?.click();
    root.querySelector<HTMLButtonElement>('[data-message-action="edit"]')?.click();
    root.querySelector<HTMLButtonElement>('[data-message-action="cancel"]')?.click();
    expect(onSendNow).toHaveBeenCalledOnce();
    expect(onEdit).toHaveBeenCalledOnce();
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("copies the scheduled text", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const root = mountPreview(scheduledMessage());

    root.querySelector<HTMLButtonElement>('[data-message-action="copy"]')?.click();

    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith("Ship the **release** notes"));
  });

  it("disables every change while the message is being sent", () => {
    const root = mountPreview(scheduledMessage({ state: "dispatching" }), { onEdit: () => undefined });

    for (const action of ["send-now", "edit", "cancel"]) {
      expect(root.querySelector<HTMLButtonElement>(`[data-message-action="${action}"]`)?.disabled).toBe(true);
    }
    expect(root.querySelector("[data-scheduled-delivery]")?.textContent).toBe("Sending now");
  });

  it("disables the changes of a message with a request in flight", () => {
    const root = mountPreview(scheduledMessage(), { onEdit: () => undefined }, true);

    expect(root.querySelector<HTMLButtonElement>('[data-message-action="edit"]')?.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>('[data-message-action="copy"]')?.disabled).toBe(false);
  });

  it("leaves out edit without a handler and send now for a failed message, which keeps its retry", () => {
    const onRetry = vi.fn().mockResolvedValue(undefined);
    const root = mountPreview(scheduledMessage({ state: "failed", lastError: "The channel was archived" }), { onRetry });

    expect(actionLabels(root)).toEqual(["Copy", "Cancel scheduled message"]);
    expect(root.querySelector("[data-scheduled-delivery]")).toBeNull();
    expect(root.textContent).toContain("The channel was archived");
    const retry = root.querySelector<HTMLButtonElement>("button[aria-label='Retry scheduled message']");
    retry?.click();
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("shows a one-time message's delivery as a single schedule line", () => {
    const root = mountPreview(scheduledMessage());

    const line = root.querySelector("[data-scheduled-delivery]");
    expect(line?.textContent).toMatch(/^Schedule: /u);
    expect(line?.querySelectorAll(".delivery-separator")).toHaveLength(0);
  });

  it("joins the next occurrence, the repeat rule, and the sent count on one line with separators", () => {
    const root = mountPreview(scheduledMessage({
      recurrence: {
        rule: { kind: "interval", minutes: 120 },
        timeZone: "UTC",
        quietPolicy: "defer",
        nextOccurrenceAt: "2026-12-01T17:30:00.000Z",
        firedCount: 3,
        skippedOccurrences: 2,
        deferredUntil: null,
      },
    }));

    const parts = [...root.querySelectorAll("[data-scheduled-delivery] > span:not(.delivery-separator)")]
      .map((part) => part.textContent);
    expect(parts).toHaveLength(3);
    expect(parts[0]).toMatch(/^Next: /u);
    expect(parts[1]).toContain("Every 2 hours");
    expect(parts[2]).toBe("Sent 3 times");
    expect(root.querySelectorAll("[data-scheduled-delivery] .delivery-separator")).toHaveLength(2);
    expect(root.querySelector(".message-context")).toBeNull();
    expect(root.textContent).toContain("Skipped 2 on this device");
  });

  it("leaves the sent count out of a repeating message that has not posted yet", () => {
    const root = mountPreview(scheduledMessage({
      recurrence: {
        rule: { kind: "interval", minutes: 60 },
        timeZone: "UTC",
        quietPolicy: "defer",
        nextOccurrenceAt: "2026-12-01T17:30:00.000Z",
        firedCount: 0,
        skippedOccurrences: 0,
        deferredUntil: null,
      },
    }));

    expect(root.querySelector("[data-scheduled-delivery]")?.textContent).not.toContain("Sent");
    expect(root.textContent).not.toContain("Skipped");
  });
});
