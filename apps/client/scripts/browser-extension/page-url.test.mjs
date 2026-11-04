import { describe, expect, it } from "vitest";

import {
  decisionUrl,
  hostFromUrl,
  isSupportedPageUrl,
} from "../../../../extensions/chromium/page-url.js";

describe("Chromium extension page URLs", () => {
  it("evaluates only web pages, so browser and extension pages are never blocked", () => {
    expect(isSupportedPageUrl("https://example.com/")).toBe(true);
    expect(isSupportedPageUrl("http://example.com/")).toBe(true);
    expect(isSupportedPageUrl("chrome://settings/")).toBe(false);
    expect(isSupportedPageUrl("chrome://newtab/")).toBe(false);
    expect(isSupportedPageUrl("chrome-extension://abcdefghijklmnop/blocked.html")).toBe(false);
    expect(isSupportedPageUrl("edge://extensions/")).toBe(false);
    expect(isSupportedPageUrl("file:///home/user/page.html")).toBe(false);
    expect(isSupportedPageUrl(undefined)).toBe(false);
  });

  it("sends the native host the origin and path without query, fragment, or credentials", () => {
    expect(decisionUrl("https://user:secret@www.reddit.com/r/rust/comments/1?token=abc#top"))
      .toBe("https://www.reddit.com/r/rust/comments/1");
    expect(decisionUrl("http://example.com:8080/path?q=1")).toBe("http://example.com:8080/path");
    expect(decisionUrl("https://example.com")).toBe("https://example.com/");
    expect(decisionUrl("not a url")).toBeNull();
  });

  it("reads lowercase hosts and rejects unparseable URLs", () => {
    expect(hostFromUrl("https://WWW.Example.COM/Path")).toBe("www.example.com");
    expect(hostFromUrl("not a url")).toBeNull();
  });
});
