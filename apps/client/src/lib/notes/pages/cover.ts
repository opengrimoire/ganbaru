import { externalMediaUrlIsSupported } from "$lib/notes/block-types/media";
import type { NotesPageCover } from "$lib/notes/types";
import { NOTES_COVER_DESIGNS, type NotesCoverDesign, type NotesCoverColor, type NotesCoverFocalPoint } from "$lib/notes/contracts/assets";
import { PALETTE_SIZE } from "$lib/calendar/types";

export { NOTES_COVER_DESIGNS };
export const NOTES_COVER_DEFAULT_FOCAL_POINT: Readonly<NotesCoverFocalPoint> = { x: 0.5, y: 0.5 };

/** Validate the editable design at a typed or external boundary. */
export function createNotesDesignCover(pattern: NotesCoverDesign, color: NotesCoverColor): NotesPageCover {
  if (!NOTES_COVER_DESIGNS.includes(pattern)) throw new Error("Unsupported cover design");
  if (color !== "default" && (!Number.isInteger(color) || color < 0 || color >= PALETTE_SIZE)) {
    throw new Error("Cover color must reference a theme palette slot");
  }
  return { type: "design", design: { pattern, color } };
}

/** Render all design previews and banners from the same theme-resolved color. */
export function notesCoverDesignBackground(pattern: NotesCoverDesign, color: string): string {
  const base = `color-mix(in srgb, ${color} 38%, var(--background))`;
  const soft = `color-mix(in srgb, ${color} 16%, var(--background))`;
  const ink = `color-mix(in srgb, ${color} 65%, var(--foreground))`;
  switch (pattern) {
    case "solid": return base;
    case "gradient": return `linear-gradient(120deg, ${soft}, ${base} 55%, ${color})`;
    case "mosaic": return soft;
    case "contours": return `linear-gradient(125deg, ${soft}, ${base} 45%, ${ink})`;
    case "studio": return `linear-gradient(150deg, ${soft}, ${base})`;
    case "botanical": return `color-mix(in srgb, ${color} 22%, var(--background))`;
    case "orbit": return `radial-gradient(ellipse at 60% 30%, ${color}, color-mix(in srgb, ${color} 52%, black) 90%)`;
    case "atlas": return `linear-gradient(145deg, ${soft}, ${base} 65%, ${soft})`;
    case "study": return `color-mix(in srgb, ${color} 10%, var(--background))`;
    case "mathematics": return `color-mix(in srgb, ${color} 48%, black)`;
    case "programming": return `color-mix(in srgb, ${color} 26%, var(--background))`;
    case "finance": return `color-mix(in srgb, ${color} 18%, var(--background))`;
    case "dots": return `radial-gradient(circle, ${ink} 1px, transparent 1.5px) 0 0 / 18px 18px, ${soft}`;
    case "grid": return `linear-gradient(${base} 1px, transparent 1px) 0 0 / 28px 28px, linear-gradient(90deg, ${base} 1px, transparent 1px) 0 0 / 28px 28px, ${soft}`;
  }
}

/** Place a subject at the center of a filled image, clamped to its edges. */
export function notesCoverObjectPosition(
  focal: NotesCoverFocalPoint,
  image: { width: number; height: number },
  viewport: { width: number; height: number },
): string {
  if (image.width <= 0 || image.height <= 0 || viewport.width <= 0 || viewport.height <= 0) return "50% 50%";
  const scale = Math.max(viewport.width / image.width, viewport.height / image.height);
  const axis = (point: number, source: number, target: number): number => {
    const overflow = source * scale - target;
    if (overflow <= 0) return 50;
    return Math.max(0, Math.min(1, (point * source * scale - target / 2) / overflow)) * 100;
  };
  return `${axis(focal.x, image.width, viewport.width)}% ${axis(focal.y, image.height, viewport.height)}%`;
}

