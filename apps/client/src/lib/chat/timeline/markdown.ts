import DOMPurify, { type Config } from "dompurify";
import { Marked, type RendererObject } from "marked";

const MAX_CHAT_MARKDOWN_CHARS = 2_000_000;
const MAX_CHAT_MARKDOWN_LINES = 50_000;
const MAX_CHAT_MARKDOWN_LINE_CHARS = 32_768;
const MAX_MARKDOWN_NESTING = 16;
const UNSAFE_CONTROL_CHARACTERS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/g;
const FENCE_OPEN = /^[ \t]*(`{3,}|~{3,})(.*)$/;
const FENCE_CLOSE = /^ {0,3}(`{3,}|~{3,})[ \t]*$/;
const LIST_ITEM = /^ {0,3}(?:[-*+]|\d{1,9}[.)])(?:[ \t]|$)/;
const INDENTED = /^[ \t]/;
const TABLE_ROW = /^[ \t]*\|/;
const REFERENCE_DEFINITION = /\[[^\]]+\]:/;
const HTML_BLOCK_START = /^ {0,3}<[A-Za-z/!?]/;
const SAFE_LINK_PROTOCOLS = new Set(["http:", "https:"]);
const SANITIZER_CONFIG: Config = {
  ALLOWED_TAGS: [
    "a", "blockquote", "br", "code", "del", "em", "h1", "h2", "h3", "h4", "h5", "h6",
    "hr", "li", "ol", "p", "pre", "strong", "table", "tbody", "td", "th", "thead", "tr", "ul",
  ],
  ALLOWED_ATTR: ["href", "title", "data-chat-external-link"],
  ALLOW_DATA_ATTR: false,
};

