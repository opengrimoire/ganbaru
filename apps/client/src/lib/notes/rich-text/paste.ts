import { readNotesClipboardTable, type NotesClipboardTable } from "$lib/notes/clipboard/html-table";
import { notesClipboardLinkUrl } from "$lib/notes/clipboard/links";
import { isNotesUuid, parseNotesLinkHash } from "$lib/notes/links/block-link";
import DOMPurify, { type Config } from "dompurify";
import {
  blockEditableRichText,
  blockWithRichText,
  createCalloutPayload,
  createCodePayload,
  createTextPayloadFromRichText,
} from "$lib/notes/blocks/factory";
import {
  createLinkedTextRichText,
  createDatabaseMentionRichText,
  createPageMentionRichText,
  createTextRichText,
  defaultRichTextAnnotations,
  isNotesExternalMentionRichText,
  replaceRichTextRange,
  richTextPlainText,
  richTextRangeSlice,
  type NotesExternalMentionRichText,
} from "./core";
import { parseNullableNotesIcon } from "$lib/notes/validation/assets";
import { parseNotesRichTextArray } from "$lib/notes/validation/rich-text";
import {
  NOTES_CLIPBOARD_MAX_BLOCKS,
  NOTES_CLIPBOARD_MAX_TEXT_LENGTH,
  NOTES_CLIPBOARD_MENTION_ATTRIBUTE,
  NOTES_CLIPBOARD_MENTION_MAX_LENGTH,
  NOTES_MARKDOWN_LIST_PARAGRAPHS_ATTRIBUTE,
} from "$lib/notes/clipboard/blocks";
import {
  NOTES_COLORS,
  type NotesBlock,
  type NotesBlockUpdate,
  type NotesBlockWrite,
  type NotesColor,
  type NotesIcon,
  type NotesRichText,
  type NotesRichTextAnnotations,
  type NotesRichTextLink,
} from "$lib/notes/types";
import { cloneData } from "$lib/utils/clone";

type NotesRichHtmlPasteBlockType =
  | "paragraph"
  | "heading_1"
  | "heading_2"
  | "heading_3"
  | "heading_4"
  | "heading_5"
  | "heading_6"
  | "bulleted_list_item"
  | "numbered_list_item"
  | "toggle"
  | "callout"
  | "quote"
  | "to_do"
  | "divider"
  | "table"
  | "table_row"
  | "child_page"
  | "child_database"
  | "code";

interface NotesRichHtmlPasteSegment {
  type: NotesRichHtmlPasteBlockType;
  richText: NotesRichText[];
  depth?: number;
  language?: string;
  checked?: boolean;
  open?: boolean;
  table?: NotesClipboardTable;
  cells?: NotesRichText[][];
  icon?: NotesIcon | null;
  color?: NotesColor;
  sourcePageId?: string;
  sourceDatabaseId?: string;
}

type NotesRichHtmlInlineReference =
  | { type: "page" | "database"; id: string; href: string | null }
  | { type: "mention"; item: NotesExternalMentionRichText };

interface NotesRichHtmlInlineContext {
  annotations: NotesRichTextAnnotations;
  linkUrl: string | null;
  preformatted: boolean;
  reference?: NotesRichHtmlInlineReference | null;
}

export interface NotesRichHtmlPastePlan {
  currentUpdate: NotesBlockUpdate;
  appendedBlocks: NotesBlockWrite[];
  blockDepths: number[];
  focusBlockId: string;
  focusOffset: number;
  /** New paired block identities mapped to local source notes for canonical duplication. */
  copiedPageIds?: Readonly<Record<string, string>>;
  /** New database block identities mapped to local sources for canonical graph duplication. */
  copiedDatabaseIds?: Readonly<Record<string, string>>;
}

export interface NotesRichHtmlPastePlanInput {
  currentBlock: NotesBlock;
  selectionStart: number;
  selectionEnd: number;
  html: string;
  createId: () => string;
}

const NOTES_RICH_HTML_MAX_LENGTH = 128 * 1024;

const SANITIZER_CONFIG = {
  ALLOWED_TAGS: [
    "a",
    "aside",
    "b",
    "blockquote",
    "br",
    "code",
    "del",
    "details",
    "div",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "input",
    "img",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "i",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "span",
    "strike",
    "strong",
    "summary",
    "u",
    "ul",
  ],
  ALLOWED_ATTR: [
    "data-notes-bold",
    "data-notes-code",
    "data-notes-callout-color",
    "data-notes-callout-icon-json",
    "data-notes-callout-label",
    "data-notes-callout-marker",
    "data-notes-italic",
    "data-notes-link-url",
    "data-notes-rich-text-color",
    "data-notes-strikethrough",
    "data-notes-underline",
    "href",
    "type",
    "checked",
    "open",
    "class",
    "colspan",
    "rowspan",
    "src",
    "alt",
    "style",
  ],
  ALLOW_ARIA_ATTR: false,
  ALLOW_DATA_ATTR: true,
  FORBID_TAGS: [
    "base",
    "button",
    "embed",
    "form",
    "iframe",
    "link",
    "math",
    "meta",
    "object",
    "script",
    "select",
    "style",
    "svg",
    "textarea",
  ],
} satisfies Config;

