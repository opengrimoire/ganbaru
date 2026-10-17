import { Temporal } from "@js-temporal/polyfill";
import type {
  NotesDatabaseMentionRichText,
  NotesDateMentionRichText,
  NotesDateMentionValue,
  NotesEquationRichText,
  NotesLocalObjectMentionRichText,
  NotesLocalObjectMentionType,
  NotesPageMentionRichText,
  NotesColor,
  NotesRichText,
  NotesRichTextAnnotations,
  NotesRichTextLink,
  NotesTextRichText,
  NotesUserMentionRichText,
} from "$lib/notes/types";
import { cloneNotesJson } from "$lib/notes/json-clone";
import { parseNotesLinkHash } from "$lib/notes/links/block-link";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import { normalizeNotesDateQuery, parseNotesTypedCalendarDate, splitNotesTypedTime } from "./date-query";

const DEFAULT_RICH_TEXT_ANNOTATIONS: NotesRichTextAnnotations = {
  bold: false,
  italic: false,
  strikethrough: false,
  underline: false,
  code: false,
  color: "default",
};

const MENTION_QUERY_LIMIT = 80;
const LINK_SCHEME_PATTERN = /^[a-z][a-z0-9+.-]*:/iu;
const EMAIL_ADDRESS_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/u;

export interface NotesPageMentionTarget {
  kind: "page";
  id: string;
  title: string;
  subtitle?: string;
  iconText?: string | null;
  /** Where the page lives, outermost first, as raw titles that may be empty. */
  location?: readonly string[];
}

export interface NotesUserMentionTarget {
  kind: "user";
  id: string;
  title: string;
  subtitle?: string;
  iconText?: string | null;
}

export interface NotesDatabaseMentionTarget {
  kind: "database";
  id: string;
  title: string;
  subtitle?: string;
  iconText?: string | null;
  /** Where the database lives, outermost first, as raw titles that may be empty. */
  location?: readonly string[];
}

export interface NotesLocalObjectMentionTarget {
  kind: NotesLocalObjectMentionType;
  id: string;
  title: string;
  subtitle?: string;
  iconText?: string | null;
}

export interface NotesDateMentionTarget {
  kind: "date";
  id: string;
  title: string;
  subtitle: string;
  iconText?: string | null;
  date: NotesDateMentionValue;
  reminder: boolean;
}

export type NotesObjectMentionTarget =
  | NotesUserMentionTarget
  | NotesDatabaseMentionTarget
  | NotesLocalObjectMentionTarget;

export type NotesNamedMentionTarget = NotesPageMentionTarget | NotesObjectMentionTarget;

export type NotesMentionTarget = NotesNamedMentionTarget | NotesDateMentionTarget;

export type NotesObjectMentionRichText =
  | NotesUserMentionRichText
  | NotesDatabaseMentionRichText
  | NotesLocalObjectMentionRichText;

export interface NotesDateMentionLabels {
  today: string;
  tomorrow: string;
  yesterday: string;
  nextWeek: string;
  date: string;
  reminder: string;
  remindTitle: (dateLabel: string) => string;
  /** A day label followed by a typed wall time, for example "Tomorrow, 3:00 PM". */
  atTime: (dateLabel: string, time: string) => string;
}

export interface NotesMentionQuery {
  start: number;
  end: number;
  query: string;
}

export interface NotesRichTextLinkRange {
  start: number;
  end: number;
  url: string | null;
}

export type NotesRichTextAnnotationName =
  | "bold"
  | "italic"
  | "strikethrough"
  | "underline"
  | "code";

export type NotesRichTextAnnotationPatch =
  Partial<Pick<NotesRichTextAnnotations, NotesRichTextAnnotationName | "color">>;

export interface NotesRichTextAnnotationRange {
  start: number;
  end: number;
  annotations: NotesRichTextAnnotations;
}

export type NotesInlineEquationConversionError =
  | "selection_required"
  | "invalid_expression";

export type NotesInlineEquationConversionPlan =
  | {
    type: "convert";
    start: number;
    end: number;
    expression: string;
    cursor: number;
  }
  | {
    type: "error";
    reason: NotesInlineEquationConversionError;
    start: number;
    end: number;
  };

interface DateMentionCandidate {
  id: string;
  aliases: readonly string[];
  date: Temporal.PlainDate;
  title: string;
}

const WEEKDAY_ALIASES: readonly {
  dayOfWeek: number;
  aliases: readonly string[];
}[] = [
  { dayOfWeek: 1, aliases: ["monday", "mon", "lunes", "lun"] },
  { dayOfWeek: 2, aliases: ["tuesday", "tue", "martes", "mar"] },
  { dayOfWeek: 3, aliases: ["wednesday", "wed", "miercoles", "miércoles", "mie", "mié"] },
  { dayOfWeek: 4, aliases: ["thursday", "thu", "jueves", "jue"] },
  { dayOfWeek: 5, aliases: ["friday", "fri", "viernes", "vie"] },
  { dayOfWeek: 6, aliases: ["saturday", "sat", "sabado", "sábado", "sab", "sáb"] },
  { dayOfWeek: 7, aliases: ["sunday", "sun", "domingo", "dom"] },
];

