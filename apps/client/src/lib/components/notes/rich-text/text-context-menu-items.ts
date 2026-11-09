import Code from "@lucide/svelte/icons/code";
import Heading1 from "@lucide/svelte/icons/heading-1";
import Heading2 from "@lucide/svelte/icons/heading-2";
import Heading3 from "@lucide/svelte/icons/heading-3";
import Heading4 from "@lucide/svelte/icons/heading-4";
import Heading5 from "@lucide/svelte/icons/heading-5";
import Heading6 from "@lucide/svelte/icons/heading-6";
import List from "@lucide/svelte/icons/list";
import ListOrdered from "@lucide/svelte/icons/list-ordered";
import Minus from "@lucide/svelte/icons/minus";
import Pilcrow from "@lucide/svelte/icons/pilcrow";
import Quote from "@lucide/svelte/icons/quote";
import SquareCheck from "@lucide/svelte/icons/square-check";

/** The block types the text context menu's Paragraph submenu converts a block to, in the order it lists them. */
export const NOTES_TEXT_CONTEXT_PARAGRAPH_ITEMS = [
  { type: "paragraph", labelKey: "notes.blockType.paragraph", icon: Pilcrow },
  { type: "heading_1", labelKey: "notes.blockType.heading1", icon: Heading1 },
  { type: "heading_2", labelKey: "notes.blockType.heading2", icon: Heading2 },
  { type: "heading_3", labelKey: "notes.blockType.heading3", icon: Heading3 },
  { type: "heading_4", labelKey: "notes.blockType.heading4", icon: Heading4 },
  { type: "heading_5", labelKey: "notes.blockType.heading5", icon: Heading5 },
  { type: "heading_6", labelKey: "notes.blockType.heading6", icon: Heading6 },
  { type: "bulleted_list_item", labelKey: "notes.blockType.bullet", icon: List },
  { type: "numbered_list_item", labelKey: "notes.blockType.numbered", icon: ListOrdered },
  { type: "to_do", labelKey: "notes.blockType.todo", icon: SquareCheck },
  { type: "quote", labelKey: "notes.blockType.quote", icon: Quote },
  { type: "code", labelKey: "notes.blockType.code", icon: Code },
] as const;

/** The blocks the text context menu's Insert submenu adds below a block, in the order it lists them. */
export const NOTES_TEXT_CONTEXT_INSERT_ITEMS = [
  { type: "paragraph", labelKey: "notes.blockType.paragraph", icon: Pilcrow },
  { type: "bulleted_list_item", labelKey: "notes.blockType.bullet", icon: List },
  { type: "numbered_list_item", labelKey: "notes.blockType.numbered", icon: ListOrdered },
  { type: "to_do", labelKey: "notes.blockType.todo", icon: SquareCheck },
  { type: "code", labelKey: "notes.blockType.code", icon: Code },
  { type: "divider", labelKey: "notes.blockType.divider", icon: Minus },
] as const;