const BLOCK_TAGS = new Set([
  "aside",
  "blockquote",
  "details",
  "div",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "li",
  "p",
  "pre",
]);

function cloneAnnotations(
  annotations: NotesRichTextAnnotations,
): NotesRichTextAnnotations {
  return { ...annotations };
}

function annotationsEqual(
  left: NotesRichTextAnnotations,
  right: NotesRichTextAnnotations,
): boolean {
  return left.bold === right.bold
    && left.italic === right.italic
    && left.strikethrough === right.strikethrough
    && left.underline === right.underline
    && left.code === right.code
    && left.color === right.color;
}

function linksEqual(
  left: NotesRichTextLink | null,
  right: NotesRichTextLink | null,
): boolean {
  return (left?.url ?? null) === (right?.url ?? null);
}

function cloneRichTextItem(item: NotesRichText): NotesRichText {
  return cloneData(item);
}

function appendRichTextItem(output: NotesRichText[], item: NotesRichText): void {
  if (!item.plain_text) return;
  const previous = output.at(-1);
  if (
    item.type === "text"
    && previous?.type === "text"
    && annotationsEqual(previous.annotations, item.annotations)
    && linksEqual(previous.text.link, item.text.link)
    && previous.href === item.href
  ) {
    previous.text.content += item.text.content;
    previous.plain_text += item.plain_text;
    return;
  }
  output.push(cloneRichTextItem(item));
}

function appendText(
  output: NotesRichText[],
  content: string,
  context: NotesRichHtmlInlineContext,
): void {
  if (!content) return;
  const reference = context.reference;
  const item: NotesRichText = !reference
    ? context.linkUrl ? createLinkedTextRichText(content, context.linkUrl) : createTextRichText(content)
    : reference.type === "mention" ? { ...cloneData(reference.item), plain_text: content, href: null }
      : reference.type === "page" ? createPageMentionRichText(reference.id, content, reference.href)
        : createDatabaseMentionRichText(reference.id, content);
  if (reference && reference.type !== "mention") {
    item.plain_text = content;
    item.href = reference.href;
  }
  item.annotations = cloneAnnotations(context.annotations);
  appendRichTextItem(output, item);
}

/** Keep one reference's line breaks in the same run without merging separate mention elements. */
function appendInlineReferenceRuns(output: NotesRichText[], items: readonly NotesRichText[]): void {
  let previous: NotesRichText | undefined;
  for (const item of items) {
    if (item.type === "mention" && previous?.type === "mention"
      && item.href === previous.href && annotationsEqual(item.annotations, previous.annotations)
      && JSON.stringify(item.mention) === JSON.stringify(previous.mention)) {
      previous.plain_text += item.plain_text;
      continue;
    }
    output.push(item);
    previous = item;
  }
}

function normalizeHtmlText(text: string, preformatted: boolean): string {
  if (preformatted) return text.replace(/\r\n?/gu, "\n");
  return text.replace(/[\t\n\f\r ]+/gu, " ");
}

function normalizedTagName(element: Element): string {
  return element.tagName.toLowerCase();
}

function booleanAttribute(element: Element, name: string): boolean | null {
  const value = element.getAttribute(name);
  if (value === null) return null;
  return value === "" || value === "true";
}

function notesColorFromAttribute(value: string | null): NotesColor | null {
  const candidate = value?.trim();
  if (!candidate) return null;
  return NOTES_COLORS.some((color) => color === candidate)
    ? candidate as NotesColor
    : null;
}

function applyStyleAnnotations(
  element: Element,
  annotations: NotesRichTextAnnotations,
): void {
  if (!(element instanceof HTMLElement)) return;
  const weight = element.style.fontWeight.trim().toLocaleLowerCase();
  const numericWeight = Number.parseInt(weight, 10);
  if (weight === "bold" || weight === "bolder" || numericWeight >= 600) {
    annotations.bold = true;
  }
  const fontStyle = element.style.fontStyle.trim().toLocaleLowerCase();
  if (fontStyle === "italic" || fontStyle === "oblique") annotations.italic = true;
  const decoration = [
    element.style.textDecoration,
    element.style.textDecorationLine,
  ].join(" ").toLocaleLowerCase();
  if (decoration.includes("underline")) annotations.underline = true;
  if (decoration.includes("line-through")) annotations.strikethrough = true;
}

