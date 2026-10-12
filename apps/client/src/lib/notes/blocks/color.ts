import type {
  NotesBlock,
  NotesBlockType,
  NotesBlockUpdate,
  NotesCalloutBlockPayload,
  NotesColor,
  NotesTextBlockPayload,
  NotesToggleBlockPayload,
  NotesTodoBlockPayload,
} from "$lib/notes/types";

export const NOTES_TEXT_COLORS = [
  "default",
  "gray",
  "brown",
  "orange",
  "yellow",
  "green",
  "blue",
  "purple",
  "pink",
  "red",
] as const satisfies readonly NotesColor[];

export const NOTES_BACKGROUND_COLORS = [
  "gray_background",
  "brown_background",
  "orange_background",
  "yellow_background",
  "green_background",
  "blue_background",
  "purple_background",
  "pink_background",
  "red_background",
] as const satisfies readonly NotesColor[];

export interface NotesColorTokens {
  text: string;
  background: string;
  border: string;
  swatchText: string;
  swatchBackground: string;
}

const DEFAULT_TOKENS: NotesColorTokens = {
  text: "var(--foreground)",
  background: "transparent",
  border: "transparent",
  swatchText: "var(--foreground)",
  swatchBackground: "var(--background)",
};

const COLOR_TOKENS: Record<NotesColor, NotesColorTokens> = {
  default: DEFAULT_TOKENS,
  gray: textTokens("#787774"),
  brown: textTokens("#9f6b53"),
  orange: textTokens("#d9730d"),
  yellow: textTokens("#cb912f"),
  green: textTokens("#448361"),
  blue: textTokens("#337ea9"),
  purple: textTokens("#9065b0"),
  pink: textTokens("#c14c8a"),
  red: textTokens("#d44c47"),
  gray_background: backgroundTokens("#787774"),
  brown_background: backgroundTokens("#9f6b53"),
  orange_background: backgroundTokens("#d9730d"),
  yellow_background: backgroundTokens("#cb912f"),
  green_background: backgroundTokens("#448361"),
  blue_background: backgroundTokens("#337ea9"),
  purple_background: backgroundTokens("#9065b0"),
  pink_background: backgroundTokens("#c14c8a"),
  red_background: backgroundTokens("#d44c47"),
};

function textTokens(color: string): NotesColorTokens {
  return {
    ...DEFAULT_TOKENS,
    text: color,
    swatchText: color,
    swatchBackground: "var(--background)",
  };
}

function backgroundTokens(color: string): NotesColorTokens {
  return {
    ...DEFAULT_TOKENS,
    background: colorWithAlpha(color, 0.16),
    border: colorWithAlpha(color, 0.24),
    swatchText: "var(--foreground)",
    swatchBackground: colorWithAlpha(color, 0.22),
  };
}

function colorWithAlpha(hex: string, alpha: number): string {
  const clamped = Math.round(Math.min(1, Math.max(0, alpha)) * 255);
  return `${hex}${clamped.toString(16).padStart(2, "0")}`;
}

export function canBlockHaveColor(type: NotesBlockType): boolean {
  return [
    "paragraph",
    "heading_1",
    "heading_2",
    "heading_3",
    "heading_4",
    "heading_5",
    "heading_6",
    "bulleted_list_item",
    "numbered_list_item",
    "to_do",
    "toggle",
    "callout",
    "table_of_contents",
    "quote",
  ].includes(type);
}

export function blockColor(block: NotesBlock): NotesColor {
  switch (block.type) {
    case "paragraph":
      return block.paragraph.color ?? "default";
    case "heading_1":
      return block.heading_1.color ?? "default";
    case "heading_2":
      return block.heading_2.color ?? "default";
    case "heading_3":
      return block.heading_3.color ?? "default";
    case "heading_4":
      return block.heading_4.color ?? "default";
    case "heading_5":
      return block.heading_5.color ?? "default";
    case "heading_6":
      return block.heading_6.color ?? "default";
    case "bulleted_list_item":
      return block.bulleted_list_item.color ?? "default";
    case "numbered_list_item":
      return block.numbered_list_item.color ?? "default";
    case "to_do":
      return block.to_do.color ?? "default";
    case "toggle":
      return block.toggle.color ?? "default";
    case "callout":
      return block.callout.color ?? "default";
    case "quote":
      return block.quote.color ?? "default";
    case "table_of_contents":
      return block.table_of_contents.color ?? "default";
    case "column_list":
    case "column":
    case "table":
    case "table_row":
    case "tab":
    case "image":
    case "video":
    case "audio":
    case "file":
    case "pdf":
    case "child_page":
    case "child_database":
    case "bookmark":
    case "link_preview":
    case "synced_block":
    case "template":
    case "button":
    case "embed":
    case "equation":
    case "breadcrumb":
    case "divider":
    case "code":
    case "unsupported":
      return "default";
  }
}

