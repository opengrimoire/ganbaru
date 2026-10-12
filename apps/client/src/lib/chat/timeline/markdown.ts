import DOMPurify, { type Config } from "dompurify";
import { Marked, type RendererObject } from "marked";

const MAX_CHAT_MARKDOWN_CHARS = 2_000_000;
const MAX_CHAT_MARKDOWN_LINES = 50_000;
const MAX_CHAT_MARKDOWN_LINE_CHARS = 32_768;
const MAX_MARKDOWN_NESTING = 16;
const UNSAFE_CONTROL_CHARACTERS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/g;
const SAFE_LINK_PROTOCOLS = new Set(["http:", "https:"]);
const SANITIZER_CONFIG: Config = {
  ALLOWED_TAGS: [
    "a", "blockquote", "br", "code", "del", "em", "h1", "h2", "h3", "h4", "h5", "h6",
    "hr", "li", "ol", "p", "pre", "strong", "table", "tbody", "td", "th", "thead", "tr", "ul",
  ],
  ALLOWED_ATTR: ["href", "title", "data-chat-external-link"],
  ALLOW_DATA_ATTR: true,
};

const CHAT_MARKDOWN_RENDERER: RendererObject = {
  html({ text }): string {
    return escapeHtml(text);
  },

  link({ href, title, tokens }): string {
    const label = this.parser.parseInline(tokens);
    if (!safeExternalUrl(href)) return label;
    const safeHref = escapeAttribute(href);
    const safeTitle = title ? ` title="${escapeAttribute(title)}"` : "";
    return `<a href="${safeHref}" data-chat-external-link="true"${safeTitle}>${label}</a>`;
  },

  image({ text }): string {
    return `<span>${escapeHtml(text)}</span>`;
  },
};

const parser = new Marked({
  gfm: true,
  breaks: false,
  renderer: CHAT_MARKDOWN_RENDERER,
});

/**
 * Parses normalized assistant Markdown and sanitizes the generated fragment.
 */
export function renderChatMarkdown(markdown: string): string {
  const bounded = boundChatMarkdown(markdown);
  const html = parser.parse(bounded, { async: false });
  return DOMPurify.sanitize(html, SANITIZER_CONFIG);
}

/** Bounds untrusted stream size, line size, indentation, and quote nesting before parsing. */
export function boundChatMarkdown(markdown: string): string {
  const source = markdown.slice(0, MAX_CHAT_MARKDOWN_CHARS);
  const lines = source.split("\n", MAX_CHAT_MARKDOWN_LINES);
  return lines.map((sourceLine) => {
    let line = sourceLine
      .slice(0, MAX_CHAT_MARKDOWN_LINE_CHARS)
      .replace(UNSAFE_CONTROL_CHARACTERS, "");
    const quotePrefix = line.match(/^(?:>\s*)+/)?.[0] ?? "";
    if (quotePrefix) {
      const quotes = quotePrefix.match(/>/g)?.length ?? 0;
      if (quotes > MAX_MARKDOWN_NESTING) {
        line = `${"> ".repeat(MAX_MARKDOWN_NESTING)}${line.slice(quotePrefix.length)}`;
      }
    }
    const indentation = line.match(/^[ \t]+/)?.[0] ?? "";
    if (indentation.length > MAX_MARKDOWN_NESTING * 4) {
      line = `${indentation.slice(0, MAX_MARKDOWN_NESTING * 4)}${line.slice(indentation.length)}`;
    }
    return line;
  }).join("\n");
}

export function safeExternalUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return SAFE_LINK_PROTOCOLS.has(url.protocol) && url.username === "" && url.password === "";
  } catch {
    return false;
  }
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function escapeAttribute(value: string): string {
  return escapeHtml(value).replaceAll("`", "&#96;");
}
