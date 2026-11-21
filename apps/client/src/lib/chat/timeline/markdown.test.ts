// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { boundChatMarkdown, renderChatMarkdown, renderChatMarkdownBlocks, safeExternalUrl, sanitizeChatMarkdownHtml, splitMarkdownBlocks } from "./markdown";

describe("Chat Markdown", () => {
  it("renders common Markdown and preserves incomplete streaming fences as inert text", () => {
    expect(renderChatMarkdown("# Result\n\n| A | B |\n| - | - |\n| 1 | 2 |")).toContain("<table>");
    const incomplete = renderChatMarkdown("```ts\nconst value = '<unsafe>';");
    expect(incomplete).toContain("<pre><code");
    expect(incomplete).toContain("&lt;unsafe&gt;");
  });

  it("rejects raw HTML, active content, remote images, and credential-bearing links", () => {
    const html = renderChatMarkdown('<script>alert(1)</script><img src="https://tracker.example/x">\n\n[bad](javascript:alert(1)) [credentials](https://user:pass@example.com)');
    expect(html).not.toContain("<script");
    expect(html).not.toContain("<img");
    expect(html).not.toContain("javascript:");
    expect(html).not.toContain("user:pass");
    expect(html).toContain("&lt;script&gt;");
  });

  it("marks only explicit HTTP and HTTPS links for user-initiated opening", () => {
    expect(safeExternalUrl("https://example.com/path")).toBe(true);
    expect(safeExternalUrl("http://example.com/path")).toBe(true);
    expect(safeExternalUrl("file:///etc/passwd")).toBe(false);
    expect(safeExternalUrl("https://token@example.com")).toBe(false);
    expect(renderChatMarkdown("[Docs](https://example.com)")).toContain('data-chat-external-link="true"');
  });

  it("drops data attributes other than the external link marker", () => {
    const html = sanitizeChatMarkdownHtml('<p data-selectable-content data-action="open">text</p><a href="https://example.com" data-chat-external-link="true" data-x="1">link</a>');
    expect(html).not.toContain("data-selectable-content");
    expect(html).not.toContain("data-action");
    expect(html).not.toContain("data-x");
    expect(html).toContain('data-chat-external-link="true"');
  });

  it("bounds adversarial line length and nesting before parsing", () => {
    const bounded = boundChatMarkdown(`${"> ".repeat(100)}deep\n${"x".repeat(40_000)}`);
    expect(bounded.match(/>/g)?.length).toBe(16);
    expect(bounded.split("\n")[1]?.length).toBe(32_768);
    expect(renderChatMarkdown("[incomplete](https://example")).toContain("[incomplete]");
  });

  it("renders script variants, terminal escapes, and crafted local links inertly", () => {
    const rendered = renderChatMarkdown([
      '<svg onload="globalThis.compromised=true"><script>alert(1)</script></svg>',
      "\u001b]8;;https://malicious.example\u0007terminal link\u001b]8;;\u0007",
      "[workspace escape](file:///etc/passwd)",
      "[command](javascript:globalThis.compromised=true)",
      "[data](data:text/html,<script>alert(1)</script>)",
      "`<img src=x onerror=alert(1)>`",
    ].join("\n\n"));

    expect(rendered).not.toMatch(/<script|<svg|<img|javascript:|file:|data:text/i);
    expect(rendered).not.toContain("\u001b");
    expect(rendered).toContain("workspace escape");
    expect(rendered).toContain("&lt;img src=x onerror=alert(1)&gt;");
  });

  it("bounds huge tables, deep mixed nesting, long lines, and pathological Unicode", () => {
    const columns = Array.from({ length: 2_000 }, (_, index) => `column-${index}`);
    const table = `${columns.join("|")}\n${columns.map(() => "---").join("|")}\n${columns.join("|")}`;
    const nested = `${"> ".repeat(1_000)}${"    ".repeat(1_000)}deep`;
    const unicode = "🚀́‏👩‍💻".repeat(20_000);
    const bounded = boundChatMarkdown(`${table}\n${nested}\n${unicode}`);
    const lines = bounded.split("\n");

    expect(lines.every((line) => line.length <= 32_768)).toBe(true);
    expect(lines[3]?.match(/>/g)?.length).toBeLessThanOrEqual(16);
    expect(() => renderChatMarkdown(bounded)).not.toThrow();
    expect(renderChatMarkdown(table)).toContain("<table>");
  });
});

