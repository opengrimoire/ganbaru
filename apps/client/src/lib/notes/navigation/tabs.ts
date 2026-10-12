import { createRichText } from "$lib/notes/blocks/factory";
import type {
  NotesBlockWrite,
  NotesBlockUpdate,
  NotesIcon,
  NotesIconColor,
  NotesParagraphBlock,
  NotesTabBlockItems,
} from "$lib/notes/types";

export const NOTES_TAB_MIN_COUNT = 1;
export const NOTES_TAB_MAX_COUNT = 12;

export const NOTES_TAB_EMOJI_ICON_CHOICES = [
  "📝",
  "📌",
  "✅",
  "💡",
  "🎯",
  "⭐",
] as const;

export const NOTES_TAB_NATIVE_ICON_CHOICES = [
  "file-text",
  "star",
  "check",
  "lightbulb",
  "target",
  "bookmark",
] as const;

export type NotesTabMoveDirection = "left" | "right";

export interface NotesTabIconOption {
  id: string;
  label: string;
  icon: NotesIcon | null;
  text: string;
}

const NATIVE_ICON_COLOR: NotesIconColor = "gray";

export function notesTabCanAdd(tabCount: number): boolean {
  return tabCount < NOTES_TAB_MAX_COUNT;
}

export function notesTabCanRemove(tabCount: number): boolean {
  return tabCount > NOTES_TAB_MIN_COUNT;
}

export function notesTabCanMove(
  tabs: readonly NotesTabBlockItems[],
  labelBlockId: string,
  direction: NotesTabMoveDirection,
): boolean {
  const index = tabs.findIndex((tab) => tab.label.id === labelBlockId);
  if (index < 0) return false;
  return direction === "left" ? index > 0 : index < tabs.length - 1;
}

export function createNotesTabLabelWrite(
  id: string,
  label: string,
  icon: NotesIcon | null = null,
): NotesBlockWrite {
  return {
    id,
    type: "paragraph",
    paragraph: createNotesTabLabelPayload(label, icon),
  };
}

export function notesTabLabelWithText(
  labelBlock: NotesParagraphBlock,
  label: string,
): NotesBlockUpdate {
  return createNotesTabLabelUpdate(label, labelBlock.paragraph.icon ?? null);
}

export function notesTabLabelWithIcon(
  labelBlock: NotesParagraphBlock,
  icon: NotesIcon | null,
): NotesBlockUpdate {
  return createNotesTabLabelUpdate(notesTabLabelText(labelBlock), icon);
}

export function notesTabLabelText(labelBlock: NotesParagraphBlock): string {
  return labelBlock.paragraph.rich_text.map((part) => part.plain_text).join("");
}

export function notesTabIconText(icon: NotesIcon | null | undefined): string | null {
  if (!icon) return null;
  if (icon.type === "emoji") return icon.emoji;
  if (icon.type === "icon") return "Icon";
  if (icon.type === "custom_emoji") return icon.custom_emoji.name ?? "Icon";
  return "Icon";
}

export function notesTabIconOptions(): NotesTabIconOption[] {
  const noIcon: NotesTabIconOption = {
    id: "none",
    label: "No icon",
    icon: null,
    text: "",
  };
  const emojiIcons = NOTES_TAB_EMOJI_ICON_CHOICES.map((emoji) => ({
    id: `emoji:${emoji}`,
    label: emoji,
    icon: { type: "emoji", emoji } satisfies NotesIcon,
    text: emoji,
  }));
  const nativeIcons = NOTES_TAB_NATIVE_ICON_CHOICES.map((name) => ({
    id: `icon:${name}`,
    label: name,
    icon: { type: "icon", icon: { name, color: NATIVE_ICON_COLOR } } satisfies NotesIcon,
    text: "Icon",
  }));
  return [noIcon, ...emojiIcons, ...nativeIcons];
}

function createNotesTabLabelPayload(label: string, icon: NotesIcon | null) {
  const trimmed = label.trim();
  return {
    rich_text: [createRichText(trimmed || "Tab")],
    color: "default" as const,
    ...(icon === null ? { icon: null } : { icon }),
  };
}

function createNotesTabLabelUpdate(label: string, icon: NotesIcon | null): NotesBlockUpdate {
  return {
    type: "paragraph",
    paragraph: createNotesTabLabelPayload(label, icon),
  };
}