export function blockWithColor(block: NotesBlock, color: NotesColor): NotesBlockUpdate {
  switch (block.type) {
    case "paragraph":
      return { type: block.type, paragraph: textPayloadWithColor(block.paragraph, color) };
    case "heading_1":
      return { type: block.type, heading_1: textPayloadWithColor(block.heading_1, color) };
    case "heading_2":
      return { type: block.type, heading_2: textPayloadWithColor(block.heading_2, color) };
    case "heading_3":
      return { type: block.type, heading_3: textPayloadWithColor(block.heading_3, color) };
    case "heading_4":
      return { type: block.type, heading_4: textPayloadWithColor(block.heading_4, color) };
    case "heading_5":
      return { type: block.type, heading_5: textPayloadWithColor(block.heading_5, color) };
    case "heading_6":
      return { type: block.type, heading_6: textPayloadWithColor(block.heading_6, color) };
    case "bulleted_list_item":
      return {
        type: block.type,
        bulleted_list_item: textPayloadWithColor(block.bulleted_list_item, color),
      };
    case "numbered_list_item":
      return {
        type: block.type,
        numbered_list_item: textPayloadWithColor(block.numbered_list_item, color),
      };
    case "to_do":
      return { type: block.type, to_do: todoPayloadWithColor(block.to_do, color) };
    case "toggle":
      return { type: block.type, toggle: togglePayloadWithColor(block.toggle, color) };
    case "callout":
      return { type: block.type, callout: calloutPayloadWithColor(block.callout, color) };
    case "quote":
      return { type: block.type, quote: textPayloadWithColor(block.quote, color) };
    case "child_page":
      return { type: block.type, child_page: block.child_page };
    case "child_database":
      return { type: block.type, child_database: block.child_database };
    case "breadcrumb":
      return { type: block.type, breadcrumb: block.breadcrumb };
    case "table_of_contents":
      return {
        type: block.type,
        table_of_contents: { ...block.table_of_contents, color },
      };
    case "column_list":
      return { type: block.type, column_list: block.column_list };
    case "column":
      return { type: block.type, column: block.column };
    case "table":
      return { type: block.type, table: block.table };
    case "table_row":
      return { type: block.type, table_row: block.table_row };
    case "tab":
      return { type: block.type, tab: block.tab };
    case "image":
      return { type: block.type, image: block.image };
    case "video":
      return { type: block.type, video: block.video };
    case "audio":
      return { type: block.type, audio: block.audio };
    case "file":
      return { type: block.type, file: block.file };
    case "pdf":
      return { type: block.type, pdf: block.pdf };
    case "bookmark":
      return { type: block.type, bookmark: block.bookmark };
    case "link_preview":
      return { type: block.type, link_preview: block.link_preview };
    case "synced_block":
      return { type: block.type, synced_block: block.synced_block };
    case "template":
      return { type: block.type, template: block.template };
    case "button":
      return { type: block.type, button: block.button };
    case "embed":
      return { type: block.type, embed: block.embed };
    case "equation":
      return { type: block.type, equation: block.equation };
    case "divider":
      return { type: block.type, divider: block.divider };
    case "code":
      return { type: block.type, code: block.code };
    case "unsupported":
      return { type: block.type, unsupported: block.unsupported };
  }
}

export function notesBlockColorStyle(color: NotesColor): string {
  const tokens = COLOR_TOKENS[color];
  return [
    `--notes-block-color: ${tokens.text}`,
    `--notes-block-bg: ${tokens.background}`,
    `--notes-block-border: ${tokens.border}`,
  ].join("; ");
}

/** Resolve a callout's shared surface color without tinting its individual text rows. */
export function notesCalloutBackground(color: NotesColor): string {
  if (color === "default") return "var(--background)";
  if (color === "gray_background") return "color-mix(in srgb, var(--foreground) 4%, var(--background))";
  return color.endsWith("_background") ? COLOR_TOKENS[color].background : "var(--background)";
}

export function notesBlockColorSwatchStyle(color: NotesColor): string {
  const tokens = COLOR_TOKENS[color];
  return [
    `--notes-color-swatch-fg: ${tokens.swatchText}`,
    `--notes-color-swatch-bg: ${tokens.swatchBackground}`,
    `--notes-color-swatch-border: ${tokens.border === "transparent" ? "var(--border)" : tokens.border}`,
  ].join("; ");
}

export function notesRichTextColorStyle(color: NotesColor): string {
  if (color === "default") return "";
  const tokens = COLOR_TOKENS[color];
  return [
    `--notes-rich-text-color: ${tokens.text}`,
    `--notes-rich-text-bg: ${tokens.background}`,
    `--notes-rich-text-border: ${tokens.border}`,
  ].join("; ");
}

function textPayloadWithColor(
  payload: NotesTextBlockPayload,
  color: NotesColor,
): NotesTextBlockPayload {
  return { ...payload, color };
}

function todoPayloadWithColor(
  payload: NotesTodoBlockPayload,
  color: NotesColor,
): NotesTodoBlockPayload {
  return { ...payload, color };
}

function togglePayloadWithColor(
  payload: NotesToggleBlockPayload,
  color: NotesColor,
): NotesToggleBlockPayload {
  return { ...payload, color };
}

function calloutPayloadWithColor(
  payload: NotesCalloutBlockPayload,
  color: NotesColor,
): NotesCalloutBlockPayload {
  return { ...payload, color };
}

/** Resolve explicit Notes colors to portable CSS for external clipboard consumers. */
export function notesClipboardColorStyle(color: NotesColor): string {
  if (color === "default") return "";
  const tokens = COLOR_TOKENS[color];
  return color.endsWith("_background")
    ? `background-color: ${tokens.background}`
    : `color: ${tokens.text}`;
}
