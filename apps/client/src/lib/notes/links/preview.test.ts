import { describe, expect, it } from "vitest";
import { createLinkPreviewPayload } from "$lib/notes/blocks/factory";
import {
  canOpenLinkPreviewUrl,
  linkPreviewDisplaySource,
  linkPreviewDisplayTitle,
  linkPreviewUrlPlainText,
} from "./preview";

describe("notes link preview blocks", () => {
  it("creates Notion-shaped link preview payloads", () => {
    const payload = createLinkPreviewPayload("https://github.com/example/repo/pull/123");

    expect(payload).toEqual({ url: "https://github.com/example/repo/pull/123" });
    expect(linkPreviewUrlPlainText(payload)).toBe("https://github.com/example/repo/pull/123");
  });

  it("opens only HTTP and HTTPS preview URLs", () => {
    expect(canOpenLinkPreviewUrl("https://example.com")).toBe(true);
    expect(canOpenLinkPreviewUrl("http://example.com")).toBe(true);
    expect(canOpenLinkPreviewUrl("file:///tmp/notes.html")).toBe(false);
    expect(canOpenLinkPreviewUrl("javascript:alert(1)")).toBe(false);
    expect(canOpenLinkPreviewUrl("")).toBe(false);
  });

  it("derives local display text without fetching metadata", () => {
    const url = "https://github.com/example/repo/pull/123?view=files";

    expect(linkPreviewDisplayTitle(url)).toBe("github.com");
    expect(linkPreviewDisplaySource(url)).toBe("github.com/example/repo/pull/123?view=files");
    expect(linkPreviewDisplayTitle("not a url")).toBe("not a url");
    expect(linkPreviewDisplaySource("   ")).toBe("");
  });
});
