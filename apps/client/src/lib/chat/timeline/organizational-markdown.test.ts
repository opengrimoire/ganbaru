// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import type { ChatMessageReference } from "$lib/chat/contracts";
import { chatUtf8Offset } from "$lib/chat/composer/message-references";
import { renderOrganizationalMarkdown } from "./organizational-markdown";

type ParticipantReference = Extract<ChatMessageReference, { kind: "participant" }>;

function mention(text: string, label: string, id = "atlas"): ParticipantReference {
  const start = text.indexOf(label);
  return {
    kind: "participant",
    metadata: {
      referenceId: `reference:${id}`,
      labelSnapshot: label.slice(1),
      startOffset: chatUtf8Offset(text, start),
      endOffset: chatUtf8Offset(text, start + label.length),
      plainTextProjection: label,
    },
    participantId: `participant:${id}`,
    participantKind: "ai_agent",
  };
}

function fragment(html: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content;
}

describe("renderOrganizationalMarkdown", () => {
  it("renders single newlines as line breaks and blank lines as paragraphs", () => {
    const { html } = renderOrganizationalMarkdown("first\nsecond\n\nthird", []);

    expect(html).toBe("<p>first<br>second</p>\n<p>third</p>\n");
  });

  it("places a slot for a mention in normal and bold text", () => {
    const text = "Ask **@Atlas** and @Nova";
    const rendered = renderOrganizationalMarkdown(text, [mention(text, "@Atlas"), mention(text, "@Nova", "nova")]);
    const content = fragment(rendered.html);

    expect(rendered.slots.map((slot) => [slot.index, slot.text])).toEqual([[0, "@Atlas"], [1, "@Nova"]]);
    expect(content.querySelector("strong > [data-chat-reference-slot='0']")).not.toBeNull();
    expect(content.querySelector("p > [data-chat-reference-slot='1']")).not.toBeNull();
    expect(content.querySelector("p")?.textContent).toBe("Ask  and ");
  });

  it("shows a mention inside inline code or a fence as literal text", () => {
    const text = "Run `ping @Atlas`\n\n```\n@Nova\n```";
    const rendered = renderOrganizationalMarkdown(text, [mention(text, "@Atlas"), mention(text, "@Nova", "nova")]);
    const content = fragment(rendered.html);

    expect(rendered.slots).toEqual([]);
    expect(content.querySelector("p > code")?.textContent).toBe("ping @Atlas");
    expect(content.querySelector("pre code")?.textContent).toBe("@Nova\n");
  });

  it("keeps a mention in a link label as text and never writes a slot into the destination", () => {
    const text = "[@Atlas docs](https://example.com/@Atlas)";
    const label = mention(text, "@Atlas");
    const destinationStart = text.lastIndexOf("@Atlas");
    const destination: ChatMessageReference = {
      ...mention(text, "@Atlas", "atlas-destination"),
      metadata: {
        ...mention(text, "@Atlas", "atlas-destination").metadata,
        startOffset: chatUtf8Offset(text, destinationStart),
        endOffset: chatUtf8Offset(text, destinationStart + 6),
      },
      participantId: "participant:other",
    };
    const rendered = renderOrganizationalMarkdown(text, [label, destination]);
    const link = fragment(rendered.html).querySelector("a");

    expect(rendered.slots).toEqual([]);
    expect(link?.getAttribute("href")).toBe("https://example.com/@Atlas");
    expect(link?.textContent).toBe("@Atlas docs");
    expect(rendered.html).not.toMatch(/[\uE000-\uF8FF]|%EE%80/u);
  });

  it("does not let reference text turn an unsafe destination into a link", () => {
    const text = "[open](javascript:@Atlas)";
    const rendered = renderOrganizationalMarkdown(text, [mention(text, "@Atlas")]);

    expect(rendered.html).not.toContain("<a");
    expect(rendered.html).not.toContain("javascript:");
  });

  it("removes forged sentinels and keeps other private use characters", () => {
    const text = "fake \uE0000\uE001 slot\uE000 and \uF8FF";
    const rendered = renderOrganizationalMarkdown(text, []);

    expect(rendered.slots).toEqual([]);
    expect(rendered.html).not.toMatch(/[\uE000\uE001]/u);
    expect(rendered.html).not.toContain("data-chat-reference-slot");
    expect(fragment(rendered.html).querySelector("p")?.textContent).toBe("fake 0 slot and \uF8FF");
  });

  it("maps a reference after multibyte characters to its slot", () => {
    const text = "日本語 👩‍💻 @Atlas!";
    const rendered = renderOrganizationalMarkdown(text, [mention(text, "@Atlas")]);
    const content = fragment(rendered.html);

    expect(rendered.slots).toMatchObject([{ index: 0, text: "@Atlas" }]);
    expect(content.querySelector("p")?.textContent).toBe("日本語 👩‍💻 !");
    expect(content.querySelector("[data-chat-reference-slot='0']")?.previousSibling?.textContent).toBe("日本語 👩‍💻 ");
  });

  it("drops references whose stored range no longer matches the text", () => {
    const text = "Ask @Atlas";
    const stale = { ...mention(text, "@Atlas"), metadata: { ...mention(text, "@Atlas").metadata, plainTextProjection: "@Nova" } };
    const rendered = renderOrganizationalMarkdown(text, [stale]);

    expect(rendered.slots).toEqual([]);
    expect(fragment(rendered.html).querySelector("p")?.textContent).toBe("Ask @Atlas");
  });

  it("still strips scripts, event handlers, and raw HTML", () => {
    const text = '<img src=x onerror="alert(1)"> <script>alert(1)</script> [x](https://example.com "t\" onclick=\"alert(1)")';
    const rendered = renderOrganizationalMarkdown(text, []);
    const content = fragment(rendered.html);

    expect(content.querySelector("img, script, [onerror], [onclick]")).toBeNull();
    expect(content.textContent).toContain("<script>alert(1)</script>");
  });
});
