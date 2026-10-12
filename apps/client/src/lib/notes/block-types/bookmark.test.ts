import { describe, expect, it } from "vitest";
import { createBookmarkPayload } from "$lib/notes/blocks/factory";
import { bookmarkCaptionPlainText, canOpenBookmarkUrl } from "./bookmark";

describe("notes bookmark blocks", () => {
  it("creates Notion-shaped bookmark payloads with optional captions", () => {
    expect(createBookmarkPayload("https://example.com")).toEqual({
      caption: [],
      url: "https://example.com",
    });

    const payload = createBookmarkPayload("https://example.com", "Read later");

    expect(bookmarkCaptionPlainText(payload)).toBe("Read later");
    expect(payload.caption[0]?.plain_text).toBe("Read later");
  });

  it("opens only HTTP and HTTPS bookmark URLs", () => {
    expect(canOpenBookmarkUrl("https://example.com")).toBe(true);
    expect(canOpenBookmarkUrl(" http://example.com ")).toBe(true);
    expect(canOpenBookmarkUrl("mailto:team@example.com")).toBe(false);
    expect(canOpenBookmarkUrl("javascript:alert(1)")).toBe(false);
    expect(canOpenBookmarkUrl("not a url")).toBe(false);
    expect(canOpenBookmarkUrl("")).toBe(false);
  });
});