export function createTextRichText(content: string): NotesTextRichText {
  return {
    type: "text",
    text: {
      content,
      link: null,
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: content,
    href: null,
  };
}

export function createLinkedTextRichText(
  content: string,
  url: string,
): NotesTextRichText {
  return {
    type: "text",
    text: {
      content,
      link: { url },
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: content,
    href: url,
  };
}

export function createPageMentionRichText(
  pageId: string,
  title: string,
  href: string | null = null,
): NotesPageMentionRichText {
  const plainText = title.trim() || pageId;
  return {
    type: "mention",
    mention: {
      type: "page",
      page: {
        id: pageId,
      },
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: plainText,
    href,
  };
}

export function createUserMentionRichText(
  userId: string,
  title: string,
): NotesUserMentionRichText {
  const plainText = title.trim() || userId;
  return {
    type: "mention",
    mention: {
      type: "user",
      user: {
        object: "user",
        id: userId,
      },
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: plainText,
    href: null,
  };
}

export function createDatabaseMentionRichText(
  databaseId: string,
  title: string,
): NotesDatabaseMentionRichText {
  const plainText = title.trim() || databaseId;
  return {
    type: "mention",
    mention: {
      type: "database",
      database: {
        id: databaseId,
      },
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: plainText,
    href: null,
  };
}

export function createLocalObjectMentionRichText(
  objectType: NotesLocalObjectMentionType,
  objectId: string,
  title: string,
): NotesLocalObjectMentionRichText {
  const plainText = title.trim() || objectId;
  return {
    type: "mention",
    mention: {
      type: "ganbaru_object",
      ganbaru_object: {
        type: objectType,
        id: objectId,
      },
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: plainText,
    href: null,
  };
}

export function createObjectMentionRichText(
  target: NotesObjectMentionTarget,
): NotesObjectMentionRichText {
  if (target.kind === "user") return createUserMentionRichText(target.id, target.title);
  if (target.kind === "database") return createDatabaseMentionRichText(target.id, target.title);
  return createLocalObjectMentionRichText(target.kind, target.id, target.title);
}

export function createDateMentionValue(
  start: string,
  reminder = false,
): NotesDateMentionValue {
  return {
    start,
    end: null,
    time_zone: null,
    ...(reminder ? { ganbaru_reminder: { enabled: true } } : {}),
  };
}

export function createDateMentionRichText(
  date: NotesDateMentionValue,
  title: string,
): NotesDateMentionRichText {
  return {
    type: "mention",
    mention: {
      type: "date",
      date,
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: title.trim() || date.start,
    href: null,
  };
}

export function normalizeRichTextEquationExpression(expression: string): string | null {
  const trimmed = expression.trim();
  if (!trimmed) return null;
  if (trimmed.length > 2048 || /[\u0000-\u001f]/u.test(trimmed)) return null;
  return trimmed;
}

export function planRichTextEquationConversion(
  text: string,
  selectionStart: number,
  selectionEnd: number,
): NotesInlineEquationConversionPlan {
  const rawStart = Math.min(selectionStart, selectionEnd);
  const rawEnd = Math.max(selectionStart, selectionEnd);
  const start = Math.max(0, Math.min(rawStart, text.length));
  const end = Math.max(start, Math.min(rawEnd, text.length));
  if (start === end) {
    return { type: "error", reason: "selection_required", start, end };
  }
  const expression = normalizeRichTextEquationExpression(text.slice(start, end));
  if (!expression) {
    return { type: "error", reason: "invalid_expression", start, end };
  }
  return {
    type: "convert",
    start,
    end,
    expression,
    cursor: start + expression.length,
  };
}

export function createEquationRichText(expression: string): NotesEquationRichText {
  const normalizedExpression = normalizeRichTextEquationExpression(expression) ?? "e=mc^2";
  return {
    type: "equation",
    equation: {
      expression: normalizedExpression,
    },
    annotations: { ...DEFAULT_RICH_TEXT_ANNOTATIONS },
    plain_text: normalizedExpression,
    href: null,
  };
}

export function richTextPlainText(richText: readonly NotesRichText[]): string {
  return richText.map((item) => item.plain_text).join("");
}

/** Mentions of people, dates, and app objects, which point outside the Notes page graph. */
export type NotesExternalMentionRichText = NotesUserMentionRichText | NotesLocalObjectMentionRichText | NotesDateMentionRichText;

export function isNotesExternalMentionRichText(item: NotesRichText | undefined): item is NotesExternalMentionRichText {
  return item?.type === "mention"
    && (item.mention.type === "user" || item.mention.type === "ganbaru_object" || item.mention.type === "date");
}

export function isNotesDateMentionRichText(item: NotesRichText | undefined): item is NotesDateMentionRichText {
  return item?.type === "mention" && item.mention.type === "date";
}

function cloneRichText(item: NotesRichText): NotesRichText {
  return cloneNotesJson(item);
}

function richTextLinksEqual(
  left: NotesRichTextLink | null,
  right: NotesRichTextLink | null,
): boolean {
  return (left?.url ?? null) === (right?.url ?? null);
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

export function defaultRichTextAnnotations(): NotesRichTextAnnotations {
  return { ...DEFAULT_RICH_TEXT_ANNOTATIONS };
}

function cloneTextRichTextWithContent(
  item: NotesTextRichText,
  content: string,
): NotesTextRichText {
  const clone = cloneNotesJson(item);
  return {
    ...clone,
    text: {
      ...clone.text,
      content,
    },
    plain_text: content,
  };
}

function appendTextItem(items: NotesRichText[], item: NotesTextRichText): void {
  if (!item.plain_text) return;
  const previous = items.at(-1);
  if (
    previous?.type === "text"
    && annotationsEqual(previous.annotations, item.annotations)
    && richTextLinksEqual(previous.text.link, item.text.link)
    && previous.href === item.href
  ) {
    previous.text.content += item.text.content;
    previous.plain_text += item.plain_text;
    return;
  }
  items.push(cloneRichText(item));
}

function appendText(items: NotesRichText[], content: string): void {
  if (!content) return;
  appendTextItem(items, createTextRichText(content));
}

function appendRichTextItem(items: NotesRichText[], item: NotesRichText): void {
  if (item.type === "text") {
    appendTextItem(items, item);
    return;
  }
  if (!item.plain_text) return;
  items.push(cloneRichText(item));
}

function isRichTextLabelWordCharacter(character: string | undefined): boolean {
  return character ? /[\p{Letter}\p{Number}_]/u.test(character) : false;
}

function hasRichTextLabelBoundary(
  text: string,
  label: string,
  start: number,
): boolean {
  const previous = text.at(start - 1);
  const next = text.at(start + label.length);
  return !(
    isRichTextLabelWordCharacter(label.at(0))
    && isRichTextLabelWordCharacter(previous)
  ) && !(
    isRichTextLabelWordCharacter(label.at(-1))
    && isRichTextLabelWordCharacter(next)
  );
}

function commonPrefixLength(left: string, right: string): number {
  const maxLength = Math.min(left.length, right.length);
  let length = 0;
  while (length < maxLength && left.at(length) === right.at(length)) {
    length += 1;
  }
  return length;
}

function commonSuffixLength(left: string, right: string): number {
  const maxLength = Math.min(left.length, right.length);
  let length = 0;
  while (
    length < maxLength
    && left.at(left.length - 1 - length) === right.at(right.length - 1 - length)
  ) {
    length += 1;
  }
  return length;
}

function fallbackPreservedRichTextStart(
  sourcePlainText: string,
  nextPlainText: string,
  sourceStart: number,
  sourceEnd: number,
  nextCursor: number,
  label: string,
): number | null {
  let bestStart: number | null = null;
  let bestScore = -1;
  let searchStart = nextCursor;
  while (searchStart <= nextPlainText.length) {
    const candidateStart = nextPlainText.indexOf(label, searchStart);
    if (candidateStart === -1) break;
    searchStart = candidateStart + 1;
    if (!hasRichTextLabelBoundary(nextPlainText, label, candidateStart)) continue;
    const candidateEnd = candidateStart + label.length;
    const beforeScore = commonSuffixLength(
      sourcePlainText.slice(0, sourceStart),
      nextPlainText.slice(0, candidateStart),
    );
    const afterScore = commonPrefixLength(
      sourcePlainText.slice(sourceEnd),
      nextPlainText.slice(candidateEnd),
    );
    const score = beforeScore + afterScore;
    if (
      score > bestScore
      || (
        score === bestScore
        && bestStart !== null
        && Math.abs(candidateStart - sourceStart) < Math.abs(bestStart - sourceStart)
      )
    ) {
      bestStart = candidateStart;
      bestScore = score;
    }
  }
  return bestStart;
}

function appendRichTextSlice(
  output: NotesRichText[],
  item: NotesRichText,
  startOffset: number,
  endOffset: number,
): void {
  const itemText = item.plain_text;
  if (startOffset >= endOffset) return;
  if (startOffset === 0 && endOffset === itemText.length) {
    output.push(cloneRichText(item));
    return;
  }
  if (item.type === "text") {
    appendTextItem(output, cloneTextRichTextWithContent(item, itemText.slice(startOffset, endOffset)));
  }
}

function richTextSlice(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
): NotesRichText[] {
  const output: NotesRichText[] = [];
  let cursor = 0;
  for (const item of richText) {
    const itemText = item.plain_text;
    const itemStart = cursor;
    const itemEnd = itemStart + itemText.length;
    cursor = itemEnd;
    if (end <= itemStart) break;
    if (start >= itemEnd) continue;
    appendRichTextSlice(
      output,
      item,
      Math.max(0, start - itemStart),
      Math.min(itemText.length, end - itemStart),
    );
  }
  return output;
}

export function richTextRangeSlice(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, end, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(Math.max(start, end), plainText.length));
  return richTextSlice(richText, safeStart, safeEnd);
}

export function replaceRichTextRange(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  replacement: readonly NotesRichText[],
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, end, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(Math.max(start, end), plainText.length));
  const output: NotesRichText[] = [];
  for (const item of richTextSlice(richText, 0, safeStart)) {
    appendRichTextItem(output, item);
  }
  for (const item of replacement) {
    appendRichTextItem(output, item);
  }
  for (const item of richTextSlice(richText, safeEnd, plainText.length)) {
    appendRichTextItem(output, item);
  }
  return output.length > 0 ? output : [createTextRichText("")];
}

export function insertPageMentionRichText(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  pageId: string,
  title: string,
  href: string | null = null,
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, plainText.length));
  return [
    ...richTextSlice(richText, 0, safeStart),
    createPageMentionRichText(pageId, title, href),
    ...richTextSlice(richText, safeEnd, plainText.length),
  ];
}

export function insertDateMentionRichText(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  date: NotesDateMentionValue,
  title: string,
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, plainText.length));
  return [
    ...richTextSlice(richText, 0, safeStart),
    createDateMentionRichText(date, title),
    ...richTextSlice(richText, safeEnd, plainText.length),
  ];
}

export function insertObjectMentionRichText(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  target: NotesObjectMentionTarget,
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, plainText.length));
  return [
    ...richTextSlice(richText, 0, safeStart),
    createObjectMentionRichText(target),
    ...richTextSlice(richText, safeEnd, plainText.length),
  ];
}

