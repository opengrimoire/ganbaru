import { NOTES_COVER_DESIGNS, NOTES_ICON_COLORS } from "../contracts/assets";
import type { NotesCalloutIcon, NotesFileObject, NotesIconColor, NotesMediaBlockPayload, NotesPageCover } from "../contracts/assets";
import type { NotesBookmarkBlockPayload, NotesEmbedBlockPayload, NotesLinkPreviewBlockPayload, NotesPageIcon } from "../contracts/core";
import type { NotesDatabaseGalleryCoverSource } from "$lib/notes/contracts/database";
import { externalMediaUrlIsSupported } from "$lib/notes/block-types/media";
import type { NotesMediaBlockType } from "$lib/notes/block-types/media";
import { createNotesDesignCover, isNotesPageCoverAssetPath, isSupportedExternalPageCoverUrl } from "$lib/notes/pages/cover";
import { isNotesPageIconAssetPath, isProjectIconAssetPath, isSupportedExternalPageIconUrl } from "$lib/notes/pages/icon";
import { UUID_PATTERN, containsControlCharacters, readDisplayString, readInteger, readNotesIconColor, readOptionalDisplayString, readRecord, readString } from "./readers";
import { parseNotesRichTextArray } from "./rich-text";

export function isNotesIconColor(value: unknown): value is NotesIconColor {
  return typeof value === "string" && NOTES_ICON_COLORS.includes(value as NotesIconColor);
}

export function parseNullableNotesIcon(value: unknown, label: string): NotesCalloutIcon {
  if (value === null) return null;
  return parseNotesIcon(value, label);
}

function parseNotesIcon(value: unknown, label: string): NotesPageIcon {
  const record = readRecord(value, label);
  const type = readString(record.type, `${label}.type`);
  if (type === "emoji") {
    return { type, emoji: readDisplayString(record.emoji, `${label}.emoji`) };
  }
  if (type === "custom_emoji") {
    const customEmoji = readRecord(record.custom_emoji, `${label}.custom_emoji`);
    const id = readDisplayString(customEmoji.id, `${label}.custom_emoji.id`);
    const name = readOptionalDisplayString(customEmoji.name, `${label}.custom_emoji.name`);
    const url = readOptionalDisplayString(customEmoji.url, `${label}.custom_emoji.url`);
    const assetPath = readOptionalDisplayString(
      customEmoji.ganbaru_asset_path,
      `${label}.custom_emoji.ganbaru_asset_path`,
    );
    if (assetPath !== undefined && !isProjectIconAssetPath(assetPath)) {
      throw new Error(`${label}.custom_emoji.ganbaru_asset_path must stay under a managed image asset directory`);
    }
    if (url !== undefined) {
      if (url.startsWith("ganbaru-asset:")) {
        const urlAssetPath = url.slice("ganbaru-asset:".length);
        if (!isProjectIconAssetPath(urlAssetPath)) {
          throw new Error(`${label}.custom_emoji.url must stay under a managed image asset directory`);
        }
        if (assetPath !== urlAssetPath) {
          throw new Error(`${label}.custom_emoji.url must reference the managed icon asset path`);
        }
      } else if (!isSupportedExternalPageIconUrl(url)) {
        throw new Error(`${label}.custom_emoji.url must be a supported HTTPS image URL`);
      }
    }
    return {
      type,
      custom_emoji: {
        id,
        ...(name === undefined ? {} : { name }),
        ...(url === undefined ? {} : { url }),
        ...(assetPath === undefined ? {} : { ganbaru_asset_path: assetPath }),
      },
    };
  }
  if (type === "icon") {
    const icon = readRecord(record.icon, `${label}.icon`);
    const name = readString(icon.name, `${label}.icon.name`);
    if (!name.trim()) throw new Error(`${label}.icon.name must not be empty`);
    return {
      type,
      icon: {
        name,
        ...(icon.color === undefined ? {} : { color: readNotesIconColor(icon.color, `${label}.icon.color`) }),
      },
    };
  }
  if (type === "external") {
    const external = readRecord(record.external, `${label}.external`);
    const url = readDisplayString(external.url, `${label}.external.url`);
    if (!isSupportedExternalPageIconUrl(url)) {
      throw new Error(`${label}.external.url must be a supported HTTPS image URL`);
    }
    return { type, external: { url } };
  }
  if (type === "file") {
    const file = readRecord(record.file, `${label}.file`);
    const url = readDisplayString(file.url, `${label}.file.url`);
    const expiryTime = readOptionalDisplayString(file.expiry_time, `${label}.file.expiry_time`);
    const name = readOptionalDisplayString(file.name, `${label}.file.name`);
    const assetPath = readOptionalDisplayString(file.ganbaru_asset_path, `${label}.file.ganbaru_asset_path`);
    const contentType = readOptionalDisplayString(file.content_type, `${label}.file.content_type`);
    const sha256 = readOptionalDisplayString(file.sha256, `${label}.file.sha256`);
    const byteSize = file.byte_size === undefined ? undefined : readInteger(file.byte_size, `${label}.file.byte_size`);
    if (assetPath !== undefined) {
      if (!isNotesPageIconAssetPath(assetPath)) {
        throw new Error(`${label}.file.ganbaru_asset_path must stay under a managed image asset directory`);
      }
      if (url !== `ganbaru-asset:${assetPath}`) {
        throw new Error(`${label}.file.url must reference the managed icon asset path`);
      }
      if (contentType !== "image/png" && contentType !== "image/jpeg" && contentType !== "image/webp") {
        throw new Error(`${label}.file.content_type must be a supported local image type`);
      }
      if (byteSize === undefined || byteSize <= 0) {
        throw new Error(`${label}.file.byte_size must be positive`);
      }
      if (sha256 === undefined || !/^[a-f0-9]{64}$/.test(sha256)) {
        throw new Error(`${label}.file.sha256 must be a lowercase SHA-256 hex digest`);
      }
    } else if (url.startsWith("ganbaru-asset:")) {
      throw new Error(`${label}.file.url must include managed asset metadata`);
    } else if (!isSupportedExternalPageIconUrl(url)) {
      throw new Error(`${label}.file.url must be a supported HTTPS image URL`);
    }
    return {
      type,
      file: {
        url,
        ...(expiryTime === undefined ? {} : { expiry_time: expiryTime }),
        ...(name === undefined ? {} : { name }),
        ...(contentType === undefined ? {} : { content_type: contentType as "image/png" | "image/jpeg" | "image/webp" }),
        ...(byteSize === undefined ? {} : { byte_size: byteSize }),
        ...(sha256 === undefined ? {} : { sha256 }),
        ...(assetPath === undefined ? {} : { ganbaru_asset_path: assetPath }),
      },
    };
  }
  throw new Error(`${label}.type must be a supported Notion icon type`);
}

