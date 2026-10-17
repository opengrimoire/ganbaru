import { describe, expect, it } from "vitest";
import { createEmbedPayload } from "$lib/notes/blocks/factory";
import { canOpenEmbedUrl, embedDisplayTitle, embedUrlPlainText } from "./embed";

describe("notes embed blocks", () => {
  it("creates Notion-shaped embed payloads", () => {
    const payload = createEmbedPayload("https://example.com/video");

    expect(payload).toEqual({ url: "https://example.com/video" });
    expect(embedUrlPlainText(payload)).toBe("https://example.com/video");
  });

  it("opens only HTTP and HTTPS embed URLs", () => {
    expect(canOpenEmbedUrl("https://example.com")).toBe(true);
    expect(canOpenEmbedUrl("http://example.com")).toBe(true);
    expect(canOpenEmbedUrl("file:///tmp/video.mp4")).toBe(false);
    expect(canOpenEmbedUrl("javascript:alert(1)")).toBe(false);
    expect(canOpenEmbedUrl("")).toBe(false);
  });

  it("derives a local display title without fetching metadata", () => {
    expect(embedDisplayTitle("https://player.vimeo.com/video/226053498")).toBe(
      "player.vimeo.com",
    );
    expect(embedDisplayTitle("not a url")).toBe("not a url");
    expect(embedDisplayTitle("   ")).toBe("");
  });
});
