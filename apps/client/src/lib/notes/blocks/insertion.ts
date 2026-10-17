import { anchoredPanelStyle } from "$lib/utils/anchored-panel";
import {
  createBlockWrite,
  createTextPayload,
  type NotesHeadingBlockType,
} from "./factory";
import type { NotesBlockType } from "$lib/notes/types";
import type { NotesBlockWrite } from "$lib/notes/types";

export const NOTES_INSERTABLE_BLOCK_TYPES = [
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
  "quote",
  "child_page",
  "child_database",
  "breadcrumb",
  "table_of_contents",
  "column_list",
  "table",
  "tab",
  "image",
  "video",
  "audio",
  "file",
  "pdf",
  "bookmark",
  "link_preview",
  "template",
  "button",
  "embed",
  "equation",
  "divider",
  "code",
] as const satisfies readonly NotesBlockType[];

export type NotesInsertableBlockType = (typeof NOTES_INSERTABLE_BLOCK_TYPES)[number];
export type NotesBlockInsertCommand =
  | { kind: "block"; blockType: NotesInsertableBlockType }
  | { kind: "toggle_heading"; headingType: NotesHeadingBlockType };
export type NotesBlockInsertRequest = NotesBlockInsertCommand;
export interface NotesBlockInsertMenuRect {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface NotesBlockInsertMenuPlacementInput {
  triggerRect: NotesBlockInsertMenuRect;
  viewportWidth: number;
  viewportHeight: number;
  preferredWidth?: number;
  preferredMaxHeight?: number;
  margin?: number;
  gap?: number;
}

const NOTES_INSERTABLE_TOGGLE_HEADING_TYPES = [
  "heading_1",
  "heading_2",
  "heading_3",
  "heading_4",
  "heading_5",
  "heading_6",
] as const satisfies readonly NotesHeadingBlockType[];

export function isNotesInsertableBlockType(value: unknown): value is NotesInsertableBlockType {
  return (
    typeof value === "string"
    && NOTES_INSERTABLE_BLOCK_TYPES.includes(value as NotesInsertableBlockType)
  );
}

export function notesInsertableBlockTypes(): readonly NotesInsertableBlockType[] {
  return NOTES_INSERTABLE_BLOCK_TYPES;
}

export function notesBlockInsertCommands(): readonly NotesBlockInsertCommand[] {
  return [
    ...NOTES_INSERTABLE_BLOCK_TYPES.map((blockType) => ({ kind: "block" as const, blockType })),
    ...NOTES_INSERTABLE_TOGGLE_HEADING_TYPES.map((headingType) => ({
      kind: "toggle_heading" as const,
      headingType,
    })),
  ];
}

export function notesBlockInsertCommandKey(command: NotesBlockInsertCommand): string {
  switch (command.kind) {
    case "block":
      return `block:${command.blockType}`;
    case "toggle_heading":
      return `toggle-heading:${command.headingType}`;
  }
}

export function normalizeNotesBlockInsertCommand(
  request?: NotesBlockInsertRequest,
): NotesBlockInsertCommand {
  if (request === undefined) return { kind: "block", blockType: "paragraph" };
  return request;
}

export function createBlockWriteFromInsertCommand(
  id: string,
  command: NotesBlockInsertCommand,
): NotesBlockWrite {
  if (command.kind === "block") return createBlockWrite(id, command.blockType);
  const payload = createTextPayload("", { isToggleable: true, open: true });
  switch (command.headingType) {
    case "heading_1":
      return { id, type: "heading_1", heading_1: payload };
    case "heading_2":
      return { id, type: "heading_2", heading_2: payload };
    case "heading_3":
      return { id, type: "heading_3", heading_3: payload };
    case "heading_4":
      return { id, type: "heading_4", heading_4: payload };
    case "heading_5":
      return { id, type: "heading_5", heading_5: payload };
    case "heading_6":
      return { id, type: "heading_6", heading_6: payload };
  }
}

export function notesBlockInsertMenuStyle(input: NotesBlockInsertMenuPlacementInput): string {
  return anchoredPanelStyle(input);
}
