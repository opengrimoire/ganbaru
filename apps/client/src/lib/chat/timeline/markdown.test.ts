// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { boundChatMarkdown, renderChatMarkdown, safeExternalUrl } from "./markdown";

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