describe("Chat Markdown blocks", () => {
  const corpus = [
    "# Title\n\nIntro paragraph with **bold** and `code`.\n\n## Section\n\nMore text\nwith a soft break.",
    "- one\n- two\n\n- loose three\n\n  continued paragraph\n\nAfter the list.",
    "1. first\n2. second\n\nText\n\n3. restarted",
    "> quoted\n> lines\n\n> second quote\n\nPlain",
    "```ts\nconst a = 1;\n\nconst b = 2;\n```\n\nAfter code.\n\n~~~\ntilde\n\n~~~\n\nEnd",
    "| A | B |\n| - | - |\n| 1 | 2 |\n\n| C |\n| - |\n| 3 |\n\nTail",
    "Paragraph\n\n    indented code\n\n    more code\n\nBack",
    "- item\n\n  ```\n  nested fence\n\n  still code\n  ```\n\nDone",
    "Text\n\n---\n\nSetext\n===\n\n* * *",
    "````md\n```\ninner\n```\n\nouter still open\n````\n\nAfter",
    "Unclosed\n\n```\ncode\n\nmore",
    "[Docs](https://example.com) and [bad](javascript:alert(1))\n\n<script>alert(1)</script>",
  ];

  function joinedBlocks(markdown: string): string {
    return renderChatMarkdownBlocks(markdown, new Map()).map((block) => block.html).join("");
  }

  it("renders the same sanitized HTML block by block as the whole document", () => {
    for (const markdown of corpus) expect(joinedBlocks(markdown)).toBe(renderChatMarkdown(markdown));
    expect(joinedBlocks(corpus.join("\n\n"))).toBe(renderChatMarkdown(corpus.join("\n\n")));
  });

  it("splits at blank lines between top-level blocks", () => {
    expect(splitMarkdownBlocks("# Title\n\nFirst\n\n\nSecond")).toEqual(["# Title", "First", "Second"]);
  });

  it("keeps unclosed fences, loose lists, tables, and indented continuations in one block", () => {
    expect(splitMarkdownBlocks("Intro\n\n```ts\nconst a = 1;\n\nconst b = 2;")).toEqual(["Intro", "```ts\nconst a = 1;\n\nconst b = 2;"]);
    expect(splitMarkdownBlocks("- one\n\n- two\n\n  more")).toHaveLength(1);
    expect(splitMarkdownBlocks("| A |\n| - |\n\n| 1 |")).toHaveLength(1);
    expect(splitMarkdownBlocks("Text\n\n    code")).toHaveLength(1);
    expect(splitMarkdownBlocks("```\na\n    ```\n\nstill code\n```")).toHaveLength(1);
  });

  it("returns null for syntax that crosses blank lines", () => {
    expect(splitMarkdownBlocks("See [docs][d].\n\n[d]: https://example.com")).toBeNull();
    expect(splitMarkdownBlocks("Note[^1]\n\n[^1]: The footnote.")).toBeNull();
    expect(splitMarkdownBlocks("<pre>\n\nraw\n</pre>")).toBeNull();
    expect(splitMarkdownBlocks("<!-- comment\n\nstill comment -->")).toBeNull();
    expect(renderChatMarkdownBlocks("See [docs][d].\n\n[d]: https://example.com", new Map())).toHaveLength(1);
  });

  it("renders only the last block again while a stream grows", () => {
    const cache = new Map<string, string>();
    const set = vi.spyOn(cache, "set");
    const first = renderChatMarkdownBlocks("# Title\n\nFirst paragraph\n\nSecond", cache);
    set.mockClear();

    const grown = renderChatMarkdownBlocks("# Title\n\nFirst paragraph\n\nSecond paragraph", cache);

    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith("Second paragraph", expect.any(String));
    expect(grown.map((block) => block.key)).toEqual([0, 1, 2]);
    expect(grown.slice(0, 2)).toEqual(first.slice(0, 2));
    expect([...cache.keys()]).toEqual(["# Title", "First paragraph", "Second paragraph"]);
  });

  it("keeps a block's text unchanged when a blank line follows it", () => {
    const cache = new Map<string, string>();
    renderChatMarkdownBlocks("First\n\n", cache);
    const set = vi.spyOn(cache, "set");

    renderChatMarkdownBlocks("First\n\nSecond", cache);

    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith("Second", expect.any(String));
  });

  it("bounds the whole text before splitting", () => {
    const markdown = "paragraph\n\n".repeat(30_000);
    const blocks = renderChatMarkdownBlocks(markdown, new Map());

    expect(blocks).toHaveLength(25_000);
    expect(blocks.map((block) => block.html).join("")).toBe(renderChatMarkdown(markdown));
  });
});
