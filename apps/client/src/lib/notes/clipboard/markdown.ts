import type { NotesRichText, NotesRichTextAnnotations } from "$lib/notes/types";
import { notesClipboardLinkUrl } from "./links";

/** Escape literal inline content so copied text cannot become unintended Markdown. */
export function escapeNotesMarkdown(text: string): string {
  return text.replace(/[\\`*_[\]<>~|#!&]/gu, "\\$&")
    .replace(/^([ \t]*)([-+])(?=\s)/gmu, "$1\\$2")
    .replace(/^([ \t]*\d+)([.)])(?=\s)/gmu, "$1\\$2")
    .replace(/^([ \t]*)([-=]+)([ \t]*)$/gmu, "$1\\$2$3");
}

/** Choose a delimiter longer than every backtick run in the source. */
export function notesMarkdownFence(text: string, minimum = 3): string {
  let length = minimum;
  for (const match of text.matchAll(/`+/gu)) length = Math.max(length, match[0].length + 1);
  return "`".repeat(length);
}

interface MarkdownRun {
  text: string;
  annotations: NotesRichTextAnnotations;
  url: string | null;
}

/** Merge editor runs whose differences, such as color, have no Markdown representation. */
function markdownRuns(items: readonly NotesRichText[]): MarkdownRun[] {
  const runs: MarkdownRun[] = [];
  for (const item of items) {
    const rawUrl = item.type === "text" ? item.text.link?.url ?? item.href : item.href;
    const url = notesClipboardLinkUrl(rawUrl);
    const previous = runs.at(-1);
    const annotations = item.annotations;
    if (previous && previous.url === url
      && (["bold", "italic", "strikethrough", "underline", "code"] as const)
        .every((name) => previous.annotations[name] === annotations[name])) {
      previous.text += item.plain_text;
    } else runs.push({ text: item.plain_text, annotations, url });
  }
  return runs;
}

/** Serialize inline annotations and safe links using Markdown and portable inline HTML. */
export function notesRichTextMarkdown(items: readonly NotesRichText[]): string {
  return markdownRuns(items).map((item) => {
    const text = item.text;
    if (!text) return "";
    const leading = text.match(/^\s*/u)?.[0] ?? "";
    const trailing = text.match(/\s*$/u)?.[0] ?? "";
    const body = text.slice(leading.length, text.length - trailing.length);
    if (!body) return text;
    let content = escapeNotesMarkdown(body);
    if (item.annotations.code) {
      const fence = notesMarkdownFence(body, 1);
      const padding = body.startsWith("`") || body.endsWith("`") ? " " : "";
      content = `${fence}${padding}${body}${padding}${fence}`;
    }
    if (item.annotations.bold && item.annotations.italic) content = `***${content}***`;
    else if (item.annotations.bold) content = `**${content}**`;
    else if (item.annotations.italic) content = `*${content}*`;
    if (item.annotations.strikethrough) content = `~~${content}~~`;
    if (item.annotations.underline) {
      // Obsidian does not parse Markdown inside HTML elements.
      content = body.replace(/&/gu, "&amp;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;")
        .replace(/\n/gu, "<br>");
      if (item.annotations.code) content = `<code>${content}</code>`;
      if (item.annotations.bold) content = `<strong>${content}</strong>`;
      if (item.annotations.italic) content = `<em>${content}</em>`;
      if (item.annotations.strikethrough) content = `<s>${content}</s>`;
      content = `<u>${content}</u>`;
    }
    const url = item.url;
    if (url) content = `[${content}](<${url.replace(/</gu, "%3C").replace(/>/gu, "%3E")}>)`;
    return `${leading}${content}${trailing}`;
  }).join("").replace(/\n/gu, "  \n");
}