/** Validate a pasted mention identity with the same reader used for stored rich text; invalid data pastes as text. */
function pastedObjectMention(value: string): NotesExternalMentionRichText | null {
  if (value.length > NOTES_CLIPBOARD_MENTION_MAX_LENGTH) return null;
  let item: NotesRichText | undefined;
  try {
    [item] = parseNotesRichTextArray([{
      type: "mention",
      mention: JSON.parse(value),
      annotations: defaultRichTextAnnotations(),
      plain_text: "",
      href: null,
    }], "pasted mention");
  } catch {
    return null;
  }
  return isNotesExternalMentionRichText(item) ? item : null;
}

/** Treat reference metadata as untrusted input; retain ordinary page hyperlinks when no markers exist. */
function inlineReferenceForElement(
  element: Element,
  linkUrl: string | null,
  inherited: NotesRichHtmlInlineReference | null | undefined,
): NotesRichHtmlInlineReference | null {
  const mention = element.getAttribute(NOTES_CLIPBOARD_MENTION_ATTRIBUTE);
  if (mention !== null) {
    const item = pastedObjectMention(mention);
    return item ? { type: "mention", item } : null;
  }
  const type = element.getAttribute("data-notes-reference-type");
  const id = element.getAttribute("data-notes-reference-id");
  const target = linkUrl?.startsWith("#notes?") ? parseNotesLinkHash(linkUrl) : null;
  if (type !== null || id !== null) {
    if ((type !== "page" && type !== "database") || !isNotesUuid(id)) return null;
    return { type, id, href: type === "page" ? `#notes?page=${id}`
      : target ? target.blockId === id ? linkUrl : null : linkUrl };
  }
  if (normalizedTagName(element) === "a") {
    return target && !target.blockId ? { type: "page", id: target.pageId, href: linkUrl } : null;
  }
  return inherited ?? null;
}

function contextForElement(
  element: Element,
  context: NotesRichHtmlInlineContext,
): NotesRichHtmlInlineContext {
  const tagName = normalizedTagName(element);
  const annotations = cloneAnnotations(context.annotations);
  if (tagName === "b" || tagName === "strong") annotations.bold = true;
  if (tagName === "i" || tagName === "em") annotations.italic = true;
  if (tagName === "u") annotations.underline = true;
  if (["s", "strike", "del"].includes(tagName)) annotations.strikethrough = true;
  if (tagName === "code") annotations.code = true;
  if (tagName === "pre") annotations.code = true;
  applyStyleAnnotations(element, annotations);
  if (booleanAttribute(element, "data-notes-bold") === true) annotations.bold = true;
  if (booleanAttribute(element, "data-notes-italic") === true) annotations.italic = true;
  if (booleanAttribute(element, "data-notes-underline") === true) annotations.underline = true;
  if (booleanAttribute(element, "data-notes-strikethrough") === true) {
    annotations.strikethrough = true;
  }
  if (booleanAttribute(element, "data-notes-code") === true) annotations.code = true;
  const color = notesColorFromAttribute(element.getAttribute("data-notes-rich-text-color"));
  if (color) annotations.color = color;

  const rawLink = element.getAttribute("data-notes-link-url")
    ?? (tagName === "a" ? element.getAttribute("href") : null);
  const linkUrl = rawLink === null
    ? context.linkUrl
    : notesClipboardLinkUrl(rawLink);
  return {
    annotations,
    linkUrl,
    reference: inlineReferenceForElement(element, linkUrl, context.reference),
    preformatted: context.preformatted || tagName === "pre"
      || (element instanceof HTMLElement && ["pre", "pre-wrap", "break-spaces"].includes(element.style.whiteSpace)),
  };
}

function blockTypeForElement(
  element: Element,
  listType: "bulleted_list_item" | "numbered_list_item" | null,
): NotesRichHtmlPasteBlockType {
  switch (normalizedTagName(element)) {
    case "h1":
      return "heading_1";
    case "h2":
      return "heading_2";
    case "h3":
      return "heading_3";
    case "h4":
      return "heading_4";
    case "h5":
      return "heading_5";
    case "h6":
      return "heading_6";
    case "hr":
      return "divider";
    case "li":
      if (Array.from(element.querySelectorAll('input[type="checkbox"]')).some((input) => input.closest("li") === element)) return "to_do";
      return listType ?? "bulleted_list_item";
    case "blockquote":
      return "quote";
    case "pre":
      return "code";
    default:
      return "paragraph";
  }
}

function hasDirectBlockChildren(element: Element): boolean {
  return Array.from(element.children).some((child) => {
    const tagName = normalizedTagName(child);
    return BLOCK_TAGS.has(tagName) || ["ol", "ul", "table"].includes(tagName);
  });
}

