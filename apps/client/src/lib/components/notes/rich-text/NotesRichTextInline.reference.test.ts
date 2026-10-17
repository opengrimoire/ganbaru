import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import { createDatabaseMentionRichText, createPageMentionRichText, createUserMentionRichText } from "$lib/notes/rich-text/core";
import type { NotesResolvedCommentAnchor } from "$lib/notes/collaboration/comments";
import type { NotesResolvedSuggestionAnchor } from "$lib/notes/collaboration/suggestions";
import type { NotesRichText } from "$lib/notes/types";
import NotesRichTextInline from "./NotesRichTextInline.svelte";

const PAGE_ID = "11111111-1111-4111-8111-111111111111";
const DATABASE_ID = "22222222-2222-4222-8222-222222222222";

/** Render the actual rich-text branch to verify editor metadata crosses the component boundary. */
function renderReference(
  richText: readonly NotesRichText[],
  commentAnchors: readonly NotesResolvedCommentAnchor[] = [],
  suggestionAnchors: readonly NotesResolvedSuggestionAnchor[] = [],
): string {
  return render(NotesRichTextInline, { props: { richText, commentAnchors, suggestionAnchors } }).body
    .replace(/<!--[\s\S]*?-->/gu, "");
}

describe("rich-text local reference rendering", () => {
  it("renders a page mention as a semantic anchor with no extra copied text or regular link popover marker", () => {
    const html = renderReference([createPageMentionRichText(PAGE_ID, "Project")]);
    expect(html).toContain(`href="#notes?page=${PAGE_ID}"`);
    expect(html).toContain(`data-notes-reference-id="${PAGE_ID}"`);
    expect(html).toContain('data-notes-reference-type="page"');
    expect(html).toContain("<svg");
    expect(html).not.toContain("data-notes-link-url");
    expect(html.replace(/<[^>]*>/gu, "")).toBe("Project");
  });

  it("forwards all annotation attributes onto a database reference with its existing destination", () => {
    const mention = createDatabaseMentionRichText(DATABASE_ID, "Tasks");
    mention.href = `#notes?page=${PAGE_ID}&block=${DATABASE_ID}`;
    mention.annotations = { bold: true, italic: true, underline: true, strikethrough: true, code: true, color: "blue_background" };
    const html = renderReference([mention]);
    for (const attribute of ["bold", "italic", "underline", "strikethrough", "code"]) {
      expect(html).toContain(`data-notes-${attribute}="true"`);
    }
    expect(html).toContain('data-notes-rich-text-color="blue_background"');
    expect(html).toContain(`data-notes-reference-id="${DATABASE_ID}"`);
    expect(html).toContain('data-notes-reference-type="database"');
    expect(html).toContain(`href="#notes?page=${PAGE_ID}&amp;block=${DATABASE_ID}"`);
    expect(html).not.toContain("data-notes-link-url");
    expect(html.replace(/<[^>]*>/gu, "")).toBe("Tasks");
  });

  it("preserves partial comment and suggestion anchors without changing the mention's text", () => {
    const comment: NotesResolvedCommentAnchor = { threadId: "comment", blockId: PAGE_ID, start: 1, end: 4, status: "open", text: "roj" };
    const suggestion: NotesResolvedSuggestionAnchor = {
      suggestionId: "suggestion", blockId: PAGE_ID, start: 2, end: 5, status: "accepted", originalText: "oje", proposedText: "new",
    };
    const html = renderReference([createPageMentionRichText(PAGE_ID, "Project")], [comment], [suggestion]);
    expect(html).toContain('data-notes-comment-anchor="comment"');
    expect(html).toContain('data-notes-suggestion-anchor="suggestion"');
    expect(html).toContain("notes-rich-text-comment-anchor");
    expect(html).toContain("notes-rich-text-suggestion-anchor-accepted");
    expect(html.replace(/<[^>]*>/gu, "")).toBe("Project");
  });

  it("keeps other mention kinds in their existing rendering branch", () => {
    const html = renderReference([createUserMentionRichText(PAGE_ID, "Teammate")]);
    expect(html).not.toContain("data-notes-reference-id");
    expect(html).not.toContain("<a ");
    expect(html.replace(/<[^>]*>/gu, "")).toBe("Teammate");
  });
});
