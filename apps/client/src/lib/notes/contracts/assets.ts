import type { NotesBlockLayout, NotesRichText } from "./core";

export const NOTES_ICON_COLORS = [
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
] as const;

export type NotesIconColor = (typeof NOTES_ICON_COLORS)[number];

export interface NotesCustomEmojiIcon {
  id: string;
  name?: string;
  url?: string;
  ganbaru_asset_path?: string;
}

export interface NotesIconFile {
  url: string;
  expiry_time?: string;
  name?: string;
  content_type?: "image/png" | "image/jpeg" | "image/webp";
  byte_size?: number;
  sha256?: string;
  ganbaru_asset_path?: string;
}

export type NotesIcon =
  | { type: "emoji"; emoji: string }
  | { type: "custom_emoji"; custom_emoji: NotesCustomEmojiIcon }
  | { type: "icon"; icon: { name: string; color?: NotesIconColor } }
  | { type: "external"; external: { url: string } }
  | { type: "file"; file: NotesIconFile };

export type NotesCalloutIcon = NotesIcon | null;

export type NotesFileObject =
  | { type: "external"; external: { url: string } }
  | {
      type: "file";
      file: {
        url: string;
        expiry_time?: string;
        name?: string;
        content_type?: string;
        byte_size?: number;
        sha256?: string;
        ganbaru_asset_path?: string;
      };
    }
  | { type: "file_upload"; file_upload: { id: string } };

export const NOTES_COVER_DESIGNS = ["contours", "studio", "botanical", "orbit", "atlas", "study", "mathematics", "programming", "finance", "solid", "gradient", "mosaic", "dots", "grid"] as const;
export type NotesCoverColor = number | "default";
export type NotesCoverDesign = (typeof NOTES_COVER_DESIGNS)[number];

/** A subject location in the original image, independent of the rendered crop. */
export interface NotesCoverFocalPoint {
  x: number;
  y: number;
}

/** Designed covers retain their theme palette identity instead of rasterized pixels. */
export type NotesPageCover =
  | { type: "design"; design: { pattern: NotesCoverDesign; color: NotesCoverColor } }
  | (NotesFileObject & { focal_point?: NotesCoverFocalPoint });

export type NotesMediaBlockPayload = NotesFileObject & NotesBlockLayout & {
  caption?: NotesRichText[];
  name?: string;
};
