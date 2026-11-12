import type { ChatMessageReference } from "$lib/chat/contracts";
import { chatReferenceTextSegments } from "$lib/chat/composer/message-references";
import { renderOrganizationalChatMarkdown, safeExternalUrl } from "./markdown";

/** Attribute naming the empty element where a reference component is mounted. */
export const ORGANIZATIONAL_REFERENCE_SLOT_ATTRIBUTE = "data-chat-reference-slot";

/** A validated message reference placed as an empty slot in rendered channel Markdown. */
export interface OrganizationalReferenceSlot {
  /** Value of the slot element's `data-chat-reference-slot` attribute. */
  index: number;
  reference: ChatMessageReference;
  /** The reference's text in the message, shown by the mounted component. */
  text: string;
}

/** Sanitized channel message HTML with the reference slots it contains. */
export interface RenderedOrganizationalMarkdown {
  html: string;
  slots: OrganizationalReferenceSlot[];
}

const SLOT_START = "\uE000";
const SLOT_END = "\uE001";
const SENTINEL_CHARACTER = /[\uE000\uE001]/gu;
const HAS_SENTINEL_CHARACTER = /[\uE000\uE001]/u;
const SENTINEL = /\uE000(\d+)\uE001/gu;
const STRAY_SENTINEL = /\uE000\d*\uE001?/gu;
const LITERAL_TEXT_CONTAINERS = "a, code, pre";

/**
 * Renders a channel message as sanitized Markdown with a slot for each reference.
 *
 * Each validated reference range is replaced by a private use sentinel before parsing; the
 * author's own sentinel characters are removed first, so a sentinel cannot be forged.
 * Other private use characters, such as icon font glyphs, stay as typed.
 * Sentinels in a link destination or title become the reference text before the destination
 * is validated. After sanitizing, sentinels in text become empty slot elements, or the
 * reference text inside links and code, and any leftover sentinel character is removed.
 * The stored text is never changed.
 *
 * @param text The message's normalized Markdown.
 * @param references Stored references as UTF-8 byte ranges of `text`.
 * @returns The HTML and the slots placed in it, in document order.
 */
export function renderOrganizationalMarkdown(
  text: string,
  references: readonly ChatMessageReference[],
): RenderedOrganizationalMarkdown {
  const candidates: OrganizationalReferenceSlot[] = [];
  let source = "";
  for (const segment of chatReferenceTextSegments(text, references)) {
    if (segment.kind === "text") {
      source += stripSentinelCharacters(segment.text);
      continue;
    }
    const index = candidates.length;
    candidates.push({ index, reference: segment.reference, text: stripSentinelCharacters(segment.text) });
    source += `${SLOT_START}${index}${SLOT_END}`;
  }
  const html = renderOrganizationalChatMarkdown(source, (value) => resolveSentinels(value, candidates));
  return placeReferenceSlots(html, candidates);
}

function placeReferenceSlots(html: string, candidates: readonly OrganizationalReferenceSlot[]): RenderedOrganizationalMarkdown {
  const template = document.createElement("template");
  template.innerHTML = html;
  const content = template.content;
  const textNodes: Text[] = [];
  const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node instanceof Text) textNodes.push(node);
  }
  const placed = new Set<number>();
  for (const node of textNodes) replaceSentinels(node, candidates, placed);
  for (const element of content.querySelectorAll("*")) stripAttributes(element);
  return {
    html: template.innerHTML,
    slots: candidates.filter((candidate) => placed.has(candidate.index)),
  };
}

function replaceSentinels(node: Text, candidates: readonly OrganizationalReferenceSlot[], placed: Set<number>): void {
  const value = node.data;
  if (!HAS_SENTINEL_CHARACTER.test(value)) return;
  const literal = (node.parentElement?.closest(LITERAL_TEXT_CONTAINERS) ?? null) !== null;
  const document = node.ownerDocument;
  const fragment = document.createDocumentFragment();
  let cursor = 0;
  for (const match of value.matchAll(SENTINEL)) {
    fragment.append(document.createTextNode(stripStraySentinels(value.slice(cursor, match.index))));
    cursor = match.index + match[0].length;
    const index = Number(match[1]);
    const candidate = candidates[index];
    if (!candidate) continue;
    if (literal || placed.has(index)) {
      fragment.append(document.createTextNode(candidate.text));
      continue;
    }
    const slot = document.createElement("span");
    slot.setAttribute(ORGANIZATIONAL_REFERENCE_SLOT_ATTRIBUTE, String(index));
    fragment.append(slot);
    placed.add(index);
  }
  fragment.append(document.createTextNode(stripStraySentinels(value.slice(cursor))));
  node.replaceWith(fragment);
}

/** Removes sentinel characters from attributes and drops a link whose destination is no longer safe. */
function stripAttributes(element: Element): void {
  for (const attribute of [...element.attributes]) {
    const value = stripStraySentinels(attribute.value);
    if (value !== attribute.value) element.setAttribute(attribute.name, value);
  }
  const href = element.getAttribute("href");
  if (element.localName === "a" && href !== null && !safeExternalUrl(href)) element.replaceWith(...element.childNodes);
}

function resolveSentinels(value: string, candidates: readonly OrganizationalReferenceSlot[]): string {
  return stripStraySentinels(value.replace(SENTINEL, (_, index: string) => candidates[Number(index)]?.text ?? ""));
}

/** Removes the two sentinel characters, including any the author typed. */
function stripSentinelCharacters(value: string): string {
  return value.replace(SENTINEL_CHARACTER, "");
}

/** Removes sentinel pieces the parser or the bounds split, then any lone sentinel character. */
function stripStraySentinels(value: string): string {
  return stripSentinelCharacters(value.replace(STRAY_SENTINEL, ""));
}
