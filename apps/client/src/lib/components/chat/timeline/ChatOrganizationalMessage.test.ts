// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
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
});