function collectInline(
  node: Node,
  context: NotesRichHtmlInlineContext,
  output: NotesRichText[],
): void {
  if (node.nodeType === Node.TEXT_NODE) {
    appendText(output, normalizeHtmlText(node.textContent ?? "", context.preformatted), context);
    return;
  }
  if (!(node instanceof Element)) return;
  const tagName = normalizedTagName(node);
  if (tagName === "br") {
    appendText(output, "\n", context);
    return;
  }
  if (tagName === "ul" || tagName === "ol") return;
  if (tagName === "table") {
    const text = Array.from(node.querySelectorAll("tr")).map((row) => Array.from(row.children)
      .map((cell) => cell.textContent ?? "").join("\t")).join("\n");
    appendText(output, text, context);
    return;
  }
  if (tagName === "img") {
    const source = node.getAttribute("src") ?? "";
    const label = node.getAttribute("alt") ?? "";
    appendText(output, [label, source].filter(Boolean).join(" (") + (label && source ? ")" : ""), context);
    return;
  }
  const separatesLines = tagName === "p" || tagName === "div";
  if (separatesLines && output.length && !richTextPlainText(output).endsWith("\n")) {
    appendText(output, "\n", context);
  }
  const childContext = contextForElement(node, context);
  const referenceItems: NotesRichText[] | null = childContext.reference && childContext.reference !== context.reference ? [] : null;
  for (const child of node.childNodes) {
    collectInline(child, childContext, referenceItems ?? output);
  }
  if (referenceItems) appendInlineReferenceRuns(output, referenceItems);
  if (tagName === "a") {
    const href = node.getAttribute("href");
    const markedReference = node.hasAttribute("data-notes-reference-type") || node.hasAttribute("data-notes-reference-id");
    if (href && !markedReference && !href.startsWith("#notes?") && !notesClipboardLinkUrl(href) && !/^[a-z][a-z0-9+.-]*:/iu.test(href)) {
      appendText(output, ` (${href})`, context);
    }
  }
  if (separatesLines) appendText(output, "\n", context);
}

function segmentFromElement(
  element: Element,
  context: NotesRichHtmlInlineContext,
  listType: "bulleted_list_item" | "numbered_list_item" | null,
): NotesRichHtmlPasteSegment {
  const tagName = normalizedTagName(element);
  if (tagName === "pre") {
    const content = normalizeHtmlText(element.textContent ?? "", true);
    return {
      type: "code",
      richText: content ? [createTextRichText(content)] : [],
      language: element.querySelector("code")?.className.match(/(?:^|\s)language-(\S+)/u)?.[1] ?? "plain text",
    };
  }
  const richText: NotesRichText[] = [];
  const childContext = contextForElement(element, context);
  const hasParagraphChildren = Array.from(element.children).some((child) => ["P", "DIV"].includes(child.tagName));
  for (const child of element.childNodes) {
    if (hasParagraphChildren && child.nodeType === Node.TEXT_NODE && !child.textContent?.trim()) continue;
    collectInline(child, childContext, richText);
  }
  const text = richTextPlainText(richText);
  const trimmed = hasParagraphChildren && text.endsWith("\n") ? richTextRangeSlice(richText, 0, text.length - 1) : richText;
  return {
    type: blockTypeForElement(element, listType),
    checked: Array.from(element.querySelectorAll('input[type="checkbox"]')).some((input) => input.closest("li") === element && input.hasAttribute("checked")),
    richText: tagName === "li"
      ? richTextRangeSlice(richText, text.length - text.trimStart().length, text.trimEnd().length)
      : trimmed,
  };
}

/** Read a foldable Obsidian callout as a toggle with ordinary child blocks. */
function foldableCalloutSegments(
  element: Element,
  context: NotesRichHtmlInlineContext,
  depth: number,
): NotesRichHtmlPasteSegment[] | null {
  const titleNode = Array.from(element.children).find((child) => normalizedTagName(child) === "p");
  if (!titleNode) return null;
  const richText: NotesRichText[] = [];
  const titleContext = { ...contextForElement(titleNode, context), preformatted: true };
  for (const child of titleNode.childNodes) collectInline(child, titleContext, richText);
  const text = richTextPlainText(richText);
  const lineEnd = text.indexOf("\n");
  const titleLine = lineEnd < 0 ? text : text.slice(0, lineEnd);
  const marker = /^\[![A-Za-z][A-Za-z0-9_-]*\]([+-])(?:[ \t]+|$)/u.exec(titleLine);
  if (!marker) return null;
  const segments: NotesRichHtmlPasteSegment[] = [{
    type: "toggle",
    richText: richTextRangeSlice(richText, marker[0].length, titleLine.length),
    open: marker[1] === "+",
    depth,
  }];
  if (lineEnd >= 0) {
    const firstBody = richTextRangeSlice(richText, lineEnd + 1, text.length);
    if (richTextPlainText(firstBody).trim()) {
      segments.push({ type: "paragraph", richText: firstBody, depth: depth + 1 });
    }
  }
  const body = document.createDocumentFragment();
  for (const child of element.childNodes) {
    if (child !== titleNode) body.append(child.cloneNode(true));
  }
  return [...segments, ...collectSegments(body, context, null, depth + 1)];
}