export function insertEquationRichText(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  expression: string,
): NotesRichText[] {
  const normalizedExpression = normalizeRichTextEquationExpression(expression);
  if (!normalizedExpression) return richText.map(cloneRichText);
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, plainText.length));
  if (safeStart === safeEnd) return richText.map(cloneRichText);
  return [
    ...richTextSlice(richText, 0, safeStart),
    createEquationRichText(normalizedExpression),
    ...richTextSlice(richText, safeEnd, plainText.length),
  ];
}

/** Normalize supported web, email, and validated local Notes destinations. */
export function normalizeRichTextLinkUrl(rawUrl: string): string | null {
  const trimmed = rawUrl.trim();
  if (!trimmed) return null;
  if (trimmed.length > 2048 || /[\u0000-\u001f]/u.test(trimmed)) return null;
  if (trimmed.startsWith("#")) return parseNotesLinkHash(trimmed) ? trimmed : null;
  const hasScheme = LINK_SCHEME_PATTERN.test(trimmed);
  if (!hasScheme && EMAIL_ADDRESS_PATTERN.test(trimmed)) return `mailto:${trimmed}`;
  if (!hasScheme && trimmed.includes("@")) return null;
  const candidate = hasScheme ? trimmed : `https://${trimmed}`;
  try {
    const parsed = new URL(candidate);
    if (parsed.protocol === "mailto:") {
      const address = parsed.pathname.trim();
      if (!EMAIL_ADDRESS_PATTERN.test(address)) return null;
      return parsed.toString();
    }
    if (!["http:", "https:"].includes(parsed.protocol)) return null;
    return parsed.toString();
  } catch {
    return null;
  }
}