const CHAT_MARKDOWN_RENDERER: RendererObject = {
  html({ text }): string {
    return escapeHtml(text);
  },

  link({ href, title, tokens }): string {
    const label = this.parser.parseInline(tokens);
    const target = resolveLinkAttribute(href);
    if (!safeExternalUrl(target)) return label;
    const safeHref = escapeAttribute(target);
    const safeTitle = title ? ` title="${escapeAttribute(resolveLinkAttribute(title))}"` : "";
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

/** Channel messages keep the single line breaks people type. */
const organizationalParser = new Marked({
  gfm: true,
  breaks: true,
  renderer: CHAT_MARKDOWN_RENDERER,
});

/** Rewrites a link destination or title during one synchronous parse, before the URL is validated. */
let resolveLinkAttribute: (value: string) => string = (value) => value;

/**
 * Parses normalized assistant Markdown and sanitizes the generated fragment.
 */
export function renderChatMarkdown(markdown: string): string {
  const bounded = boundChatMarkdown(markdown);
  return renderBoundedChatMarkdown(bounded);
}

/**
 * Parses channel message Markdown, keeping single line breaks, and sanitizes the fragment.
 *
 * @param markdown Untrusted message text.
 * @param resolveAttribute Rewrites each link destination and title before the destination is
 * validated, so text substituted into a link can never bypass the URL check.
 * @returns Sanitized HTML.
 */
export function renderOrganizationalChatMarkdown(
  markdown: string,
  resolveAttribute: (value: string) => string = (value) => value,
): string {
  const bounded = boundChatMarkdown(markdown);
  const previous = resolveLinkAttribute;
  resolveLinkAttribute = resolveAttribute;
  try {
    return sanitizeChatMarkdownHtml(organizationalParser.parse(bounded, { async: false }));
  } finally {
    resolveLinkAttribute = previous;
  }
}

/** One top-level Markdown block rendered to sanitized HTML. */
export interface RenderedMarkdownBlock {
  /** Position of the block in the document, stable while a stream appends to its end. */
  key: number;
  html: string;
}

/**
 * Renders Markdown block by block, reusing the HTML of blocks whose text is unchanged.
 *
 * The text is bounded once as a whole before splitting, so a split document never exceeds
 * the limits a whole one would. A document that `splitMarkdownBlocks` cannot split safely
 * renders as one block.
 *
 * @param markdown Untrusted Markdown, possibly still streaming.
 * @param cache Sanitized HTML by block text, owned by one rendered message. It is pruned to
 * the current blocks, so it never holds more entries than the message has blocks.
 * @returns The blocks in document order.
 */
export function renderChatMarkdownBlocks(markdown: string, cache: Map<string, string>): RenderedMarkdownBlock[] {
  const bounded = boundChatMarkdown(markdown);
  const texts = splitMarkdownBlocks(bounded) ?? [bounded];
  const blocks = texts.map((text, key) => {
    let html = cache.get(text);
    if (html === undefined) {
      html = renderBoundedChatMarkdown(text);
      cache.set(text, html);
    }
    return { key, html };
  });
  const current = new Set(texts);
  for (const text of cache.keys()) {
    if (!current.has(text)) cache.delete(text);
  }
  return blocks;
}

/**
 * Splits bounded Markdown into top-level blocks that render one by one to the same HTML as
 * the whole document.
 *
 * A split happens at a blank line outside a fenced code block when the next line is not
 * indented, not a list item continuing a list, and not a table row. Fences of three or more
 * backticks or tildes are tracked until a matching closing fence; an unclosed fence keeps the
 * rest of the text in its block. Blank lines between blocks are dropped, since they render
 * nothing, so a block's text stays the same while a stream appends after it.
 *
 * @param markdown Text already passed through `boundChatMarkdown`.
 * @returns The block texts, or `null` when the document uses syntax that crosses blank lines
 * in this parser configuration (link reference or footnote definitions, raw HTML blocks),
 * which must render whole.
 */
export function splitMarkdownBlocks(markdown: string): string[] | null {
  const blocks: string[] = [];
  let current: string[] = [];
  let fence: MarkdownFence | null = null;
  let previousBlank = false;
  let inList = false;
  for (const line of markdown.split("\n")) {
    if (fence) {
      current.push(line);
      if (closesFence(line, fence)) fence = null;
      continue;
    }
    if (REFERENCE_DEFINITION.test(line) || HTML_BLOCK_START.test(line)) return null;
    const blank = line.trim() === "";
    if (!blank) {
      if (previousBlank && hasContent(current) && startsNewBlock(line, inList)) {
        blocks.push(withoutTrailingBlankLines(current));
        current = [];
      }
      if (LIST_ITEM.test(line)) inList = true;
      else if (previousBlank && !INDENTED.test(line)) inList = false;
    }
    current.push(line);
    fence = opensFence(line);
    previousBlank = blank;
  }
  if (current.length > 0) blocks.push(fence ? current.join("\n") : withoutTrailingBlankLines(current));
  return blocks;
}
/**
 * Sanitizes a rendered Chat fragment. Among `data-*` attributes only the renderer's
 * `data-chat-external-link` marker is allowed, so other data attributes cannot reach handlers.
 */
export function sanitizeChatMarkdownHtml(html: string): string {
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

/** Returns whether a link target is an HTTP or HTTPS URL without embedded credentials. */
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

/** Parses and sanitizes text that `boundChatMarkdown` already bounded. */
function renderBoundedChatMarkdown(bounded: string): string {
  return sanitizeChatMarkdownHtml(parser.parse(bounded, { async: false }));
}
interface MarkdownFence {
  marker: "`" | "~";
  length: number;
}


/** Returns the fence a line opens; fences are read liberally, since a missed close only merges blocks. */
function opensFence(line: string): MarkdownFence | null {
  const match = FENCE_OPEN.exec(line);
  const run = match?.[1];
  if (!run) return null;
  const marker = run[0] === "~" ? "~" : "`";
  if (marker === "`" && match[2]?.includes("`")) return null;
  return { marker, length: run.length };
}

/** Returns whether a line closes a fence; closes are read strictly, since a false close would split inside code. */
function closesFence(line: string, fence: MarkdownFence): boolean {
  const run = FENCE_CLOSE.exec(line)?.[1];
  return run !== undefined && run[0] === fence.marker && run.length >= fence.length;
}

function startsNewBlock(line: string, inList: boolean): boolean {
  return !INDENTED.test(line) && !TABLE_ROW.test(line) && !(inList && LIST_ITEM.test(line));
}

function hasContent(lines: readonly string[]): boolean {
  return lines.some((line) => line.trim() !== "");
}

function withoutTrailingBlankLines(lines: readonly string[]): string {
  let end = lines.length;
  while (end > 0 && lines[end - 1]?.trim() === "") end -= 1;
  return lines.slice(0, end).join("\n");
}
