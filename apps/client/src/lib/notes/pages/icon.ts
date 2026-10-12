import type { NotesIconColor, NotesPageIcon } from "$lib/notes/types";
import { COMPACT_IDENTITY_EMOJI_SCALE } from "$lib/icon-sizing";

export const NOTES_PAGE_CHROME_EMOJI_SCALE = COMPACT_IDENTITY_EMOJI_SCALE;

export const NOTES_PAGE_ICON_COLOR_CHOICES = [
  "gray",
  "lightgray",
  "brown",
  "yellow",
  "orange",
  "green",
  "blue",
  "purple",
  "pink",
  "red",
] as const satisfies readonly NotesIconColor[];

export interface NotesPageIconAssetMetadata {
  relativePath: string;
  originalName?: string | null;
  contentType: string;
  byteSize: number;
  sha256: string;
}

export interface NotesCustomEmojiIconSource {
  id: string;
  name?: string;
  url?: string;
  assetPath?: string;
}

const ICON_IMAGE_EXTENSIONS = [".gif", ".jpeg", ".jpg", ".png", ".webp"] as const;
const NOTES_PAGE_ICON_ASSET_PATTERN = /^notes\/page-icons\/[a-f0-9]{64}\.(png|jpg|jpeg|webp)$/i;
const PROJECT_ICON_ASSET_PATTERN = /^project-icons\/[a-f0-9]{64}\.(png|jpg|jpeg|webp)$/i;

/** Create a validated page emoji icon payload for the Tauri update boundary. */
export function createNotesEmojiPageIcon(emoji: string): NotesPageIcon {
  const trimmed = emoji.trim();
  if (!trimmed) throw new Error("page icon emoji must not be empty");
  return { type: "emoji", emoji: trimmed };
}

/** Create a validated Notion-style native page icon payload. */
export function createNotesNativePageIcon(name: string, color?: NotesIconColor): NotesPageIcon {
  const trimmed = name.trim();
  if (!trimmed) throw new Error("page icon name must not be empty");
  return {
    type: "icon",
    icon: {
      name: trimmed,
      ...(color ? { color } : {}),
    },
  };
}

/** Create a validated Notion-style custom emoji icon payload. */
export function createNotesCustomEmojiPageIcon(source: NotesCustomEmojiIconSource): NotesPageIcon {
  const id = source.id.trim();
  if (!id) throw new Error("page custom emoji id must not be empty");
  const name = source.name?.trim();
  const assetPath = source.assetPath?.trim();
  if (assetPath !== undefined && !isProjectIconAssetPath(assetPath)) {
    throw new Error("page custom emoji asset path must stay under project-icons");
  }
  const sourceUrl = source.url?.trim();
  if (sourceUrl !== undefined && sourceUrl.startsWith("ganbaru-asset:")) {
    throw new Error("page custom emoji managed URL must include an asset path");
  }
  if (sourceUrl !== undefined && !isSupportedExternalPageIconUrl(sourceUrl)) {
    throw new Error("page custom emoji URL must be a supported HTTPS image URL");
  }
  const url = assetPath ? managedIconAssetUrl(assetPath) : sourceUrl;
  return {
    type: "custom_emoji",
    custom_emoji: {
      id,
      ...(name ? { name } : {}),
      ...(url ? { url } : {}),
      ...(assetPath ? { ganbaru_asset_path: assetPath } : {}),
    },
  };
}

/** Create a validated external image page icon payload. */
export function createNotesExternalPageIcon(url: string): NotesPageIcon {
  const trimmed = url.trim();
  if (!trimmed) throw new Error("page icon URL must not be empty");
  if (!isSupportedExternalPageIconUrl(trimmed)) {
    throw new Error("page icon URL must be a supported HTTPS image URL");
  }
  return { type: "external", external: { url: trimmed } };
}

