import type { Component } from "svelte";
import ArrowDown from "@lucide/svelte/icons/arrow-down";
import ArrowUp from "@lucide/svelte/icons/arrow-up";
import Bookmark from "@lucide/svelte/icons/bookmark";
import Code from "@lucide/svelte/icons/code";
import Columns2 from "@lucide/svelte/icons/columns-2";
import Copy from "@lucide/svelte/icons/copy";
import Database from "@lucide/svelte/icons/database";
import ExternalLink from "@lucide/svelte/icons/external-link";
import FileIcon from "@lucide/svelte/icons/file";
import FileText from "@lucide/svelte/icons/file-text";
import Heading1 from "@lucide/svelte/icons/heading-1";
import Heading2 from "@lucide/svelte/icons/heading-2";
import Heading3 from "@lucide/svelte/icons/heading-3";
import Heading4 from "@lucide/svelte/icons/heading-4";
import Heading5 from "@lucide/svelte/icons/heading-5";
import Heading6 from "@lucide/svelte/icons/heading-6";
import ImageIcon from "@lucide/svelte/icons/image";
import LinkIcon from "@lucide/svelte/icons/link";
import List from "@lucide/svelte/icons/list";
import ListCollapse from "@lucide/svelte/icons/list-collapse";
import ListOrdered from "@lucide/svelte/icons/list-ordered";
import ListTree from "@lucide/svelte/icons/list-tree";
import MessageSquareWarning from "@lucide/svelte/icons/message-square-warning";
import Minus from "@lucide/svelte/icons/minus";
import MousePointerClick from "@lucide/svelte/icons/mouse-pointer-click";
import Music from "@lucide/svelte/icons/eighth-notes";
import Pilcrow from "@lucide/svelte/icons/pilcrow";
import Quote from "@lucide/svelte/icons/quote";
import Route from "@lucide/svelte/icons/route";
import Sigma from "@lucide/svelte/icons/sigma";
import SquareCheck from "@lucide/svelte/icons/square-check";
import Table2 from "@lucide/svelte/icons/table-2";
import Trash2 from "@lucide/svelte/icons/trash";
import Video from "@lucide/svelte/icons/video";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { NotesHeadingBlockType } from "$lib/notes/blocks/factory";
import type { NotesInsertableBlockType } from "$lib/notes/blocks/insertion";
import {
  notesSlashCommandItems,
  type NotesSlashAction,
  type NotesSlashCommand,
  type NotesSlashCommandItem,
  type NotesSlashCommandSections,
} from "$lib/notes/editor/slash-commands";
import type { NotesColor } from "$lib/notes/types";

/** The browsing group a block command is listed under. */
export type NotesSlashBlockGroup = "basic" | "media" | "advanced";

/** One labeled group of the slash menu, in the order the menu lists them. */
export interface NotesSlashMenuGroup {
  label: string;
  items: readonly NotesSlashCommandItem[];
}

/**
 * The slash menu's commands, searchable by their localized labels as well as their keywords.
 *
 * @param canSetColor Whether the block can take a color, which adds the color commands.
 * @param t The translator for the labels.
 * @returns Every command the menu can list.
 */
export function notesSlashMenuCatalog(canSetColor: boolean, t: Translate): readonly NotesSlashCommandItem[] {
  return notesSlashCommandItems({ canSetColor }).map((item) => ({
    ...item,
    searchText: `${item.searchText} ${notesSlashCommandLabel(item.command, t).normalize("NFD").replace(/\p{M}/gu, "").toLowerCase()}`,
  }));
}

/**
 * Group the sectioned commands as the slash menu lists them: recent, basic blocks, media, advanced, actions, then colors.
 *
 * @param sections The filtered commands, split by section.
 * @param t The translator for the group labels.
 * @returns Every group, including empty ones, which the menu skips.
 */
export function notesSlashMenuGroups(sections: NotesSlashCommandSections, t: Translate): readonly NotesSlashMenuGroup[] {
  const blocksIn = (group: NotesSlashBlockGroup) => sections.blocks.filter((item) => notesSlashBlockGroup(item.command) === group);
  return [
    { label: t("notes.slashRecent"), items: sections.recent },
    { label: t("notes.slashBlocks"), items: blocksIn("basic") },
    { label: t("notes.slashMedia"), items: blocksIn("media") },
    { label: t("notes.slashAdvanced"), items: blocksIn("advanced") },
    { label: t("notes.slashActions"), items: sections.actions },
    { label: t("notes.slashColors"), items: sections.colors },
  ];
}