function collectSegments(
  parent: ParentNode,
  context: NotesRichHtmlInlineContext,
  listType: "bulleted_list_item" | "numbered_list_item" | null,
  depth = 0,
): NotesRichHtmlPasteSegment[] {
  const segments: NotesRichHtmlPasteSegment[] = [];
  const inlineRichText: NotesRichText[] = [];
  const flushInline = () => {
    if (richTextPlainText(inlineRichText).trim().length === 0) {
      inlineRichText.length = 0;
      return;
    }
    segments.push({ type: "paragraph", depth, richText: [...inlineRichText] });
    inlineRichText.length = 0;
  };

  for (const child of parent.childNodes) {
    if (!(child instanceof Element)) {
      collectInline(child, context, inlineRichText);
      continue;
    }
    const tagName = normalizedTagName(child);
    const sourcePageId = child.getAttribute("data-notes-child-page-id");
    const sourceDatabaseId = child.getAttribute("data-notes-child-database-id");
    if (tagName === "p" && isNotesUuid(sourceDatabaseId)) {
      flushInline();
      segments.push({ type: "child_database", richText: [createTextRichText(child.textContent ?? "")], sourceDatabaseId, depth });
      continue;
    }
    if (tagName === "p" && isNotesUuid(sourcePageId)) {
      flushInline();
      segments.push({ type: "child_page", richText: [createTextRichText(child.textContent ?? "")], sourcePageId, depth });
      continue;
    }
    if (tagName === "aside") {
      flushInline();
      const directChildren = Array.from(child.children);
      const explicitMarker = directChildren.find((element) => element.hasAttribute("data-notes-callout-marker"));
      const firstChild = directChildren[0];
      const implicitMarker = !explicitMarker && firstChild && normalizedTagName(firstChild) === "p"
        && isSingleEmoji(firstChild.textContent ?? "") ? firstChild : null;
      const marker = explicitMarker ?? implicitMarker;
      const label = directChildren.find((element) => element.hasAttribute("data-notes-callout-label"));
      const labelRichText: NotesRichText[] = [];
      if (label) {
        const labelContext = contextForElement(label, context);
        for (const node of label.childNodes) collectInline(node, labelContext, labelRichText);
      }
      const icon = calloutIconFromElement(child, marker);
      const rawColor = child.getAttribute("data-notes-callout-color");
      const color = rawColor && NOTES_COLORS.includes(rawColor as NotesColor)
        ? rawColor as NotesColor : "gray_background";
      segments.push({ type: "callout", richText: labelRichText, icon, color, depth });
      const body = document.createDocumentFragment();
      for (const node of child.childNodes) {
        if (node !== marker && node !== label) body.append(node.cloneNode(true));
      }
      segments.push(...collectSegments(body, contextForElement(child, context), null, depth + 1));
      continue;
    }
    if (tagName === "details") {
      const summary = Array.from(child.children).find((element) => normalizedTagName(element) === "summary");
      if (summary) {
        flushInline();
        const richText: NotesRichText[] = [];
        const summaryContext = contextForElement(summary, context);
        for (const node of summary.childNodes) collectInline(node, summaryContext, richText);
        segments.push({
          type: "toggle", richText,
          open: child.getAttribute("data-notes-toggle-open") === "false" ? false : child.hasAttribute("open"),
          depth,
        });
        const body = document.createDocumentFragment();
        for (const node of child.childNodes) {
          if (node !== summary) body.append(node.cloneNode(true));
        }
        segments.push(...collectSegments(body, contextForElement(child, context), null, depth + 1));
        continue;
      }
    }
    if (tagName === "blockquote") {
      const foldable = foldableCalloutSegments(child, context, depth);
      if (foldable) {
        flushInline();
        segments.push(...foldable);
        continue;
      }
    }
    if (tagName === "table") {
      flushInline();
      const table = readNotesClipboardTable(child, (cell) => {
        const runs: NotesRichText[] = [];
        const cellContext = contextForElement(cell, context);
        for (const node of cell.childNodes) collectInline(node, cellContext, runs);
        return runs;
      });
      if (table) {
        segments.push({ type: "table", richText: [], table, depth });
        for (const cells of table.rows) segments.push({ type: "table_row", cells, depth: depth + 1,
          richText: [createTextRichText(cells.map(richTextPlainText).join("\t"))] });
      } else {
        const text = Array.from(child.querySelectorAll("tr")).map((row) => Array.from(row.children)
          .map((cell) => cell.textContent ?? "").join("\t")).join("\n");
        segments.push({ type: "paragraph", richText: [createTextRichText(text)], depth });
      }
      continue;
    }
    if (tagName === "ol" || tagName === "ul") {
      flushInline();
      const childListType = tagName === "ol" ? "numbered_list_item" : "bulleted_list_item";
      segments.push(...collectSegments(child, contextForElement(child, context), childListType, depth));
      continue;
    }
    if (tagName === "div" && hasDirectBlockChildren(child)) {
      flushInline();
      segments.push(...collectSegments(child, contextForElement(child, context), listType, depth));
      continue;
    }
    if (BLOCK_TAGS.has(tagName)) {
      flushInline();
      const directParagraphs = tagName === "li"
        ? Array.from(child.children).filter((element) => normalizedTagName(element) === "p") : [];
      const firstParagraph = directParagraphs[0];
      const firstParagraphIndex = firstParagraph ? Array.from(child.childNodes).indexOf(firstParagraph) : -1;
      const hasLeadingContent = firstParagraphIndex > 0 && Array.from(child.childNodes)
        .slice(0, firstParagraphIndex).some((node) => !!node.textContent?.trim());
      if (firstParagraph && directParagraphs.length > 1 && !hasLeadingContent
        && child.hasAttribute(NOTES_MARKDOWN_LIST_PARAGRAPHS_ATTRIBUTE)) {
        const title = segmentFromElement(firstParagraph, context, null);
        const list = segmentFromElement(child, context, listType);
        segments.push({ ...title, type: list.type, checked: list.checked, depth });
        for (const paragraph of directParagraphs.slice(1)) {
          const body = segmentFromElement(paragraph, context, null);
          const text = richTextPlainText(body.richText);
          segments.push({ ...body,
            richText: richTextRangeSlice(body.richText, text.length - text.trimStart().length, text.trimEnd().length),
            depth: depth + 1,
          });
        }
      } else {
        segments.push({ ...segmentFromElement(child, context, listType), depth });
      }
      if (tagName !== "pre") {
        // Nested list containers are handled by their nearest list ancestor.
        for (const list of child.querySelectorAll("ul, ol")) {
          const ancestorList = list.parentElement?.closest("ul, ol");
          if (ancestorList && child.contains(ancestorList)) continue;
          segments.push(...collectSegments(list, contextForElement(child, context),
            normalizedTagName(list) === "ol" ? "numbered_list_item" : "bulleted_list_item", depth + 1));
        }
      }
      continue;
    }
    collectInline(child, context, inlineRichText);
  }
  flushInline();
  return segments;
}