export function parseMediaPayload(
  value: unknown,
  label: string,
  blockType: NotesMediaBlockType,
): NotesMediaBlockPayload {
  const record = readRecord(value, label);
  const caption = record.caption === undefined
    ? []
    : parseNotesRichTextArray(record.caption, `${label}.caption`);
  if (blockType === "file" && record.caption === undefined) {
    throw new Error(`${label}.caption must be a rich text array`);
  }
  const name = record.name === undefined ? undefined : readString(record.name, `${label}.name`);
  if (name !== undefined && containsControlCharacters(name)) {
    throw new Error(`${label}.name must not contain control characters`);
  }
  return {
    caption,
    ...(name === undefined ? {} : { name }),
    ...parseFileObject(record, label, blockType, true),
  };
}

function parseFileObject(
  value: unknown,
  label: string,
  mediaType: NotesMediaBlockType,
  allowBlankExternalUrl = false,
): NotesFileObject {
  const record = readRecord(value, label);
  const fileType = readString(record.type, `${label}.type`);
  if (fileType === "external") {
    const external = readRecord(record.external, `${label}.external`);
    const url = readString(external.url, `${label}.external.url`);
    if ((!allowBlankExternalUrl || url.trim()) && !externalMediaUrlIsSupported(mediaType, url)) {
      throw new Error(`${label}.external.url must be a supported HTTPS ${mediaType} URL`);
    }
    return { type: "external", external: { url } };
  }
  if (fileType === "file") {
    const file = readRecord(record.file, `${label}.file`);
    const url = readString(file.url, `${label}.file.url`);
    const assetPath = readOptionalDisplayString(file.ganbaru_asset_path, `${label}.file.ganbaru_asset_path`);
    const fileName = readOptionalDisplayString(file.name, `${label}.file.name`);
    const contentType = readOptionalDisplayString(file.content_type, `${label}.file.content_type`);
    const byteSize = file.byte_size === undefined ? undefined : readInteger(file.byte_size, `${label}.file.byte_size`);
    const sha256 = readOptionalDisplayString(file.sha256, `${label}.file.sha256`);
    if (assetPath !== undefined) {
      if (!isNotesFileAssetPath(assetPath)) {
        throw new Error(`${label}.file.ganbaru_asset_path must stay under the managed Notes file directory`);
      }
      if (url !== `ganbaru-asset:${assetPath}`) {
        throw new Error(`${label}.file.url must reference the managed file asset path`);
      }
      if (contentType === undefined || !localMediaContentTypeMatchesBlock(mediaType, contentType)) {
        throw new Error(`${label}.file.content_type must match the local media block type`);
      }
      if (byteSize === undefined || byteSize <= 0) {
        throw new Error(`${label}.file.byte_size must be positive`);
      }
      if (sha256 === undefined || !/^[a-f0-9]{64}$/.test(sha256)) {
        throw new Error(`${label}.file.sha256 must be a lowercase SHA-256 hex digest`);
      }
      return {
        type: "file",
        file: {
          url,
          ...(fileName === undefined ? {} : { name: fileName }),
          content_type: contentType,
          byte_size: byteSize,
          sha256,
          ganbaru_asset_path: assetPath,
        },
      };
    }
    if (url.startsWith("ganbaru-asset:")) {
      throw new Error(`${label}.file.url must include managed asset metadata`);
    }
    const expiryTime = readString(file.expiry_time, `${label}.file.expiry_time`);
    if (!externalMediaUrlIsSupported(mediaType, url)) {
      throw new Error(`${label}.file.url must be a supported HTTPS ${mediaType} URL`);
    }
    if (containsControlCharacters(expiryTime)) {
      throw new Error(`${label}.file.expiry_time must not contain control characters`);
    }
    return { type: "file", file: { url, expiry_time: expiryTime } };
  }
  if (fileType === "file_upload") {
    const fileUpload = readRecord(record.file_upload, `${label}.file_upload`);
    const id = readString(fileUpload.id, `${label}.file_upload.id`);
    if (!UUID_PATTERN.test(id)) {
      throw new Error(`${label}.file_upload.id must be a UUID`);
    }
    return { type: "file_upload", file_upload: { id } };
  }
  throw new Error(`${label}.type must be file, external, or file_upload`);
}