/**
 * Group supported content types for menu browsing.
 *
 * @param command The command to place.
 * @returns The group its block type belongs to; actions and colors count as basic.
 */
export function notesSlashBlockGroup(command: NotesSlashCommand): NotesSlashBlockGroup {
  if (command.kind === "toggle_heading") return "advanced";
  if (command.kind !== "block") return "basic";
  switch (command.blockType) {
    case "image": case "video": case "audio": case "file": case "pdf": case "bookmark": case "link_preview": case "embed":
      return "media";
    case "child_database": case "breadcrumb": case "table_of_contents": case "column_list": case "table": case "tab": case "template": case "button": case "equation": case "code":
      return "advanced";
    default: return "basic";
  }
}

/**
 * The familiar Markdown cue shown beside basic content types.
 *
 * @param command The command to describe.
 * @returns The cue, or an empty string when the command has none.
 */
export function notesSlashCommandHint(command: NotesSlashCommand): string {
  if (command.kind !== "block") return "";
  switch (command.blockType) {
    case "heading_1": return "#";
    case "heading_2": return "##";
    case "heading_3": return "###";
    case "heading_4": return "####";
    case "heading_5": return "#####";
    case "heading_6": return "######";
    case "bulleted_list_item": return "-";
    case "numbered_list_item": return "1.";
    case "to_do": return "[]";
    case "toggle": return ">";
    default: return "";
  }
}

/**
 * The localized label of a slash command.
 *
 * @param command The command to name.
 * @param t The translator.
 * @returns The label the menu shows and searches.
 */
export function notesSlashCommandLabel(command: NotesSlashCommand, t: Translate): string {
  switch (command.kind) {
    case "block":
      return blockLabel(command.blockType, t);
    case "toggle_heading":
      return toggleHeadingLabel(command.headingType, t);
    case "action":
      return actionLabel(command.action, t);
    case "color":
      return colorLabel(command.color, t);
  }
}

/**
 * The icon of a slash command.
 *
 * @param command The command to draw.
 * @returns The icon, or null for colors, which draw a swatch instead.
 */
export function notesSlashCommandIcon(command: NotesSlashCommand): Component | null {
  switch (command.kind) {
    case "block":
      return blockIcon(command.blockType);
    case "toggle_heading":
      return headingIcon(command.headingType);
    case "action":
      return actionIcon(command.action);
    case "color":
      return null;
  }
}

function blockLabel(type: NotesInsertableBlockType, t: Translate): string {
  switch (type) {
    case "paragraph": return t("notes.blockType.paragraph");
    case "heading_1": return t("notes.blockType.heading1");
    case "heading_2": return t("notes.blockType.heading2");
    case "heading_3": return t("notes.blockType.heading3");
    case "heading_4": return t("notes.blockType.heading4");
    case "heading_5": return t("notes.blockType.heading5");
    case "heading_6": return t("notes.blockType.heading6");
    case "bulleted_list_item": return t("notes.blockType.bullet");
    case "numbered_list_item": return t("notes.blockType.numbered");
    case "to_do": return t("notes.blockType.todo");
    case "toggle": return t("notes.blockType.toggle");
    case "callout": return t("notes.blockType.callout");
    case "quote": return t("notes.blockType.quote");
    case "child_page": return t("notes.blockType.childPage");
    case "child_database": return t("notes.blockType.childDatabase");
    case "breadcrumb": return t("notes.blockType.breadcrumb");
    case "table_of_contents": return t("notes.blockType.tableOfContents");
    case "column_list": return t("notes.blockType.columns");
    case "table": return t("notes.blockType.table");
    case "tab": return t("notes.blockType.tab");
    case "image": return t("notes.blockType.image");
    case "video": return t("notes.blockType.video");
    case "audio": return t("notes.blockType.audio");
    case "file": return t("notes.blockType.file");
    case "pdf": return t("notes.blockType.pdf");
    case "bookmark": return t("notes.blockType.bookmark");
    case "link_preview": return t("notes.blockType.linkPreview");
    case "template": return t("notes.blockType.template");
    case "button": return t("notes.blockType.button");
    case "embed": return t("notes.blockType.embed");
    case "equation": return t("notes.blockType.equation");
    case "divider": return t("notes.blockType.divider");
    case "code": return t("notes.blockType.code");
  }
}