function isSingleEmoji(value: string): boolean {
  const text = value.trim();
  return /\p{Extended_Pictographic}/u.test(text)
    && [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text)].length === 1;
}

function calloutIconFromElement(element: Element, marker: Element | null): NotesIcon | null {
  const metadata = element.getAttribute("data-notes-callout-icon-json");
  if (metadata) {
    try {
      return parseNullableNotesIcon(JSON.parse(metadata) as unknown, "callout clipboard icon");
    } catch (error) {
      console.warn("Ignoring invalid Notes callout clipboard icon", error);
    }
  }
  const emoji = marker?.textContent?.trim();
  return emoji && isSingleEmoji(emoji) ? { type: "emoji", emoji } : createCalloutPayload("").icon;
}

/** Sanitize clipboard markup before reading semantic content or reconciling formats. */
export function sanitizeNotesRichHtml(html: string): DocumentFragment | null {
  if (!html.trim() || html.length > NOTES_RICH_HTML_MAX_LENGTH) return null;
  if (typeof document === "undefined") return null;
  const sanitized = DOMPurify.sanitize(html, SANITIZER_CONFIG);
  const template = document.createElement("template");
  template.innerHTML = sanitized;
  return template.content;
}

function plainTextLength(segments: readonly NotesRichHtmlPasteSegment[]): number {
  if (segments.length === 0) return 0;
  return segments
    .map((segment) => richTextPlainText(segment.richText))
    .join("\n")
    .length;
}

function parseNotesRichHtmlPaste(html: string): NotesRichHtmlPasteSegment[] | null {
  const fragment = sanitizeNotesRichHtml(html);
  if (!fragment) return null;
  const context: NotesRichHtmlInlineContext = {
    annotations: defaultRichTextAnnotations(),
    linkUrl: null,
    preformatted: false,
  };
  const segments = collectSegments(fragment, context, null);
  if (segments.length === 0 || plainTextLength(segments) > NOTES_CLIPBOARD_MAX_TEXT_LENGTH) {
    return null;
  }
  return capSegments(segments);
}

