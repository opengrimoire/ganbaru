import { describe, expect, it } from "vitest";
import { parseNotesSuggestion } from "$lib/notes/blocks/validation";
import {
  notesApplySuggestionToBlock,
  notesResolveSuggestionAnchor,
  notesSuggestionAnchorsForBlock,
  notesSuggestionCreateRequest,
  notesSuggestionDraft,
} from "./suggestions";
import { createLinkedTextRichText, createTextRichText } from "$lib/notes/rich-text/core";
import { richTextPlainText } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesSuggestion } from "$lib/notes/types";

const blockId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const pageId = "11111111-1111-4111-8111-111111111111";
const suggestionId = "40404040-4040-4040-8040-404040404040";

function suggestionFixture(overrides: Partial<NotesSuggestion> = {}): NotesSuggestion {
  return parseNotesSuggestion({
    object: "suggestion",
    id: suggestionId,
    page_id: pageId,
    block_id: blockId,
    created_by: { object: "user", id: "local-user" },
    display_name: { type: "user", resolved_name: "You" },
    status: "open",
    range_start: 6,
    range_end: 10,
    original_text: "beta",
    proposed_text: "delta",
    prefix: "Alpha ",
    suffix: " gamma",
    accepted_at: null,
    accepted_by: null,
    rejected_at: null,
    rejected_by: null,
    created_time: "2026-07-02T00:00:00.000Z",
    last_edited_time: "2026-07-02T00:00:00.000Z",
    ...overrides,
  });
}

function paragraphBlock(): NotesBlock {
  return {
    object: "block",
    id: blockId,
    parent: {
      type: "page_id",
      page_id: pageId,
    },
    created_time: "2026-07-02T00:00:00.000Z",
    last_edited_time: "2026-07-02T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "paragraph",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    paragraph: {
      rich_text: [
        createTextRichText("Alpha "),
        createLinkedTextRichText("link", "https://example.com"),
        createTextRichText(" beta"),
      ],
      color: "default",
    },
  };
}

describe("notes suggestions", () => {
  it("parses suggestion DTOs from the Tauri boundary", () => {
    const suggestion = suggestionFixture({
      status: "accepted",
      accepted_at: "2026-07-02T00:01:00.000Z",
      accepted_by: { object: "user", id: "local-user" },
    });

    expect(suggestion.status).toBe("accepted");
    expect(suggestion.original_text).toBe("beta");
    expect(suggestion.proposed_text).toBe("delta");
    expect(suggestion.display_name.resolved_name).toBe("You");
  });

  it("creates suggestion requests from selected text", () => {
    const draft = notesSuggestionDraft(blockId, "Alpha beta gamma", 6, 10);

    expect(draft).toMatchObject({
      block_id: blockId,
      range_start: 6,
      range_end: 10,
      original_text: "beta",
      proposed_text: "beta",
      prefix: "Alpha ",
      suffix: " gamma",
    });
    if (!draft) throw new Error("draft should exist");
    expect(notesSuggestionCreateRequest(suggestionId, draft, "delta")).toMatchObject({
      id: suggestionId,
      proposed_text: "delta",
    });
    expect(notesSuggestionDraft(blockId, "Alpha beta", 5, 6)).toBeNull();
  });

  it("relocates anchors with stored context", () => {
    const suggestion = suggestionFixture();

    expect(notesResolveSuggestionAnchor(suggestion, "Alpha beta gamma")).toMatchObject({
      start: 6,
      end: 10,
    });
    expect(notesResolveSuggestionAnchor(suggestion, "Intro Alpha beta gamma")).toMatchObject({
      start: 12,
      end: 16,
    });
    expect(notesSuggestionAnchorsForBlock([suggestion], blockId, "Intro Alpha beta gamma")).toHaveLength(1);
    expect(notesResolveSuggestionAnchor(suggestion, "Alpha gamma")).toBeNull();
  });

  it("applies suggestions without dropping rich text outside the target range", () => {
    const block = paragraphBlock();
    const suggestion = suggestionFixture({
      range_start: 11,
      range_end: 15,
      original_text: "beta",
      proposed_text: "delta",
      prefix: "Alpha link ",
      suffix: "",
    });

    const plan = notesApplySuggestionToBlock(block, suggestion);

    expect(plan).not.toBeNull();
    if (!plan) throw new Error("plan should exist");
    expect(richTextPlainText(plan.richText)).toBe("Alpha link delta");
    expect(plan.richText.some((item) => item.type === "text" && item.text.link?.url === "https://example.com")).toBe(true);
    expect(plan.selectionStart).toBe(11);
    expect(plan.selectionEnd).toBe(16);
  });
});