function toggleHeadingLabel(type: NotesHeadingBlockType, t: Translate): string {
  switch (type) {
    case "heading_1": return t("notes.blockType.toggleHeading1");
    case "heading_2": return t("notes.blockType.toggleHeading2");
    case "heading_3": return t("notes.blockType.toggleHeading3");
    case "heading_4": return t("notes.blockType.toggleHeading4");
    case "heading_5": return t("notes.blockType.toggleHeading5");
    case "heading_6": return t("notes.blockType.toggleHeading6");
  }
}

function actionLabel(action: NotesSlashAction, t: Translate): string {
  switch (action) {
    case "copy_link": return t("notes.copyBlockLink");
    case "duplicate": return t("notes.duplicateBlock");
    case "move_up": return t("notes.moveBlockUp");
    case "move_down": return t("notes.moveBlockDown");
    case "delete": return t("notes.deleteBlock");
  }
}

function colorLabel(color: NotesColor, t: Translate): string {
  switch (color) {
    case "default": return t("notes.blockColor.default");
    case "gray": return t("notes.blockColor.gray");
    case "brown": return t("notes.blockColor.brown");
    case "orange": return t("notes.blockColor.orange");
    case "yellow": return t("notes.blockColor.yellow");
    case "green": return t("notes.blockColor.green");
    case "blue": return t("notes.blockColor.blue");
    case "purple": return t("notes.blockColor.purple");
    case "pink": return t("notes.blockColor.pink");
    case "red": return t("notes.blockColor.red");
    case "gray_background": return t("notes.blockColor.grayBackground");
    case "brown_background": return t("notes.blockColor.brownBackground");
    case "orange_background": return t("notes.blockColor.orangeBackground");
    case "yellow_background": return t("notes.blockColor.yellowBackground");
    case "green_background": return t("notes.blockColor.greenBackground");
    case "blue_background": return t("notes.blockColor.blueBackground");
    case "purple_background": return t("notes.blockColor.purpleBackground");
    case "pink_background": return t("notes.blockColor.pinkBackground");
    case "red_background": return t("notes.blockColor.redBackground");
  }
}

function blockIcon(type: NotesInsertableBlockType): Component {
  switch (type) {
    case "paragraph": return Pilcrow;
    case "heading_1": case "heading_2": case "heading_3": case "heading_4": case "heading_5": case "heading_6":
      return headingIcon(type);
    case "bulleted_list_item": return List;
    case "numbered_list_item": return ListOrdered;
    case "to_do": return SquareCheck;
    case "toggle": return ListCollapse;
    case "callout": return MessageSquareWarning;
    case "quote": return Quote;
    case "child_page": return FileText;
    case "child_database": return Database;
    case "breadcrumb": return Route;
    case "table_of_contents": return ListTree;
    case "column_list": return Columns2;
    case "table": return Table2;
    case "tab": return Columns2;
    case "image": return ImageIcon;
    case "video": return Video;
    case "audio": return Music;
    case "file": return FileIcon;
    case "pdf": return FileText;
    case "bookmark": return Bookmark;
    case "link_preview": return LinkIcon;
    case "template": return FileText;
    case "button": return MousePointerClick;
    case "embed": return ExternalLink;
    case "equation": return Sigma;
    case "divider": return Minus;
    case "code": return Code;
  }
}

function headingIcon(type: NotesHeadingBlockType): Component {
  switch (type) {
    case "heading_1": return Heading1;
    case "heading_2": return Heading2;
    case "heading_3": return Heading3;
    case "heading_4": return Heading4;
    case "heading_5": return Heading5;
    case "heading_6": return Heading6;
  }
}

function actionIcon(action: NotesSlashAction): Component {
  switch (action) {
    case "copy_link": return LinkIcon;
    case "duplicate": return Copy;
    case "move_up": return ArrowUp;
    case "move_down": return ArrowDown;
    case "delete": return Trash2;
  }
}
