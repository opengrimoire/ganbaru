import {
  createBlockWrite,
  createButtonPayload,
  createCodePayload,
  createTextPayloadFromRichText,
  richTextPlainText,
} from "./factory";
import type { NotesBlockType, NotesBlockWrite, NotesColor, NotesRichText } from "$lib/notes/types";

export function createBlockWriteFromRichText(
  id: string,
  type: NotesBlockType,
  richText: readonly NotesRichText[],
  color: NotesColor = "default",
): NotesBlockWrite {
  switch (type) {
    case "paragraph":
      return { id, type, paragraph: createTextPayloadFromRichText(richText, color) };
    case "heading_1":
      return { id, type, heading_1: createTextPayloadFromRichText(richText, color) };
    case "heading_2":
      return { id, type, heading_2: createTextPayloadFromRichText(richText, color) };
    case "heading_3":
      return { id, type, heading_3: createTextPayloadFromRichText(richText, color) };
    case "heading_4":
      return { id, type, heading_4: createTextPayloadFromRichText(richText, color) };
    case "heading_5":
      return { id, type, heading_5: createTextPayloadFromRichText(richText, color) };
    case "heading_6":
      return { id, type, heading_6: createTextPayloadFromRichText(richText, color) };
    case "bulleted_list_item":
      return { id, type, bulleted_list_item: createTextPayloadFromRichText(richText, color) };
    case "numbered_list_item":
      return { id, type, numbered_list_item: createTextPayloadFromRichText(richText, color) };
    case "to_do":
      return {
        id,
        type,
        to_do: { ...createTextPayloadFromRichText(richText, color), checked: false },
      };
    case "toggle":
      return {
        id,
        type,
        toggle: { ...createTextPayloadFromRichText(richText, color), ganbaru_open: true },
      };
    case "callout":
      return {
        id,
        type,
        callout: {
          ...createTextPayloadFromRichText(richText, color),
          icon: { type: "icon", icon: { name: "info", color: "gray" } },
        },
      };
    case "quote":
      return { id, type, quote: createTextPayloadFromRichText(richText, color) };
    case "code":
      return { id, type, code: { ...createCodePayload(""), rich_text: [...richText] } };
    case "template":
      return { id, type, template: { rich_text: [...richText] } };
    case "button":
      return { id, type, button: { ...createButtonPayload("Button"), rich_text: [...richText] } };
    default:
      return createBlockWrite(id, type, richTextPlainText(richText), color);
  }
}
