import {
  parseProjectIcon,
  serializeProjectIcon,
  type ProjectIconColor,
} from "$lib/projects/icons/values";
import {
  createNotesCustomEmojiPageIcon,
  createNotesEmojiPageIcon,
  createNotesNativePageIcon,
} from "./icon";
import type { NotesIconColor, NotesPageIcon } from "$lib/notes/types";

export interface NotesPageIconPickerCustomEmoji {
  id: string;
  name: string;
  assetPath: string;
}

const EVENT_SLOT_TO_NOTES_ICON_COLOR = [
  "pink", "pink", "red", "red", "red", "brown", "orange", "orange",
  "yellow", "yellow", "yellow", "green", "green", "green", "green", "green",
  "blue", "blue", "blue", "blue", "purple", "purple", "purple", "purple",
  "pink", "brown", "brown", "brown", "lightgray", "lightgray", "gray", "gray",
] as const satisfies readonly NotesIconColor[];

const NOTES_ICON_COLOR_TO_EVENT_SLOT: Record<NotesIconColor, number> = {
  gray: 30,
  lightgray: 28,
  brown: 26,
  yellow: 8,
  orange: 7,
  green: 13,
  blue: 18,
  purple: 22,
  pink: 1,
  red: 3,
};

function notesIconColorFromProjectColor(color: ProjectIconColor): NotesIconColor | undefined {
  if (typeof color === "number") return EVENT_SLOT_TO_NOTES_ICON_COLOR[color] ?? "lightgray";
  return undefined;
}

/** Convert Notes page icon metadata into the shared picker value format. */
export function notesPageIconPickerValue(icon: NotesPageIcon | null): string {
  if (!icon) return "none";
  if (icon.type === "emoji") {
    return serializeProjectIcon({ kind: "emoji", emoji: icon.emoji });
  }
  if (icon.type === "icon") {
    const color = icon.icon.color === undefined
      ? "default"
      : NOTES_ICON_COLOR_TO_EVENT_SLOT[icon.icon.color];
    return serializeProjectIcon({ kind: "lucide", slug: icon.icon.name, color });
  }
  if (icon.type === "custom_emoji") {
    return serializeProjectIcon({ kind: "custom-emoji", id: icon.custom_emoji.id });
  }
  return "none";
}

/** Convert a shared picker selection into validated Notes page icon metadata. */
export function notesPageIconFromPickerValue(
  value: string,
  customEmojis: readonly NotesPageIconPickerCustomEmoji[],
): NotesPageIcon | null {
  const icon = parseProjectIcon(value);
  if (icon.kind === "none") return null;
  if (icon.kind === "emoji") return createNotesEmojiPageIcon(icon.emoji);
  if (icon.kind === "lucide") {
    return createNotesNativePageIcon(icon.slug, notesIconColorFromProjectColor(icon.color));
  }
  if (icon.kind === "custom-emoji") {
    const customEmoji = customEmojis.find((candidate) => candidate.id === icon.id);
    if (!customEmoji) throw new Error("selected custom emoji is unavailable");
    return createNotesCustomEmojiPageIcon({
      id: customEmoji.id,
      name: customEmoji.name,
      assetPath: customEmoji.assetPath,
    });
  }
  throw new Error("uploaded note icons must use the Notes asset adapter");
}