/** Recognize a single copied URL without treating ordinary replacement text as a host name. */
export function notesPastedLinkUrl(text: string): string | null {
  const candidate = text.trim();
  if (/\s/u.test(candidate)) return null;
  if (!/^(?:https?:|mailto:|#notes\?)/iu.test(candidate)
    && !EMAIL_ADDRESS_PATTERN.test(candidate)
    && !/^[^\s/:?#]+\.[^\s/:?#]+(?:[/:?#].*)?$/u.test(candidate)) return null;
  return normalizeRichTextLinkUrl(candidate);
}

/** Edit a link label while preserving surrounding text and the selected text's shared formatting. */
export function editRichTextLink(
  richText: readonly NotesRichText[], start: number, end: number, title: string, url: string,
): NotesRichText[] {
  const selected = richTextRangeSlice(richText, start, end);
  const replacement = createTextRichText(title);
  replacement.annotations = { ...richTextAnnotationsForSelection(selected, 0, richTextPlainText(selected).length).annotations };
  return applyRichTextLink(replaceRichTextRange(richText, start, end, [replacement]), start, start + title.length, url);
}

function itemLinkUrl(item: NotesRichText): string | null {
  return item.type === "text" ? item.text.link?.url ?? null : null;
}

function itemAnnotations(item: NotesRichText): NotesRichTextAnnotations {
  return item.annotations;
}

function annotationsForRange(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
): NotesRichTextAnnotations {
  let cursor = 0;
  let commonAnnotations: NotesRichTextAnnotations | null = null;
  for (const item of richText) {
    const itemStart = cursor;
    const itemEnd = itemStart + item.plain_text.length;
    cursor = itemEnd;
    if (end <= itemStart) break;
    if (start >= itemEnd || item.type !== "text") continue;
    if (!commonAnnotations) {
      commonAnnotations = { ...itemAnnotations(item) };
      continue;
    }
    const annotations = itemAnnotations(item);
    commonAnnotations = {
      bold: commonAnnotations.bold && annotations.bold,
      italic: commonAnnotations.italic && annotations.italic,
      strikethrough: commonAnnotations.strikethrough && annotations.strikethrough,
      underline: commonAnnotations.underline && annotations.underline,
      code: commonAnnotations.code && annotations.code,
      color: commonAnnotations.color === annotations.color ? commonAnnotations.color : "default",
    };
  }
  return commonAnnotations ?? defaultRichTextAnnotations();
}

function annotationsAtCursor(
  richText: readonly NotesRichText[],
  cursorPosition: number,
): NotesRichTextAnnotations {
  let cursor = 0;
  let previousTextAnnotations: NotesRichTextAnnotations | null = null;
  for (const item of richText) {
    const itemStart = cursor;
    const itemEnd = itemStart + item.plain_text.length;
    cursor = itemEnd;
    if (item.type !== "text") continue;
    if (cursorPosition >= itemStart && cursorPosition < itemEnd) {
      return { ...itemAnnotations(item) };
    }
    if (cursorPosition === itemEnd) previousTextAnnotations = itemAnnotations(item);
    if (cursorPosition < itemEnd) break;
  }
  return previousTextAnnotations ? { ...previousTextAnnotations } : defaultRichTextAnnotations();
}

function commonLinkUrlInRange(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
): string | null {
  let cursor = 0;
  let commonUrl: string | null | undefined;
  for (const item of richText) {
    const itemStart = cursor;
    const itemEnd = itemStart + item.plain_text.length;
    cursor = itemEnd;
    if (end <= itemStart) break;
    if (start >= itemEnd || item.type !== "text") continue;
    const url = itemLinkUrl(item);
    if (!url) continue;
    if (commonUrl === undefined) {
      commonUrl = url;
      continue;
    }
    if (commonUrl !== url) return null;
  }
  return commonUrl ?? null;
}

export function richTextLinkRangeForSelection(
  richText: readonly NotesRichText[],
  selectionStart: number,
  selectionEnd: number,
): NotesRichTextLinkRange {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(selectionStart, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(selectionEnd, plainText.length));
  if (safeStart !== safeEnd) {
    return {
      start: safeStart,
      end: safeEnd,
      url: commonLinkUrlInRange(richText, safeStart, safeEnd),
    };
  }

  let rangeCursor = 0;
  const ranges = richText.map((item) => {
    const start = rangeCursor;
    const end = start + item.plain_text.length;
    rangeCursor = end;
    return { start, end, url: itemLinkUrl(item) };
  });
  const rangeIndex = ranges.findIndex(
    (range) =>
      range.url !== null
      && (
        (safeStart >= range.start && safeStart < range.end)
        || (safeStart === range.end && range.end > range.start)
      ),
  );
  if (rangeIndex !== -1) {
    const url = ranges[rangeIndex]?.url ?? null;
    let rangeStart = ranges[rangeIndex]?.start ?? safeStart;
    let rangeEnd = ranges[rangeIndex]?.end ?? safeStart;
    for (let index = rangeIndex - 1; index >= 0; index -= 1) {
      const range = ranges[index];
      if (!range || range.url !== url || range.end !== rangeStart) break;
      rangeStart = range.start;
    }
    for (let index = rangeIndex + 1; index < ranges.length; index += 1) {
      const range = ranges[index];
      if (!range || range.url !== url || range.start !== rangeEnd) break;
      rangeEnd = range.end;
    }
    return { start: rangeStart, end: rangeEnd, url };
  }

  return { start: safeStart, end: safeEnd, url: null };
}

export function richTextAnnotationsForSelection(
  richText: readonly NotesRichText[],
  selectionStart: number,
  selectionEnd: number,
): NotesRichTextAnnotationRange {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(selectionStart, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(selectionEnd, plainText.length));
  return {
    start: safeStart,
    end: safeEnd,
    annotations: safeStart === safeEnd
      ? annotationsAtCursor(richText, safeStart)
      : annotationsForRange(richText, safeStart, safeEnd),
  };
}

export function applyRichTextLink(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  url: string | null,
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, plainText.length));
  if (safeStart === safeEnd) return richText.map(cloneRichText);
  const normalizedUrl = url === null ? null : normalizeRichTextLinkUrl(url);
  if (url !== null && normalizedUrl === null) return richText.map(cloneRichText);
  const output: NotesRichText[] = [];
  let cursor = 0;
  for (const item of richText) {
    const itemText = item.plain_text;
    const itemStart = cursor;
    const itemEnd = itemStart + itemText.length;
    cursor = itemEnd;
    if (itemEnd <= safeStart || itemStart >= safeEnd) {
      output.push(cloneRichText(item));
      continue;
    }
    appendRichTextSlice(output, item, 0, Math.max(0, safeStart - itemStart));
    const middleStart = Math.max(0, safeStart - itemStart);
    const middleEnd = Math.min(itemText.length, safeEnd - itemStart);
    if (item.type === "text") {
      const nextText = cloneTextRichTextWithContent(
        item,
        itemText.slice(middleStart, middleEnd),
      );
      nextText.text.link = normalizedUrl ? { url: normalizedUrl } : null;
      nextText.href = normalizedUrl;
      appendTextItem(output, nextText);
    } else {
      appendRichTextSlice(output, item, middleStart, middleEnd);
    }
    appendRichTextSlice(output, item, Math.min(itemText.length, safeEnd - itemStart), itemText.length);
  }
  return output.length > 0 ? output : [createTextRichText("")];
}

export function applyRichTextAnnotations(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
  patch: NotesRichTextAnnotationPatch,
): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const safeStart = Math.max(0, Math.min(start, plainText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, plainText.length));
  if (safeStart === safeEnd) return richText.map(cloneRichText);
  const output: NotesRichText[] = [];
  let cursor = 0;
  for (const item of richText) {
    const itemText = item.plain_text;
    const itemStart = cursor;
    const itemEnd = itemStart + itemText.length;
    cursor = itemEnd;
    if (itemEnd <= safeStart || itemStart >= safeEnd) {
      output.push(cloneRichText(item));
      continue;
    }
    appendRichTextSlice(output, item, 0, Math.max(0, safeStart - itemStart));
    const middleStart = Math.max(0, safeStart - itemStart);
    const middleEnd = Math.min(itemText.length, safeEnd - itemStart);
    if (item.type === "text") {
      const nextText = cloneTextRichTextWithContent(
        item,
        itemText.slice(middleStart, middleEnd),
      );
      nextText.annotations = {
        ...nextText.annotations,
        ...patch,
      };
      appendTextItem(output, nextText);
    } else {
      appendRichTextSlice(output, item, middleStart, middleEnd);
    }
    appendRichTextSlice(
      output,
      item,
      Math.min(itemText.length, safeEnd - itemStart),
      itemText.length,
    );
  }
  return output.length > 0 ? output : [createTextRichText("")];
}