function capSegments(
  segments: readonly NotesRichHtmlPasteSegment[],
): NotesRichHtmlPasteSegment[] {
  if (segments.length <= NOTES_CLIPBOARD_MAX_BLOCKS) return [...segments];
  if (segments.some((segment) => segment.type === "table")) {
    const flattened: NotesRichHtmlPasteSegment[] = [];
    for (let index = 0; index < segments.length; index += 1) {
      const segment = segments[index];
      if (segment.type !== "table") { flattened.push(segment); continue; }
      const rows: string[] = [];
      while (segments[index + 1]?.type === "table_row") {
        rows.push(richTextPlainText(segments[++index].richText));
      }
      flattened.push({ type: "paragraph", depth: segment.depth, richText: [createTextRichText(rows.join("\n"))] });
    }
    return capSegments(flattened);
  }
  const kept = segments.slice(0, NOTES_CLIPBOARD_MAX_BLOCKS - 1);
  const overflow = segments
    .slice(NOTES_CLIPBOARD_MAX_BLOCKS - 1)
    .map((segment) => richTextPlainText(segment.richText))
    .join("\n");
  return [
    ...kept,
    {
      type: "paragraph",
      richText: overflow ? [createTextRichText(overflow)] : [],
    },
  ];
}

function richTextOrEmptyText(richText: readonly NotesRichText[]): NotesRichText[] {
  return richText.length > 0 ? [...richText] : [createTextRichText("")];
}

