import { describe, expect, it } from "vitest";
import type { ChatMessageReference } from "$lib/chat/contracts";
import {
  chatReferenceTextSegments,
  chatReferencesForEditor,
  chatReferencesForMarkdown,
  copyChatReferenceSlice,
  expandEditRangeToChatReferences,
  insertChatMessageReference,
  normalizeChatMessageReferences,
  pasteChatReferenceSlice,
  rebaseChatMessageReferences,
} from "./message-references";

function participantReference(
  referenceId: string,
  label = "Ágata",
): ChatMessageReference {
  return {
    kind: "participant",
    metadata: {
      referenceId,
      labelSnapshot: label,
      startOffset: 0,
      endOffset: 0,
      plainTextProjection: `@${label}`,
    },
    participantId: "participant:agata",
    participantKind: "ai_teammate",
  };
}

describe("message references", () => {
  it("stores UTF-8 byte ranges and renders a reference as one segment", () => {
    const inserted = insertChatMessageReference("Hola ", [], 5, 5, participantReference("ref:one"));

    expect(inserted.text).toBe("Hola @Ágata ");
    expect(inserted.references[0]?.metadata).toMatchObject({ startOffset: 5, endOffset: 12 });
    expect(chatReferenceTextSegments(inserted.text, inserted.references)).toEqual([
      { kind: "text", text: "Hola " },
      { kind: "reference", text: "@Ágata", reference: inserted.references[0] },
      { kind: "text", text: " " },
    ]);
  });

  it("expands partial edits and removes the whole atom", () => {
    const inserted = insertChatMessageReference("Review ", [], 7, 7, participantReference("ref:one"));
    const range = expandEditRangeToChatReferences(inserted.text, inserted.references, 9, 9);
    const nextText = `${inserted.text.slice(0, range.start)}x${inserted.text.slice(range.end)}`;

    expect(range).toEqual({ start: 7, end: 13 });
    expect(rebaseChatMessageReferences(
      inserted.text,
      nextText,
      inserted.references,
      range.start,
      range.end,
    )).toEqual([]);
  });

  it("prevents duplicate semantic targets", () => {
    const first = insertChatMessageReference("", [], 0, 0, participantReference("ref:one"));
    const duplicate = insertChatMessageReference(
      first.text,
      first.references,
      first.text.length,
      first.text.length,
      participantReference("ref:two"),
    );

    expect(duplicate.inserted).toBe(false);
    expect(duplicate.references).toHaveLength(1);
  });

  it("preserves semantic atoms only for an explicit same-vault clipboard slice", () => {
    const original = insertChatMessageReference("Ask ", [], 4, 4, participantReference("ref:one"));
    const slice = copyChatReferenceSlice("vault:one", original.text, original.references, 4, 10);
    const pasted = pasteChatReferenceSlice("Then ", [], 5, 5, slice);

    expect(pasted.text).toBe("Then @Ágata");
    expect(pasted.references).toHaveLength(1);
    expect(pasted.references[0]?.metadata.referenceId).not.toBe("ref:one");
  });

  it("drops overlaps, duplicate identities, controls, and broken byte boundaries", () => {
    const valid = insertChatMessageReference("", [], 0, 0, participantReference("ref:one"));
    const broken = participantReference("ref:broken", "\u0000bad");
    broken.metadata.startOffset = 0;
    broken.metadata.endOffset = 1;

    expect(normalizeChatMessageReferences(valid.text, [
      ...valid.references,
      { ...valid.references[0]!, metadata: { ...valid.references[0]!.metadata, referenceId: "ref:two" } },
      broken,
    ])).toEqual(valid.references);
  });

  it("projects visible atom ranges through Markdown formatting delimiters", () => {
    const document = {
      lines: [{
        runs: [
          { text: "Plan ", marks: ["bold" as const] },
          { text: "@Ágata", marks: [] },
        ],
      }],
    };
    const editorReference = participantReference("ref:one");
    editorReference.metadata.startOffset = 5;
    editorReference.metadata.endOffset = 12;
    const persisted = chatReferencesForMarkdown(document, [editorReference]);

    expect(persisted[0]?.metadata).toMatchObject({ startOffset: 9, endOffset: 16 });
    expect(chatReferencesForEditor(document, persisted)).toEqual([editorReference]);
  });

  it("keeps formatting delimiters outside a fully formatted atom range", () => {
    const document = {
      lines: [{ runs: [{ text: "@Ágata", marks: ["bold" as const] }] }],
    };
    const editorReference = participantReference("ref:one");
    editorReference.metadata.endOffset = 7;
    const persisted = chatReferencesForMarkdown(document, [editorReference]);

    expect(persisted[0]?.metadata).toMatchObject({ startOffset: 2, endOffset: 9 });
    expect(chatReferencesForEditor(document, persisted)).toEqual([editorReference]);
  });
});