export function richTextHasVisibleFormatting(
  richText: readonly NotesRichText[],
): boolean {
  return richText.some((item) => {
    if (item.type !== "text") return true;
    return item.text.link !== null
      || item.href !== null
      || item.annotations.bold
      || item.annotations.italic
      || item.annotations.strikethrough
      || item.annotations.underline
      || item.annotations.code
      || item.annotations.color !== "default";
  });
}

export function richTextAnnotationTogglePatch(
  annotations: NotesRichTextAnnotations,
  name: NotesRichTextAnnotationName,
): NotesRichTextAnnotationPatch {
  switch (name) {
    case "bold":
      return { bold: !annotations.bold };
    case "italic":
      return { italic: !annotations.italic };
    case "strikethrough":
      return { strikethrough: !annotations.strikethrough };
    case "underline":
      return { underline: !annotations.underline };
    case "code":
      return { code: !annotations.code };
  }
}

export function richTextColorPatch(color: NotesColor): NotesRichTextAnnotationPatch {
  return { color };
}

export function replacePlainTextPreservingRichText(
  richText: readonly NotesRichText[],
  nextPlainText: string,
): NotesRichText[] {
  const preservableItems: {
    item: NotesRichText;
    start: number;
    end: number;
  }[] = [];
  let sourceCursor = 0;
  for (const item of richText) {
    const start = sourceCursor;
    const end = start + item.plain_text.length;
    sourceCursor = end;
    const preservesRichObject = item.type !== "text"
      || item.text.link !== null
      || item.href !== null
      || !annotationsEqual(item.annotations, DEFAULT_RICH_TEXT_ANNOTATIONS);
    if (preservesRichObject && item.plain_text) {
      preservableItems.push({ item, start, end });
    }
  }
  if (preservableItems.length === 0) return [createTextRichText(nextPlainText)];

  const sourcePlainText = richTextPlainText(richText);
  const sharedPrefixLength = commonPrefixLength(sourcePlainText, nextPlainText);
  let sharedSuffixLength = 0;
  const maxSharedSuffixLength = Math.min(
    sourcePlainText.length - sharedPrefixLength,
    nextPlainText.length - sharedPrefixLength,
  );
  while (
    sharedSuffixLength < maxSharedSuffixLength
    && sourcePlainText.at(sourcePlainText.length - 1 - sharedSuffixLength)
      === nextPlainText.at(nextPlainText.length - 1 - sharedSuffixLength)
  ) {
    sharedSuffixLength += 1;
  }

  const sourceChangedStart = sharedPrefixLength;
  const sourceChangedEnd = sourcePlainText.length - sharedSuffixLength;
  const nextChangedEnd = nextPlainText.length - sharedSuffixLength;

  const output: NotesRichText[] = [];
  let nextCursor = 0;
  for (const preservableItem of preservableItems) {
    const { item, start, end } = preservableItem;
    let nextStart: number | null = null;
    if (end <= sourceChangedStart) {
      nextStart = start;
    } else if (start >= sourceChangedEnd) {
      nextStart = nextChangedEnd + (start - sourceChangedEnd);
    }
    if (
      nextStart === null
      || nextStart < nextCursor
      || nextPlainText.slice(nextStart, nextStart + item.plain_text.length) !== item.plain_text
    ) {
      nextStart = fallbackPreservedRichTextStart(
        sourcePlainText,
        nextPlainText,
        start,
        end,
        nextCursor,
        item.plain_text,
      );
    }
    if (nextStart === null || nextStart < nextCursor) continue;
    const nextEnd = nextStart + item.plain_text.length;
    if (nextPlainText.slice(nextStart, nextEnd) !== item.plain_text) continue;
    appendText(output, nextPlainText.slice(nextCursor, nextStart));
    appendRichTextItem(output, item);
    nextCursor = nextEnd;
  }
  appendText(output, nextPlainText.slice(nextCursor));
  return output.length > 0 ? output : [createTextRichText("")];
}

