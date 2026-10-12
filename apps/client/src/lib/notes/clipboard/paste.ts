import { notesClipboardLinkUrl } from "./links";
import { Marked } from "marked";
import { NOTES_MARKDOWN_LIST_PARAGRAPHS_ATTRIBUTE, normalizeNotesClipboardPlainText } from "./blocks";
import { sanitizeNotesRichHtml } from "$lib/notes/rich-text/paste";

const markdown = new Marked({
  gfm: true,
  renderer: {
    link({ href, tokens }) {
      const text = this.parser.parseInline(tokens);
      const url = notesClipboardLinkUrl(href);
      const escaped = href.replace(/&/gu, "&amp;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;").replace(/"/gu, "&quot;");
      return url ? `<a href="${escaped}">${text}</a>` : `${text} (${escaped})`;
    },
    image({ href, text }) {
      return `${text} (${href})`.replace(/&/gu, "&amp;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;");
    },
    code({ text, lang }) {
      const content = text.replace(/&/gu, "&amp;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;");
      const language = (lang?.split(/\s/u)[0] ?? "").replace(/[^a-zA-Z0-9_+.-]/gu, "");
      return `<pre><code class="language-${language}">${content}</code></pre>`;
    },
    html({ text }) {
      // Plain clipboard text is not an HTML execution or import channel.
      if (/^<\/?(?:u|strong|em|s|code)>$/iu.test(text) || /^<br\s*\/?\s*>$/iu.test(text)) return text;
      return text.replace(/&/gu, "&amp;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;");
    },
  },
});

/** Compare semantic text without treating layout whitespace as content differences. */
function comparableText(node: ParentNode): string {
  return (node.textContent ?? "").replace(/\s+/gu, "");
}

/** Prefer foldable Markdown when HTML exposes only a styled wrapper with the same words. */
function markdownRecoversFoldableCallout(original: ParentNode, rendered: ParentNode): boolean {
  if (Array.from(original.querySelectorAll("details")).some((details) => details.querySelector("summary"))) {
    return false;
  }
  const marker = /^\[![A-Za-z][A-Za-z0-9_-]*\][+-](?:[ \t]+|$)/u;
  const hasCallout = Array.from(rendered.querySelectorAll("blockquote > p"))
    .some((paragraph) => marker.test(paragraph.textContent ?? ""));
  if (!hasCallout) return false;
  return comparableText(original) === comparableText(rendered)
    .replace(/\[![A-Za-z][A-Za-z0-9_-]*\][+-]/gu, "");
}

function escapeHtmlAttribute(value: string): string {
  return value.replace(/&/gu, "&amp;").replace(/"/gu, "&quot;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;");
}

/** Read Notion's plain-text aside wrapper as nested editable callout blocks. */
function renderPlainTextAsides(text: string): string | null {
  const lines = text.split("\n");
  if (!lines.some((line) => line.trim() === "<aside>")) return null;
  const render = (source: readonly string[]): string | null => {
    let html = "";
    let plain: string[] = [];
    const flush = () => {
      if (plain.some((line) => line.trim())) html += markdown.parser(markdown.lexer(plain.join("\n")));
      plain = [];
    };
    for (let index = 0; index < source.length; index += 1) {
      const line = source[index].trim();
      if (line === "</aside>") return null;
      if (line !== "<aside>") {
        plain.push(source[index]);
        continue;
      }
      flush();
      const start = index + 1;
      let depth = 1;
      index = start;
      while (index < source.length && depth > 0) {
        const inner = source[index].trim();
        if (inner === "<aside>") depth += 1;
        else if (inner === "</aside>") depth -= 1;
        if (depth > 0) index += 1;
      }
      if (depth !== 0) return null;
      const body = source.slice(start, index);
      const first = body.findIndex((candidate) => candidate.trim().length > 0);
      const candidate = first >= 0 ? body[first].trim() : "";
      const isEmoji = /\p{Extended_Pictographic}/u.test(candidate)
        && [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(candidate)].length === 1;
      const icon = isEmoji ? { type: "emoji" as const, emoji: candidate } : null;
      const content = isEmoji ? body.slice(first + 1) : body;
      const bodyHtml = render(content);
      if (bodyHtml === null) return null;
      const metadata = icon ? ` data-notes-callout-icon-json="${escapeHtmlAttribute(JSON.stringify(icon))}"` : "";
      html += `<aside${metadata}><span data-notes-callout-marker>${isEmoji ? escapeHtmlAttribute(candidate) : ""}</span>${bodyHtml}</aside>`;
    }
    flush();
    return html;
  };
  return render(lines);
}

/** Resolve clipboard representations, using matching Markdown headings as authorial levels. */
export function notesClipboardPasteHtml(plainText: string, html = ""): string {
  const text = normalizeNotesClipboardPlainText(plainText);
  if (!text) return html;
  const asideHtml = renderPlainTextAsides(text);
  if (asideHtml) return /<aside\b/iu.test(html) ? html : asideHtml;
  const tokens = markdown.lexer(text);
  let formatted = false;
  markdown.walkTokens(tokens, (token) => {
    if (token.type === "html" && /^<\/?(?:u|strong|em|s|code)>|^<br\s*\/?\s*>/iu.test(token.raw)) formatted = true;
    if (["heading", "list", "blockquote", "code", "codespan", "strong", "em", "del", "link", "hr", "table", "image", "escape", "br"].includes(token.type)) formatted = true;
  });
  if (!formatted) return html;
  const rendered = markdown.parser(tokens)
    .replaceAll("<li>", `<li ${NOTES_MARKDOWN_LIST_PARAGRAPHS_ATTRIBUTE}>`);
  if (!html.trim()) return rendered;
  const original = sanitizeNotesRichHtml(html);
  const fromMarkdown = sanitizeNotesRichHtml(rendered);
  if (!original || !fromMarkdown) return html;
  if (markdownRecoversFoldableCallout(original, fromMarkdown)) return rendered;
  if (comparableText(original) !== comparableText(fromMarkdown)) return html;
  const headings = Array.from(original.querySelectorAll("h1,h2,h3,h4,h5,h6"));
  const markdownHeadings = Array.from(fromMarkdown.querySelectorAll("h1,h2,h3,h4,h5,h6"));
  if (!headings.length || headings.length !== markdownHeadings.length
    || headings.some((heading, index) => comparableText(heading) !== comparableText(markdownHeadings[index]))) return html;
  for (const [index, heading] of headings.entries()) {
    const replacement = document.createElement(markdownHeadings[index].tagName.toLowerCase());
    for (const attribute of heading.attributes) replacement.setAttribute(attribute.name, attribute.value);
    replacement.append(...heading.childNodes);
    heading.replaceWith(replacement);
  }
  const container = document.createElement("div");
  container.append(original);
  return container.innerHTML;
}

/** Read both representations from the same clipboard item for menu-based paste. */
export async function readNotesClipboard(): Promise<{ plainText: string; html: string }> {
  if (!navigator.clipboard) throw new Error("Notes clipboard is unavailable");
  if (navigator.clipboard.read) {
    try {
      const items = await navigator.clipboard.read();
      const item = items.find((candidate) => candidate.types.includes("text/html") || candidate.types.includes("text/plain"));
      if (item) return {
        plainText: item.types.includes("text/plain") ? await (await item.getType("text/plain")).text() : "",
        html: item.types.includes("text/html") ? await (await item.getType("text/html")).text() : "",
      };
    } catch (error) {
      console.warn("Notes rich clipboard read failed; trying text", error);
    }
  }
  return { plainText: await navigator.clipboard.readText(), html: "" };
}