function isNotesFileAssetPath(path: string): boolean {
  const remainder = path.trim().startsWith("notes/files/")
    ? path.trim().slice("notes/files/".length)
    : "";
  return Boolean(remainder)
    && !remainder.includes("/")
    && !path.includes("..")
    && !path.includes("\\");
}

function localMediaContentTypeMatchesBlock(
  mediaType: NotesMediaBlockType,
  contentType: string,
): boolean {
  if (mediaType === "image") {
    return contentType === "image/png"
      || contentType === "image/jpeg"
      || contentType === "image/webp";
  }
  if (mediaType === "video") return contentType.startsWith("video/");
  if (mediaType === "audio") return contentType.startsWith("audio/");
  if (mediaType === "pdf") return contentType === "application/pdf";
  return contentType.includes("/")
    && !contentType.includes(" ")
    && !contentType.includes(";")
    && contentType !== "image/svg+xml";
}

/** Validate editable cover metadata without resolving theme colors or flattening image crops. */
export function parseNullablePageCover(value: unknown, label: string): NotesPageCover | null {
  if (value === null) return null;
  const record = readRecord(value, label);
  if (record.type === "design") {
    if (record.focal_point !== undefined) throw new Error(`${label}: designed covers cannot have an image focal point`);
    const design = readRecord(record.design, `${label}.design`);
    const pattern = NOTES_COVER_DESIGNS.find((candidate) => candidate === design.pattern);
    if (!pattern) throw new Error(`${label}.design.pattern is unsupported`);
    return createNotesDesignCover(pattern, design.color === "default" ? "default" : readInteger(design.color, `${label}.design.color`));
  }
  const cover = parsePageCoverFileObject(value, label);
  if (record.focal_point === undefined) return cover;
  const focal = readRecord(record.focal_point, `${label}.focal_point`);
  const coordinate = (value: unknown): number => {
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) {
      throw new Error(`${label}.focal_point coordinates must be between 0 and 1`);
    }
    return value;
  };
  return { ...cover, focal_point: { x: coordinate(focal.x), y: coordinate(focal.y) } };
}

