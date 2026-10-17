import {
  NOTES_BACKGROUND_COLORS,
  NOTES_TEXT_COLORS,
} from "$lib/notes/blocks/color";
import {
  notesBlockInsertCommandKey,
  notesBlockInsertCommands,
  type NotesBlockInsertCommand,
  type NotesInsertableBlockType,
} from "$lib/notes/blocks/insertion";
import type { NotesHeadingBlockType } from "$lib/notes/blocks/factory";
import type { NotesColor } from "$lib/notes/types";

export type NotesSlashAction =
  | "copy_link"
  | "duplicate"
  | "move_up"
  | "move_down"
  | "delete";

export type NotesSlashCommand =
  | NotesBlockInsertCommand
  | { kind: "action"; action: NotesSlashAction }
  | { kind: "color"; color: NotesColor };
export type NotesSlashBlockCommand = NotesBlockInsertCommand;

export type NotesSlashCommandSection = "blocks" | "actions" | "colors";
export type NotesSlashCommandPanelSection = "recent" | NotesSlashCommandSection;
export type NotesSlashCommandKey = string;

export interface NotesSlashCommandItem {
  key: NotesSlashCommandKey;
  section: NotesSlashCommandSection;
  command: NotesSlashCommand;
  keywords: readonly string[];
  searchText: string;
}

export interface NotesSlashCommandOptions {
  canSetColor: boolean;
}

export interface NotesSlashCommandSections {
  recent: readonly NotesSlashCommandItem[];
  blocks: readonly NotesSlashCommandItem[];
  actions: readonly NotesSlashCommandItem[];
  colors: readonly NotesSlashCommandItem[];
}

export interface NotesSlashInputSession {
  open: boolean;
  query: string;
}

const ACTIONS = [
  "copy_link",
  "duplicate",
  "move_up",
  "move_down",
  "delete",
] as const satisfies readonly NotesSlashAction[];

let recentNotesSlashCommandKeys: NotesSlashCommandKey[] = [];

export function notesRecentSlashCommandKeys(): readonly NotesSlashCommandKey[] {
  return recentNotesSlashCommandKeys;
}

export function recordRecentNotesSlashCommandKey(
  key: NotesSlashCommandKey,
): readonly NotesSlashCommandKey[] {
  recentNotesSlashCommandKeys = recordNotesSlashCommandKey(recentNotesSlashCommandKeys, key);
  return recentNotesSlashCommandKeys;
}

export function notesSlashCommandItems(
  options: NotesSlashCommandOptions,
): readonly NotesSlashCommandItem[] {
  return [
    ...notesBlockInsertCommands().map(blockCommandItem),
    ...ACTIONS.map(actionCommandItem),
    ...(options.canSetColor
      ? [...NOTES_TEXT_COLORS, ...NOTES_BACKGROUND_COLORS].map(colorCommandItem)
      : []),
  ];
}

export function notesSlashCommandKey(command: NotesSlashCommand): NotesSlashCommandKey {
  switch (command.kind) {
    case "block":
    case "toggle_heading":
      return notesBlockInsertCommandKey(command);
    case "action":
      return `action:${command.action}`;
    case "color":
      return `color:${command.color}`;
  }
}

export function filterNotesSlashCommandItems(
  items: readonly NotesSlashCommandItem[],
  query: string,
): readonly NotesSlashCommandItem[] {
  const terms = normalizeSearchText(query).split(" ").filter(Boolean);
  if (terms.length === 0) return items;
  if (/^#{1,6}$/u.test(query.trim())) {
    return items.filter((item) => item.command.kind === "block" && item.command.blockType === `heading_${query.trim().length}`);
  }
  return items.filter((item) => {
    return terms.every((term) => item.searchText.includes(term));
  });
}

export function sectionNotesSlashCommandItems(
  items: readonly NotesSlashCommandItem[],
  query: string,
  recentKeys: readonly NotesSlashCommandKey[] = [],
): NotesSlashCommandSections {
  const filteredItems = filterNotesSlashCommandItems(items, query);
  const recent = query.trim()
    ? []
    : recentNotesSlashCommandItems(filteredItems, recentKeys);
  const recentKeySet = new Set(recent.map((item) => item.key));
  const groupedItems = filteredItems.filter((item) => !recentKeySet.has(item.key));
  return {
    recent,
    blocks: groupedItems.filter((item) => item.section === "blocks"),
    actions: groupedItems.filter((item) => item.section === "actions"),
    colors: groupedItems.filter((item) => item.section === "colors"),
  };
}

export function recordNotesSlashCommandKey(
  keys: readonly NotesSlashCommandKey[],
  key: NotesSlashCommandKey,
  limit = 5,
): NotesSlashCommandKey[] {
  return [key, ...keys.filter((candidate) => candidate !== key)].slice(0, limit);
}

export function flatNotesSlashCommandSectionItems(
  sections: NotesSlashCommandSections,
  sectionOrder: readonly NotesSlashCommandPanelSection[],
): readonly NotesSlashCommandItem[] {
  return sectionOrder.flatMap((section) => sections[section]);
}

export function clampNotesSlashActiveIndex(index: number, itemCount: number): number {
  if (itemCount <= 0) return 0;
  if (!Number.isFinite(index)) return 0;
  return Math.max(0, Math.min(Math.trunc(index), itemCount - 1));
}

export function nextNotesSlashActiveIndex(
  currentIndex: number,
  itemCount: number,
  direction: "next" | "previous",
): number {
  if (itemCount <= 0) return 0;
  const current = clampNotesSlashActiveIndex(currentIndex, itemCount);
  return direction === "next"
    ? (current + 1) % itemCount
    : (current + itemCount - 1) % itemCount;
}

