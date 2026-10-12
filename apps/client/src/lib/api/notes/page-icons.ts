import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { invalidateAssetUrl, loadAssetUrl } from "$lib/api/asset-url-cache";
import type { NotesPageIconAssetMetadata } from "$lib/notes/pages/icon";

interface NotesPageIconAssetDto {
  relativePath: string;
  originalName: string | null;
  contentType: string;
  byteSize: number;
  sha256: string;
}

function mapNotesPageIconAssetDto(value: NotesPageIconAssetDto): NotesPageIconAssetMetadata {
  return {
    relativePath: value.relativePath,
    originalName: value.originalName,
    contentType: value.contentType,
    byteSize: value.byteSize,
    sha256: value.sha256,
  };
}

/** Open a native file picker and copy the selected image into managed Notes page icon assets. */
export async function pickNotesPageIconImageFile(): Promise<NotesPageIconAssetMetadata | null> {
  const dbUrl = await ensureDbUrl();
  const asset = await invoke<NotesPageIconAssetDto | null>("notes_pick_page_icon_file", { dbUrl });
  return asset ? mapNotesPageIconAssetDto(asset) : null;
}

/** Save a pasted image data URL into managed Notes page icon assets. */
export async function saveNotesPageIconImageDataUrl(
  dataUrl: string,
  originalName?: string | null,
): Promise<NotesPageIconAssetMetadata> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageIconAssetDto(
    await invoke<NotesPageIconAssetDto>("notes_save_page_icon_data_url", {
      dbUrl,
      dataUrl,
      originalName: originalName ?? null,
    }),
  );
}

/** Load a managed Notes page icon asset as a data URL for local rendering. */
export async function notesPageIconAssetUrl(relativePath: string): Promise<string> {
  return loadAssetUrl("notes-page-icon", relativePath, async () => {
    const dbUrl = await ensureDbUrl();
    return invoke<string>("notes_page_icon_asset_data_url", { dbUrl, relativePath });
  });
}

/** Invalidates a managed Notes page icon after replacement or removal. */
export function invalidateNotesPageIconAssetUrl(relativePath: string): void {
  invalidateAssetUrl("notes-page-icon", relativePath);
}
