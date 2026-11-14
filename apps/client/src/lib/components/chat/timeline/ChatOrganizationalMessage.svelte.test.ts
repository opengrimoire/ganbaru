// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatMessageRead, ChatParticipantRead } from "$lib/chat/contracts";
import ChatOrganizationalMessage from "./ChatOrganizationalMessage.svelte";

const localParticipant: ChatParticipantRead = {
  id: "participant:local-owner",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

const message: ChatMessageRead = {
  itemId: "message:mention",
  conversationId: "conversation:general",
  replyThreadId: null,
  revisionId: "revision:mention",
  revision: 1,
  author: localParticipant,
  authorLabelSnapshot: "You",
  normalizedMarkdown: "Ask @Atlas now",
  richContent: { schemaVersion: 1, value: {} },
  attachmentIds: [],
  references: [{
    kind: "participant",
    metadata: {
      referenceId: "reference:atlas",
      labelSnapshot: "Atlas",
      startOffset: 4,
      endOffset: 10,
      plainTextProjection: "@Atlas",
    },
    participantId: "participant:atlas",
    participantKind: "ai_teammate",
  }],
  replyThread: null,
  orderKey: "c001",
  editedAt: null,
  createdAt: "2026-08-12T20:00:00.000Z",
};

describe("ChatOrganizationalMessage", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("renders structured mentions as identity tags without changing surrounding text", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatOrganizationalMessage, { target, props: { message } });
    await tick();

    const copy = target.querySelector<HTMLElement>(".message-copy");
    const mention = copy?.querySelector<HTMLButtonElement>(".mention-trigger");
    expect(mention?.textContent).toBe("@Atlas");
    expect(copy?.querySelector("p")?.textContent).toBe("Ask @Atlas now");

    mention?.click();
    await tick();
    expect(document.body.querySelector(".identity-card")?.textContent).toContain("Atlas");
  });

  it("renders Markdown with references mounted in place and code kept literal", async () => {
    const text = "**See #general** and `@Atlas`\n- @Atlas";
    const channelStart = text.indexOf("#general");
    const mentionStart = text.lastIndexOf("@Atlas");
    const atlas = message.references[0];
    if (atlas?.kind !== "participant") throw new Error("The fixture needs a participant reference.");
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatOrganizationalMessage, {
      target,
      props: {
        message: {
          ...message,
          normalizedMarkdown: text,
          references: [
            {
              kind: "channel",
              channelId: "conversation:general",
              metadata: { referenceId: "reference:general", labelSnapshot: "general", startOffset: channelStart, endOffset: channelStart + 8, plainTextProjection: "#general" },
            },
            { ...atlas, metadata: { ...atlas.metadata, startOffset: mentionStart, endOffset: mentionStart + 6 } },
          ],
        },
      },
    });
    await tick();

    const copy = target.querySelector<HTMLElement>(".message-copy");
    expect(copy?.querySelector("strong .inline-reference")?.textContent).toBe("#general");
    expect(copy?.querySelector("p code")?.textContent).toBe("@Atlas");
    expect(copy?.querySelector("li .mention-trigger")?.textContent).toBe("@Atlas");
    expect(copy?.querySelectorAll(".mention-trigger")).toHaveLength(1);
  });

  it("keeps one control per reference when the message refreshes with the same text", async () => {
    const text = "See #general and @Atlas";
    const channelStart = text.indexOf("#general");
    const mentionStart = text.indexOf("@Atlas");
    const atlas = message.references[0];
    if (atlas?.kind !== "participant") throw new Error("The fixture needs a participant reference.");
    const original: ChatMessageRead = {
      ...message,
      normalizedMarkdown: text,
      references: [
        {
          kind: "channel",
          channelId: "conversation:general",
          metadata: { referenceId: "reference:general", labelSnapshot: "general", startOffset: channelStart, endOffset: channelStart + 8, plainTextProjection: "#general" },
        },
        { ...atlas, metadata: { ...atlas.metadata, startOffset: mentionStart, endOffset: mentionStart + 6 } },
      ],
    };
    const props = $state({ message: original });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatOrganizationalMessage, { target, props });
    await tick();

    props.message = structuredClone(original);
    await tick();
    props.message = { ...structuredClone(original), normalizedMarkdown: `${text}!` };
    await tick();

    const copy = target.querySelector<HTMLElement>(".message-copy");
    expect(copy?.querySelectorAll(".inline-reference")).toHaveLength(1);
    expect(copy?.querySelectorAll(".mention-trigger")).toHaveLength(1);
    expect(copy?.querySelector("p")?.textContent).toBe("See #general and @Atlas!");
  });

  it("offers suggested answers as chips and names a delegation outcome's state", async () => {
    const onChooseAnswer = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatOrganizationalMessage, {
      target,
      props: {
        message: { ...message, references: [], normalizedMarkdown: "Which?" },
        answerChoices: ["Keep", "Drop"],
        onChooseAnswer,
      },
    });
    await tick();

    const chips = [...target.querySelectorAll<HTMLButtonElement>("[data-answer-choices] button")];
    expect(chips.map((chip) => chip.textContent)).toEqual(["Keep", "Drop"]);
    chips[1]?.click();
    expect(onChooseAnswer).toHaveBeenCalledWith("Drop");
    await unmount(component);

    component = mount(ChatOrganizationalMessage, {
      target,
      props: {
        message: {
          ...message,
          references: [],
          normalizedMarkdown: "Done",
          richContent: {
            schemaVersion: 1,
            value: {
              type: "agent_update",
              updateKind: "delegation",
              assignmentId: "assignment:parent",
              agentRunId: null,
              payload: { phase: "settled", childAssignmentId: "assignment:child", parentAssignmentId: "assignment:parent", state: "completed" },
            },
          },
        },
        answerChoices: ["Keep"],
      },
    });
    await tick();

    expect(target.querySelector("[data-answer-choices]")).toBeNull();
    expect(target.querySelector("[data-delegation-outcome]")?.textContent).toBe("Completed");
  });

  it("marks a reply strip whose thread holds an unseen failure", async () => {
    const thread = {
      id: "reply-thread:failed",
      replyCount: 1,
      lastActivityAt: "2026-08-12T20:05:00.000Z",
      participants: [],
      unread: false,
      unseenFailure: true,
      workState: null,
    } satisfies ChatMessageRead["replyThread"];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatOrganizationalMessage, {
      target,
      props: { message: { ...message, references: [], normalizedMarkdown: "Fix it", replyThread: thread } },
    });
    await tick();

    expect(target.querySelector(".reply-strip [data-chat-thread-failure]")?.getAttribute("aria-label"))
      .toBe("An agent run failed");
    await unmount(component);

    component = mount(ChatOrganizationalMessage, {
      target,
      props: {
        message: { ...message, references: [], normalizedMarkdown: "Fix it", replyThread: { ...thread, unseenFailure: false } },
      },
    });
    await tick();

    expect(target.querySelector(".reply-strip")).not.toBeNull();
    expect(target.querySelector("[data-chat-thread-failure]")).toBeNull();
  });

  it("explains a launch failure in localized text without its raw reason or a Retry it was not given", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatOrganizationalMessage, {
      target,
      props: {
        message: {
          ...message,
          references: [],
          normalizedMarkdown: "spawn codex ENOENT",
          richContent: {
            schemaVersion: 1,
            value: {
              type: "agent_update",
              updateKind: "failure",
              assignmentId: "assignment:launch",
              agentRunId: null,
              payload: { launchFailure: true },
            },
          },
        },
      },
    });
    await tick();

    const failure = target.querySelector("[data-chat-failure]");
    expect(failure?.querySelector(".notice-message")?.textContent)
      .toBe("You could not start: Chat could not finish that action, so try again.");
    expect(target.textContent).not.toContain("spawn codex ENOENT");
    expect(target.querySelector(".message-copy")).toBeNull();
    expect(failure?.querySelector("[data-notice-retry]")).toBeNull();
    expect(failure?.querySelector("[data-notice-copy]")).not.toBeNull();
  });
});