function createUpdateForSegment(segment: NotesRichHtmlPasteSegment): NotesBlockUpdate {
  switch (segment.type) {
    case "child_database":
      return { type: "child_database", child_database: { title: richTextPlainText(segment.richText) } };
    case "child_page":
      return { type: "child_page", child_page: { title: richTextPlainText(segment.richText) } };
    case "table": {
      if (!segment.table) throw new Error("Clipboard table has no cell structure");
      return { type: "table", table: { table_width: segment.table.width,
        has_column_header: segment.table.hasColumnHeader, has_row_header: segment.table.hasRowHeader } };
    }
    case "table_row":
      return { type: "table_row", table_row: { cells: segment.cells ?? [] } };
    case "divider":
      return { type: "divider", divider: {} };
    case "to_do":
      return { type: "to_do", to_do: { ...createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)), checked: segment.checked ?? false } };
    case "toggle":
      return { type: "toggle", toggle: {
        ...createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
        ganbaru_open: segment.open ?? true,
      } };
    case "callout":
      return { type: "callout", callout: {
        ...createTextPayloadFromRichText(richTextOrEmptyText(segment.richText), segment.color ?? "gray_background"),
        icon: segment.icon === undefined ? createCalloutPayload("").icon : segment.icon,
      } };
    case "heading_1":
      return {
        type: "heading_1",
        heading_1: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "heading_2":
      return {
        type: "heading_2",
        heading_2: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "heading_3":
      return {
        type: "heading_3",
        heading_3: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "heading_4":
      return {
        type: "heading_4",
        heading_4: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "heading_5":
      return {
        type: "heading_5",
        heading_5: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "heading_6":
      return {
        type: "heading_6",
        heading_6: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "bulleted_list_item":
      return {
        type: "bulleted_list_item",
        bulleted_list_item: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "numbered_list_item":
      return {
        type: "numbered_list_item",
        numbered_list_item: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "quote":
      return {
        type: "quote",
        quote: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
    case "code":
      return {
        type: "code",
        code: createCodePayload(richTextPlainText(segment.richText), segment.language),
      };
    case "paragraph":
      return {
        type: "paragraph",
        paragraph: createTextPayloadFromRichText(richTextOrEmptyText(segment.richText)),
      };
  }
}

function createWriteForSegment(
  id: string,
  segment: NotesRichHtmlPasteSegment,
): NotesBlockWrite {
  const update = createUpdateForSegment(segment);
  return { id, ...update };
}

function segmentWithSuffix(
  segment: NotesRichHtmlPasteSegment,
  suffix: readonly NotesRichText[],
): NotesRichHtmlPasteSegment {
  if (suffix.length === 0) return segment;
  if (segment.type === "code") {
    return {
      ...segment,
      richText: [createTextRichText(`${richTextPlainText(segment.richText)}${richTextPlainText(suffix)}`)],
    };
  }
  const end = richTextPlainText(segment.richText).length;
  return {
    ...segment,
    richText: replaceRichTextRange(segment.richText, end, end, suffix),
  };
}

/** Keep paste focus out of children hidden by a closed toggle. */
function lastVisibleSegmentIndex(segments: readonly NotesRichHtmlPasteSegment[]): number {
  let lastVisible = 0;
  let closedDepth: number | null = null;
  for (const [index, segment] of segments.entries()) {
    const depth = segment.depth ?? 0;
    if (closedDepth !== null && depth > closedDepth) continue;
    closedDepth = segment.type === "toggle" && segment.open === false ? depth : null;
    lastVisible = index;
  }
  return lastVisible;
}

export function planNotesRichHtmlPaste(
  input: NotesRichHtmlPastePlanInput,
): NotesRichHtmlPastePlan | null {
  if (input.currentBlock.type === "code") return null;
  const segments = parseNotesRichHtmlPaste(input.html);
  if (!segments) return null;

  const currentRichText = blockEditableRichText(input.currentBlock);
  const currentPlainText = richTextPlainText(currentRichText);
  const start = Math.max(0, Math.min(input.selectionStart, input.selectionEnd));
  const end = Math.min(
    currentPlainText.length,
    Math.max(input.selectionStart, input.selectionEnd),
  );
  const prefix = richTextRangeSlice(currentRichText, 0, start);
  const suffix = richTextRangeSlice(currentRichText, end, currentPlainText.length);
  if (["child_page", "child_database"].includes(segments[0].type) || (["table", "divider", "toggle", "callout"].includes(segments[0].type)
    && (richTextPlainText(prefix).length > 0
      || (input.currentBlock.type !== "paragraph" && end !== currentPlainText.length)))) {
    segments.unshift({ type: "paragraph", richText: [], depth: 0 });
  }
  if (segments.at(-1)?.type === "table_row" || segments.at(-1)?.type === "table"
    || ["child_page", "child_database"].includes(segments.at(-1)?.type ?? "")
    || (segments.at(-1)?.type === "divider" && richTextPlainText(suffix))
    || (segments.some((segment) => segment.type === "toggle" || segment.type === "callout")
      && richTextPlainText(suffix)
      && (["toggle", "callout"].includes(segments.at(-1)?.type ?? "") || (segments.at(-1)?.depth ?? 0) > 0))) {
    segments.push({ type: "paragraph", richText: [], depth: 0 });
  }
  const [firstSegment, ...remainingSegments] = segments;
  if (!firstSegment) return null;

  const shouldConvertCurrentBlock = (input.currentBlock.type === "paragraph"
    || (start === 0 && end === currentPlainText.length && firstSegment.type !== "paragraph"))
    && richTextPlainText(prefix).length === 0;
  const currentSegment: NotesRichHtmlPasteSegment = {
    ...firstSegment,
    richText: shouldConvertCurrentBlock
      ? firstSegment.richText
      : replaceRichTextRange(currentRichText, start, currentPlainText.length, firstSegment.richText),
  };
  if (remainingSegments.length === 0) {
    const singleSegment = segmentWithSuffix(currentSegment, suffix);
    return {
      currentUpdate: shouldConvertCurrentBlock
        ? createUpdateForSegment(singleSegment)
        : blockWithRichText(
            input.currentBlock,
            replaceRichTextRange(currentRichText, start, end, firstSegment.richText),
          ),
      appendedBlocks: [],
      blockDepths: [0],
      focusBlockId: input.currentBlock.id,
      focusOffset: shouldConvertCurrentBlock
        ? richTextPlainText(firstSegment.richText).length
        : start + richTextPlainText(firstSegment.richText).length,
    };
  }

  const lastSegment = remainingSegments.at(-1);
  const appendedSegments = lastSegment
    ? [
        ...remainingSegments.slice(0, -1),
        segmentWithSuffix(lastSegment, suffix),
      ]
    : [];
  const appendedBlocks = appendedSegments.map((segment) => {
    const id = input.createId();
    return createWriteForSegment(id, segment);
  });
  const copiedPageIds = Object.fromEntries(appendedSegments.flatMap((segment, index) =>
    segment.sourcePageId ? [[appendedBlocks[index].id, segment.sourcePageId]] : []));
  const copiedDatabaseIds = Object.fromEntries(appendedSegments.flatMap((segment, index) =>
    segment.sourceDatabaseId ? [[appendedBlocks[index].id, segment.sourceDatabaseId]] : []));
  const visibleIndex = lastVisibleSegmentIndex(segments);
  const visibleSegment = segments[visibleIndex];
  return {
    currentUpdate: shouldConvertCurrentBlock
      ? createUpdateForSegment(currentSegment)
      : blockWithRichText(input.currentBlock, currentSegment.richText),
    appendedBlocks,
    ...(Object.keys(copiedPageIds).length ? { copiedPageIds } : {}),
    ...(Object.keys(copiedDatabaseIds).length ? { copiedDatabaseIds } : {}),
    blockDepths: segments.map((segment) => segment.depth ?? 0),
    focusBlockId: visibleIndex === 0 ? input.currentBlock.id : appendedBlocks[visibleIndex - 1].id,
    focusOffset: visibleIndex === segments.length - 1 && lastSegment
      ? richTextPlainText(lastSegment.richText).length
      : richTextPlainText(visibleSegment.richText).length,
  };
}

/** Flatten portable blocks into an inline field while retaining supported text annotations. */
export function notesInlineClipboardRichText(html: string): NotesRichText[] | null {
  const segments = parseNotesRichHtmlPaste(html);
  if (!segments) return null;
  const output: NotesRichText[] = [];
  for (const segment of segments) {
    if (segment.type === "table") continue;
    if (output.length) appendRichTextItem(output, createTextRichText("\n"));
    for (const item of segment.richText) appendRichTextItem(output, item);
  }
  return output;
}
