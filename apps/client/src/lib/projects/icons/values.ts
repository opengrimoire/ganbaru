import { PALETTE_SIZE, type EventColor } from "$lib/calendar/types";

export type ProjectIconColor = EventColor | "default";

export type ProjectIconValue =
  | { kind: "none" }
  | { kind: "emoji"; emoji: string }
  | { kind: "lucide"; slug: string; color: ProjectIconColor }
  | { kind: "custom-emoji"; id: string }
  | { kind: "asset"; relativePath: string };

export const DEFAULT_PROJECT_ICON_VALUE: ProjectIconValue = {
  kind: "lucide",
  slug: "folder",
  color: "default",
};

const ICON_SLUG_PATTERN = /^[a-z0-9][a-z0-9-]*$/;
const ASSET_PATH_PATTERN = /^project-icons\/[a-f0-9]{64}\.(png|jpg|jpeg|webp)$/i;
function parseProjectIconColor(value: string): EventColor | null {
  const numericColor = Number(value);
  return Number.isInteger(numericColor) && numericColor >= 0 && numericColor < PALETTE_SIZE
    ? numericColor
    : null;
}

function isIconSlug(value: string): boolean {
  return ICON_SLUG_PATTERN.test(value);
}

export function isProjectIconAssetPath(value: string): boolean {
  return ASSET_PATH_PATTERN.test(value.trim());
}

export function parseProjectIcon(value: string | null | undefined): ProjectIconValue {
  const trimmed = value?.trim() ?? "";
  if (!trimmed) return DEFAULT_PROJECT_ICON_VALUE;
  if (trimmed === "none") return { kind: "none" };

  if (trimmed.startsWith("emoji:")) {
    const emoji = trimmed.slice("emoji:".length).trim();
    return emoji ? { kind: "emoji", emoji } : DEFAULT_PROJECT_ICON_VALUE;
  }

  if (trimmed.startsWith("lucide:")) {
    const parts = trimmed.split(":");
    const slug = parts[1] ?? "";
    if (!isIconSlug(slug)) return DEFAULT_PROJECT_ICON_VALUE;
    if (parts.length === 2) return { kind: "lucide", slug, color: "default" };
    if (parts.length !== 3) return DEFAULT_PROJECT_ICON_VALUE;
    const color = parseProjectIconColor(parts[2] ?? "");
    if (color === null) return DEFAULT_PROJECT_ICON_VALUE;
    return {
      kind: "lucide",
      slug,
      color,
    };
  }

  if (trimmed.startsWith("custom-emoji:")) {
    const id = trimmed.slice("custom-emoji:".length).trim();
    return id ? { kind: "custom-emoji", id } : DEFAULT_PROJECT_ICON_VALUE;
  }

  if (trimmed.startsWith("asset:")) {
    const relativePath = trimmed.slice("asset:".length).trim();
    return isProjectIconAssetPath(relativePath)
      ? { kind: "asset", relativePath }
      : DEFAULT_PROJECT_ICON_VALUE;
  }

  return DEFAULT_PROJECT_ICON_VALUE;
}

export function serializeProjectIcon(value: ProjectIconValue): string {
  switch (value.kind) {
    case "none":
      return "none";
    case "emoji":
      return value.emoji.trim() ? `emoji:${value.emoji.trim()}` : serializeProjectIcon(DEFAULT_PROJECT_ICON_VALUE);
    case "lucide":
      return value.color === "default"
        ? `lucide:${value.slug}`
        : `lucide:${value.slug}:${value.color}`;
    case "custom-emoji":
      return value.id.trim()
        ? `custom-emoji:${value.id.trim()}`
        : serializeProjectIcon(DEFAULT_PROJECT_ICON_VALUE);
    case "asset":
      return isProjectIconAssetPath(value.relativePath)
        ? `asset:${value.relativePath.trim()}`
        : serializeProjectIcon(DEFAULT_PROJECT_ICON_VALUE);
  }
}

export function projectIconColorToEventColor(color: ProjectIconColor): EventColor | undefined {
  return typeof color === "number" ? color : undefined;
}

export function projectIconDisplayLabel(value: ProjectIconValue): string {
  switch (value.kind) {
    case "none":
      return "None";
    case "emoji":
      return value.emoji;
    case "lucide":
      return value.slug
        .split("-")
        .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
        .join(" ");
    case "custom-emoji":
      return "Custom emoji";
    case "asset":
      return "Uploaded image";
  }
}

export function projectIconRecentValue(value: ProjectIconValue): string | null {
  if (value.kind === "none") return null;
  return serializeProjectIcon(value);
}