/** Move a filled image with the pointer, preserving the visible crop at clamped edges. */
export function notesCoverFocalPointFromDrag(
  focal: NotesCoverFocalPoint,
  delta: { x: number; y: number },
  image: { width: number; height: number },
  viewport: { width: number; height: number },
): NotesCoverFocalPoint {
  if (image.width <= 0 || image.height <= 0 || viewport.width <= 0 || viewport.height <= 0) return { ...focal };
  const scale = Math.max(viewport.width / image.width, viewport.height / image.height);
  const axis = (point: number, movement: number, source: number, target: number): number => {
    const scaled = source * scale;
    const overflow = Math.max(0, scaled - target);
    if (overflow === 0 || movement === 0) return point;
    const offset = Math.max(0, Math.min(overflow, point * scaled - target / 2));
    return (Math.max(0, Math.min(overflow, offset - movement)) + target / 2) / scaled;
  };
  return { x: axis(focal.x, delta.x, image.width, viewport.width), y: axis(focal.y, delta.y, image.height, viewport.height) };
}

export interface NotesPageCoverAssetMetadata {
  relativePath: string;
  originalName?: string | null;
  contentType: string;
  byteSize: number;
  sha256: string;
}

const NOTES_PAGE_COVER_ASSET_PATTERN = /^notes\/page-covers\/[a-f0-9]{64}\.(png|jpg|jpeg|webp)$/i;

/** Create a Notion-style external page cover payload for the Tauri update boundary. */
export function createNotesExternalPageCover(url: string): NotesPageCover {
  const trimmed = url.trim();
  if (!trimmed) throw new Error("page cover URL must not be empty");
  if (!isSupportedExternalPageCoverUrl(trimmed)) {
    throw new Error("page cover URL must be a supported HTTPS image URL");
  }
  return { type: "external", external: { url: trimmed } };
}

/** Create a Notion-style local file cover payload from a managed asset. */
export function createNotesLocalFilePageCover(asset: NotesPageCoverAssetMetadata): Extract<NotesPageCover, { type: "file" }> {
  const relativePath = asset.relativePath.trim();
  if (!isNotesPageCoverAssetPath(relativePath)) {
    throw new Error("page cover asset path must stay under notes/page-covers");
  }
  if (!["image/png", "image/jpeg", "image/webp"].includes(asset.contentType)) {
    throw new Error("page cover asset must be a PNG, JPG, or WebP image");
  }
  if (!Number.isInteger(asset.byteSize) || asset.byteSize <= 0) {
    throw new Error("page cover asset size must be positive");
  }
  if (!/^[a-f0-9]{64}$/.test(asset.sha256)) {
    throw new Error("page cover asset hash must be a SHA-256 hex digest");
  }
  const originalName = asset.originalName?.trim();
  return {
    type: "file",
    file: {
      url: managedCoverAssetUrl(relativePath),
      ...(originalName ? { name: originalName } : {}),
      content_type: asset.contentType as "image/png" | "image/jpeg" | "image/webp",
      byte_size: asset.byteSize,
      sha256: asset.sha256,
      ganbaru_asset_path: relativePath,
    },
  };
}

/** Return the previewable URL for cover sources that can be rendered locally. */
export function notesPageCoverUrl(cover: NotesPageCover | null): string | null {
  if (!cover) return null;
  if (cover.type === "external") return cover.external.url;
  if (cover.type === "file" && !cover.file.ganbaru_asset_path) return cover.file.url;
  return null;
}

/** Return the managed asset path referenced by a local cover, if one exists. */
export function notesPageCoverAssetPath(cover: NotesPageCover | null): string | null {
  if (!cover || cover.type !== "file") return null;
  return cover.file.ganbaru_asset_path ?? null;
}

/** Return true when the relative path points to a managed Notes page cover asset. */
export function isNotesPageCoverAssetPath(value: string): boolean {
  return NOTES_PAGE_COVER_ASSET_PATTERN.test(value.trim());
}

/** Return true when the URL can be used as an external page cover image. */
export function isSupportedExternalPageCoverUrl(url: string): boolean {
  return externalMediaUrlIsSupported("image", url);
}

function managedCoverAssetUrl(relativePath: string): string {
  return `ganbaru-asset:${relativePath.trim()}`;
}