/** Follow an existing slash query or text entered into an empty block, independently of keyboard layout. */
export function notesSlashInputSessionFromText(
  text: string,
  wasOpen: boolean,
  previousText?: string,
): NotesSlashInputSession {
  if ((!wasOpen && previousText !== "") || !text.startsWith("/") || text.includes("\n")) {
    return { open: false, query: "" };
  }
  return { open: true, query: text.slice(1) };
}

function recentNotesSlashCommandItems(
  items: readonly NotesSlashCommandItem[],
  recentKeys: readonly NotesSlashCommandKey[],
): readonly NotesSlashCommandItem[] {
  const itemByKey = new Map(items.map((item) => [item.key, item]));
  return recentKeys.flatMap((key) => {
    const item = itemByKey.get(key);
    return item ? [item] : [];
  });
}

function blockCommandItem(command: NotesSlashBlockCommand): NotesSlashCommandItem {
  return slashCommandItemWithSearchText({
    key: notesSlashCommandKey(command),
    section: "blocks",
    command,
    keywords: command.kind === "block"
      ? blockKeywords(command.blockType)
      : toggleHeadingKeywords(command.headingType),
  });
}

function actionCommandItem(action: NotesSlashAction): NotesSlashCommandItem {
  const command = { kind: "action", action } as const;
  return slashCommandItemWithSearchText({
    key: notesSlashCommandKey(command),
    section: "actions",
    command,
    keywords: actionKeywords(action),
  });
}

function colorCommandItem(color: NotesColor): NotesSlashCommandItem {
  const command = { kind: "color", color } as const;
  return slashCommandItemWithSearchText({
    key: notesSlashCommandKey(command),
    section: "colors",
    command,
    keywords: colorKeywords(color),
  });
}

function slashCommandItemWithSearchText(
  item: Omit<NotesSlashCommandItem, "searchText">,
): NotesSlashCommandItem {
  return {
    ...item,
    searchText: normalizeSearchText([item.key, ...item.keywords].join(" ")),
  };
}

function blockKeywords(type: NotesInsertableBlockType): readonly string[] {
  switch (type) {
    case "paragraph":
      return ["paragraph", "text"];
    case "heading_1":
      return ["heading 1", "h1", "title"];
    case "heading_2":
      return ["heading 2", "h2", "subtitle"];
    case "heading_3":
      return ["heading 3", "h3"];
    case "heading_4":
      return ["heading 4", "h4"];
    case "heading_5":
      return ["heading 5", "h5"];
    case "heading_6":
      return ["heading 6", "h6"];
    case "bulleted_list_item":
      return ["bullet", "bulleted list", "list"];
    case "numbered_list_item":
      return ["numbered", "numbered list", "ordered list"];
    case "to_do":
      return ["to do", "todo", "task", "checkbox"];
    case "toggle":
      return ["toggle", "toggle list", "collapse"];
    case "callout":
      return ["callout", "notice"];
    case "quote":
      return ["quote"];
    case "child_page":
      return ["page", "child page", "subpage"];
    case "child_database":
      return ["database", "table database", "data source"];
    case "breadcrumb":
      return ["breadcrumb", "path"];
    case "table_of_contents":
      return ["table of contents", "toc", "outline"];
    case "column_list":
      return ["columns", "column list", "layout"];
    case "table":
      return ["table", "grid"];
    case "tab":
      return ["tab", "tabs", "tabbed container"];
    case "image":
      return ["image", "picture", "photo"];
    case "video":
      return ["video"];
    case "audio":
      return ["audio", "music", "sound"];
    case "file":
      return ["file", "attachment"];
    case "pdf":
      return ["pdf", "document"];
    case "bookmark":
      return ["bookmark", "saved link"];
    case "link_preview":
      return ["link preview", "preview", "pasted link"];
    case "template":
      return ["template", "template button", "duplicate content"];
    case "button":
      return ["button", "automation", "insert blocks"];
    case "embed":
      return ["embed", "external"];
    case "equation":
      return ["equation", "math", "formula"];
    case "divider":
      return ["divider", "line", "separator"];
    case "code":
      return ["code", "snippet"];
  }
}

function toggleHeadingKeywords(type: NotesHeadingBlockType): readonly string[] {
  switch (type) {
    case "heading_1":
      return ["toggle heading 1", "toggle h1", "collapsible heading 1"];
    case "heading_2":
      return ["toggle heading 2", "toggle h2", "collapsible heading 2"];
    case "heading_3":
      return ["toggle heading 3", "toggle h3", "collapsible heading 3"];
    case "heading_4":
      return ["toggle heading 4", "toggle h4", "collapsible heading 4"];
    case "heading_5":
      return ["toggle heading 5", "toggle h5", "collapsible heading 5"];
    case "heading_6":
      return ["toggle heading 6", "toggle h6", "collapsible heading 6"];
  }
}

function actionKeywords(action: NotesSlashAction): readonly string[] {
  switch (action) {
    case "copy_link":
      return ["copy link", "block link", "link"];
    case "duplicate":
      return ["duplicate", "copy block", "clone"];
    case "move_up":
      return ["move up", "up"];
    case "move_down":
      return ["move down", "down"];
    case "delete":
      return ["delete", "remove"];
  }
}

function colorKeywords(color: NotesColor): readonly string[] {
  const normalizedColor = color.replace("_", " ");
  if (color === "default") return ["default", "default color", "color"];
  if (color.endsWith("_background")) {
    const base = color.replace("_background", "");
    return [normalizedColor, `${base} background`, `${base} bg`, "background", "color"];
  }
  return [normalizedColor, `${normalizedColor} text`, "text color", "color"];
}

function normalizeSearchText(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase().replace(/[_-]+/g, " ").replace(/\s+/g, " ").trim();
}