function parsePageCoverFileObject(value: unknown, label: string): Exclude<NotesPageCover, { type: "design" }> {
  const record = readRecord(value, label);
  const type = readString(record.type, `${label}.type`);
  if (type === "external") {
    const external = readRecord(record.external, `${label}.external`);
    const url = readDisplayString(external.url, `${label}.external.url`);
    if (!isSupportedExternalPageCoverUrl(url)) {
      throw new Error(`${label}.external.url must be a supported HTTPS image URL`);
    }
    return { type, external: { url } };
  }
  if (type === "file") {
    const file = readRecord(record.file, `${label}.file`);
    const url = readDisplayString(file.url, `${label}.file.url`);
    const expiryTime = readOptionalDisplayString(file.expiry_time, `${label}.file.expiry_time`);
    const name = readOptionalDisplayString(file.name, `${label}.file.name`);
    const assetPath = readOptionalDisplayString(file.ganbaru_asset_path, `${label}.file.ganbaru_asset_path`);
    const contentType = readOptionalDisplayString(file.content_type, `${label}.file.content_type`);
    const sha256 = readOptionalDisplayString(file.sha256, `${label}.file.sha256`);
    const byteSize = file.byte_size === undefined ? undefined : readInteger(file.byte_size, `${label}.file.byte_size`);
    if (assetPath !== undefined) {
      if (!isNotesPageCoverAssetPath(assetPath)) {
        throw new Error(`${label}.file.ganbaru_asset_path must stay under a managed image asset directory`);
      }
      if (url !== `ganbaru-asset:${assetPath}`) {
        throw new Error(`${label}.file.url must reference the managed cover asset path`);
      }
      if (contentType !== "image/png" && contentType !== "image/jpeg" && contentType !== "image/webp") {
        throw new Error(`${label}.file.content_type must be a supported local image type`);
      }
      if (byteSize === undefined || byteSize <= 0) {
        throw new Error(`${label}.file.byte_size must be positive`);
      }
      if (sha256 === undefined || !/^[a-f0-9]{64}$/.test(sha256)) {
        throw new Error(`${label}.file.sha256 must be a lowercase SHA-256 hex digest`);
      }
    } else if (url.startsWith("ganbaru-asset:")) {
      throw new Error(`${label}.file.url must include managed asset metadata`);
    } else {
      if (!isSupportedExternalPageCoverUrl(url)) {
        throw new Error(`${label}.file.url must be a supported HTTPS image URL`);
      }
      if (expiryTime === undefined) {
        throw new Error(`${label}.file.expiry_time must be a string`);
      }
    }
    return {
      type,
      file: {
        url,
        ...(expiryTime === undefined ? {} : { expiry_time: expiryTime }),
        ...(name === undefined ? {} : { name }),
        ...(contentType === undefined ? {} : { content_type: contentType as "image/png" | "image/jpeg" | "image/webp" }),
        ...(byteSize === undefined ? {} : { byte_size: byteSize }),
        ...(sha256 === undefined ? {} : { sha256 }),
        ...(assetPath === undefined ? {} : { ganbaru_asset_path: assetPath }),
      },
    };
  }
  if (type === "file_upload") {
    const fileUpload = readRecord(record.file_upload, `${label}.file_upload`);
    const id = readString(fileUpload.id, `${label}.file_upload.id`);
    if (!UUID_PATTERN.test(id)) {
      throw new Error(`${label}.file_upload.id must be a UUID`);
    }
    return { type, file_upload: { id } };
  }
  throw new Error(`${label}.type must be file, external, or file_upload`);
}

export function parseBookmarkPayload(value: unknown, label: string): NotesBookmarkBlockPayload {
  const record = readRecord(value, label);
  return {
    caption: parseNotesRichTextArray(record.caption, `${label}.caption`),
    url: readString(record.url, `${label}.url`),
  };
}

export function parseEmbedPayload(value: unknown, label: string): NotesEmbedBlockPayload {
  const record = readRecord(value, label);
  return {
    url: readString(record.url, `${label}.url`),
  };
}

export function parseLinkPreviewPayload(value: unknown, label: string): NotesLinkPreviewBlockPayload {
  const record = readRecord(value, label);
  const url = readString(record.url, `${label}.url`);
  if (containsControlCharacters(url)) {
    throw new Error(`${label}.url must not contain control characters`);
  }
  return { url };
}

export function isNotesGalleryCoverSource(value: unknown): value is NotesDatabaseGalleryCoverSource {
  return value === "page_cover" || value === "files_property" || value === "none";
}