export function detectPageMentionQuery(
  text: string,
  selectionStart: number,
  selectionEnd: number,
): NotesMentionQuery | null {
  if (selectionStart !== selectionEnd) return null;
  const cursor = Math.max(0, Math.min(selectionStart, text.length));
  const lineStart = text.lastIndexOf("\n", cursor - 1) + 1;
  const atIndex = text.lastIndexOf("@", cursor - 1);
  if (atIndex < lineStart) return null;
  const before = atIndex === 0 ? "" : text.at(atIndex - 1) ?? "";
  if (before && !/[\s([{:;,]/u.test(before)) return null;
  const query = text.slice(atIndex + 1, cursor);
  if (query.length > MENTION_QUERY_LIMIT || query.includes("@")) return null;
  return { start: atIndex, end: cursor, query };
}

export function filterPageMentionTargets(
  targets: readonly NotesPageMentionTarget[],
  query: string,
  limit = 8,
): NotesPageMentionTarget[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const matches = normalizedQuery
    ? targets.filter((target) => target.title.toLocaleLowerCase().includes(normalizedQuery))
    : [...targets];
  return matches
    .sort((left, right) => {
      const leftTitle = left.title.toLocaleLowerCase();
      const rightTitle = right.title.toLocaleLowerCase();
      const leftStarts = normalizedQuery && leftTitle.startsWith(normalizedQuery);
      const rightStarts = normalizedQuery && rightTitle.startsWith(normalizedQuery);
      if (leftStarts !== rightStarts) return leftStarts ? -1 : 1;
      return left.title.localeCompare(right.title);
    })
    .slice(0, limit);
}

export function notesMentionTargetKey(target: NotesMentionTarget): string {
  return `${target.kind}:${target.id}`;
}

export function filterNotesMentionTargets(
  targets: readonly NotesNamedMentionTarget[],
  query: string,
  limit = 8,
): NotesNamedMentionTarget[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const matches = normalizedQuery
    ? targets.filter((target) => {
      const location = "location" in target ? (target.location ?? []).join(" ") : "";
      return `${target.title} ${target.subtitle ?? ""} ${location}`.toLocaleLowerCase().includes(normalizedQuery);
    })
    : [...targets];
  return matches
    .sort((left, right) => {
      const leftTitle = left.title.toLocaleLowerCase();
      const rightTitle = right.title.toLocaleLowerCase();
      const leftStarts = normalizedQuery && leftTitle.startsWith(normalizedQuery);
      const rightStarts = normalizedQuery && rightTitle.startsWith(normalizedQuery);
      if (leftStarts !== rightStarts) return leftStarts ? -1 : 1;
      if (left.kind !== right.kind) return left.kind.localeCompare(right.kind);
      return left.title.localeCompare(right.title);
    })
    .slice(0, limit);
}

function splitReminderQuery(query: string): { reminder: boolean; dateQuery: string } {
  const normalized = normalizeNotesDateQuery(query);
  for (const prefix of ["remind me", "remind", "reminder", "recordar", "recuerdame", "recuérdame"]) {
    if (normalized === prefix) return { reminder: true, dateQuery: "" };
    if (normalized.startsWith(`${prefix} `)) {
      return { reminder: true, dateQuery: normalized.slice(prefix.length + 1).trim() };
    }
  }
  return { reminder: false, dateQuery: normalized };
}

/** A calendar date as mention menus spell it, for example "Oct 11, 2026". */
export function formatPlainDate(date: Temporal.PlainDate, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(new Date(date.year, date.month - 1, date.day));
}

/**
 * The spelled-out date shown beside a date suggestion, or null when its title already spells that date or the
 * stored date cannot be parsed.
 */
export function notesDateMentionDetail(target: NotesDateMentionTarget, locale: string): string | null {
  try {
    const spelled = formatPlainDate(Temporal.PlainDate.from(target.date.start), locale);
    return target.title.includes(spelled) ? null : spelled;
  } catch {
    return null;
  }
}

function formatWeekday(date: Temporal.PlainDate, locale: string): string {
  return new Intl.DateTimeFormat(locale, { weekday: "long" }).format(
    new Date(date.year, date.month - 1, date.day),
  );
}

function nextWeekday(today: Temporal.PlainDate, dayOfWeek: number, forceNext: boolean): Temporal.PlainDate {
  const offset = (dayOfWeek - today.dayOfWeek + 7) % 7;
  return today.add({ days: offset === 0 && forceNext ? 7 : offset });
}

function baseDateMentionCandidates(
  today: Temporal.PlainDate,
  labels: NotesDateMentionLabels,
): DateMentionCandidate[] {
  return [
    {
      id: "today",
      aliases: ["today", "tod", "hoy"],
      date: today,
      title: labels.today,
    },
    {
      id: "tomorrow",
      aliases: ["tomorrow", "tom", "manana", "mañana"],
      date: today.add({ days: 1 }),
      title: labels.tomorrow,
    },
    {
      id: "yesterday",
      aliases: ["yesterday", "yest", "ayer"],
      date: today.subtract({ days: 1 }),
      title: labels.yesterday,
    },
    {
      id: "next-week",
      aliases: ["next week", "week", "proxima semana", "próxima semana", "semana"],
      date: today.add({ weeks: 1 }),
      title: labels.nextWeek,
    },
  ];
}

function weekdayDateMentionCandidates(
  query: string,
  today: Temporal.PlainDate,
  locale: string,
): DateMentionCandidate[] {
  const forceNext = query.startsWith("next ") || query.startsWith("proximo ") || query.startsWith("proxima ");
  const normalizedWeekdayQuery = forceNext ? query.replace(/^(next|proximo|proxima)\s+/u, "") : query;
  return WEEKDAY_ALIASES
    .filter((weekday) =>
      weekday.aliases.some((alias) =>
        alias.includes(normalizedWeekdayQuery) || normalizedWeekdayQuery.includes(alias)
      )
    )
    .map((weekday) => {
      const date = nextWeekday(today, weekday.dayOfWeek, forceNext);
      return {
        id: `${forceNext ? "next-" : ""}weekday-${weekday.dayOfWeek}`,
        aliases: weekday.aliases,
        date,
        title: formatWeekday(date, locale),
      };
    });
}

function dateMentionCandidateMatches(candidate: DateMentionCandidate, query: string): boolean {
  if (!query) return true;
  const title = normalizeNotesDateQuery(candidate.title);
  return title.includes(query)
    || candidate.date.toString().includes(query)
    || candidate.aliases.some((alias) => alias.includes(query) || query.includes(alias));
}

function deduplicateDateCandidates(
  candidates: readonly DateMentionCandidate[],
): DateMentionCandidate[] {
  const seen = new Set<string>();
  const output: DateMentionCandidate[] = [];
  for (const candidate of candidates) {
    const key = candidate.date.toString();
    if (seen.has(key)) continue;
    seen.add(key);
    output.push(candidate);
  }
  return output;
}

/** A wall time typed after a date, with its label in the user's time format. */
interface DateMentionTime {
  time: Temporal.PlainTime;
  label: string;
}

function formatMentionTime(time: Temporal.PlainTime, locale: string, timeFormat: CalendarTimeFormat | undefined): string {
  return new Intl.DateTimeFormat(locale, {
    hour: "numeric",
    minute: "2-digit",
    ...(timeFormat === undefined ? {} : { hour12: timeFormat === "12h" }),
  }).format(new Date(2000, 0, 1, time.hour, time.minute));
}

/**
 * Suggest dates for the text typed after the at sign. The text may start with a reminder word, name a relative day,
 * a weekday, or a calendar date ("2025-12-2", "oct 30", "30 de octubre"), and end with a wall time ("12:1", "3pm").
 * A typed time applies to every suggested day and is stored as a floating wall time.
 *
 * @param query The text typed after the at sign.
 * @param options The current day, the active locale, the localized labels, how many suggestions to keep, and the
 * user's time format for typed times.
 * @returns Date mention suggestions, the typed calendar date first.
 */
export function buildDateMentionTargets(
  query: string,
  options: {
    today: Temporal.PlainDate;
    locale: string;
    labels: NotesDateMentionLabels;
    limit?: number;
    timeFormat?: CalendarTimeFormat;
  },
): NotesDateMentionTarget[] {
  const { reminder, dateQuery: reminderlessQuery } = splitReminderQuery(query);
  const { dateQuery, time } = splitNotesTypedTime(reminderlessQuery);
  const typedDate = parseNotesTypedCalendarDate(dateQuery, { today: options.today, locale: options.locale });
  const typedCandidate: DateMentionCandidate[] = typedDate
    ? [{
        id: "typed-date",
        aliases: [typedDate.toString()],
        date: typedDate,
        title: formatPlainDate(typedDate, options.locale),
      }]
    : [];
  const baseCandidates = baseDateMentionCandidates(options.today, options.labels);
  const weekdayCandidates = dateQuery
    ? weekdayDateMentionCandidates(dateQuery, options.today, options.locale)
    : [];
  const candidates = deduplicateDateCandidates([
    ...typedCandidate,
    ...weekdayCandidates,
    ...baseCandidates.filter((candidate) => dateMentionCandidateMatches(candidate, dateQuery)),
  ]);
  const typedTime = time ? { time, label: formatMentionTime(time, options.locale, options.timeFormat) } : null;
  return candidates
    .slice(0, options.limit ?? 6)
    .map((candidate) => dateMentionTarget(candidate, reminder, options.labels, typedTime));
}

/**
 * Suggest the dates offered before anything is typed after the at sign: today, and a reminder for tomorrow.
 *
 * @param options The current day and the localized date labels.
 * @returns Today as a date mention followed by tomorrow as a reminder.
 */
export function buildIdleDateMentionTargets(options: {
  today: Temporal.PlainDate;
  labels: NotesDateMentionLabels;
}): NotesDateMentionTarget[] {
  const [today, tomorrow] = baseDateMentionCandidates(options.today, options.labels);
  return [
    dateMentionTarget(today, false, options.labels),
    dateMentionTarget(tomorrow, true, options.labels),
  ];
}

function dateMentionTarget(
  candidate: DateMentionCandidate,
  reminder: boolean,
  labels: NotesDateMentionLabels,
  time: DateMentionTime | null = null,
): NotesDateMentionTarget {
  const start = time
    ? `${candidate.date.toString()}T${time.time.toString({ smallestUnit: "minute" })}`
    : candidate.date.toString();
  const dateTitle = time ? labels.atTime(candidate.title, time.label) : candidate.title;
  return {
    kind: "date",
    id: `${reminder ? "reminder" : "date"}:${start}`,
    title: reminder ? labels.remindTitle(dateTitle) : dateTitle,
    subtitle: reminder ? labels.reminder : labels.date,
    iconText: null,
    date: createDateMentionValue(start, reminder),
    reminder,
  };
}