/** Create a validated local file page icon payload from a managed asset. */
export function createNotesLocalFilePageIcon(asset: NotesPageIconAssetMetadata): NotesPageIcon {
  const relativePath = asset.relativePath.trim();
  if (!isNotesPageIconAssetPath(relativePath)) {
    throw new Error("page icon asset path must stay under notes/page-icons");
  }
  if (!["image/png", "image/jpeg", "image/webp"].includes(asset.contentType)) {
    throw new Error("page icon asset must be a PNG, JPG, or WebP image");
  }
  if (!Number.isInteger(asset.byteSize) || asset.byteSize <= 0) {
    throw new Error("page icon asset size must be positive");
  }
  if (!/^[a-f0-9]{64}$/.test(asset.sha256)) {
    throw new Error("page icon asset hash must be a SHA-256 hex digest");
  }
  const originalName = asset.originalName?.trim();
  return {
    type: "file",
    file: {
      url: managedIconAssetUrl(relativePath),
      ...(originalName ? { name: originalName } : {}),
      content_type: asset.contentType as "image/png" | "image/jpeg" | "image/webp",
      byte_size: asset.byteSize,
      sha256: asset.sha256,
      ganbaru_asset_path: relativePath,
    },
  };
}

/** Return the text glyph used for a page icon when the current renderer supports it. */
export function notesPageIconText(icon: NotesPageIcon | null): string | null {
  return icon?.type === "emoji" ? icon.emoji : null;
}

/** Return the managed asset path referenced by an icon, if one exists. */
export function notesPageIconAssetPath(icon: NotesPageIcon | null): string | null {
  if (!icon) return null;
  if (icon.type === "file" && icon.file.ganbaru_asset_path) return icon.file.ganbaru_asset_path;
  if (icon.type === "custom_emoji" && icon.custom_emoji.ganbaru_asset_path) {
    return icon.custom_emoji.ganbaru_asset_path;
  }
  return null;
}

/** Return a direct external URL when an icon can be rendered from an external image. */
export function notesPageIconExternalUrl(icon: NotesPageIcon | null): string | null {
  if (!icon) return null;
  if (icon.type === "external") return icon.external.url;
  if (icon.type === "file" && !icon.file.ganbaru_asset_path) return icon.file.url;
  if (icon.type === "custom_emoji" && !icon.custom_emoji.ganbaru_asset_path) {
    return icon.custom_emoji.url ?? null;
  }
  return null;
}

/** Return a compact text label for accessible page icon controls. */
export function notesPageIconLabel(icon: NotesPageIcon | null): string {
  if (!icon) return "No icon";
  if (icon.type === "emoji") return icon.emoji;
  if (icon.type === "icon") return icon.icon.name;
  if (icon.type === "custom_emoji") return icon.custom_emoji.name ?? "Custom emoji";
  if (icon.type === "external") return "External image";
  return icon.file.name ?? "Uploaded image";
}

/** Return the display color for Notion-style native page icons. */
export function notesPageNativeIconColor(color: NotesIconColor | undefined): string | undefined {
  switch (color) {
    case undefined:
      return undefined;
    case "gray":
      return "#646470";
    case "lightgray":
      return "#9ca3af";
    case "brown":
      return "#8b5e3c";
    case "yellow":
      return "#ca8a04";
    case "orange":
      return "#ea580c";
    case "green":
      return "#16a34a";
    case "blue":
      return "#2563eb";
    case "purple":
      return "#7c3aed";
    case "pink":
      return "#db2777";
    case "red":
      return "#dc2626";
  }
}

/** Return true when the relative path points to a managed Notes page icon asset. */
export function isNotesPageIconAssetPath(value: string): boolean {
  return NOTES_PAGE_ICON_ASSET_PATTERN.test(value.trim());
}

/** Return true when the relative path points to a reusable custom emoji asset. */
export function isProjectIconAssetPath(value: string): boolean {
  return PROJECT_ICON_ASSET_PATTERN.test(value.trim());
}

/** Return true when the URL can be used as an external page icon image. */
export function isSupportedExternalPageIconUrl(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url.trim());
  } catch {
    return false;
  }
  if (parsed.protocol !== "https:") return false;
  const pathname = parsed.pathname.toLowerCase();
  return ICON_IMAGE_EXTENSIONS.some((extension) => pathname.endsWith(extension));
}

function managedIconAssetUrl(relativePath: string): string {
  return `ganbaru-asset:${relativePath.trim()}`;
}
