import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { invalidateAssetUrl, loadAssetUrl } from "$lib/api/asset-url-cache";
import type { NotesPageCoverAssetMetadata } from "$lib/notes/pages/cover";

interface NotesPageCoverAssetDto {
  relativePath: string;
  originalName: string | null;
  contentType: string;
  byteSize: number;
  sha256: string;
}

function mapNotesPageCoverAssetDto(value: NotesPageCoverAssetDto): NotesPageCoverAssetMetadata {
  return {
    relativePath: value.relativePath,
    originalName: value.originalName,
    contentType: value.contentType,
    byteSize: value.byteSize,
    sha256: value.sha256,
  };
}

/** Open a native file picker and copy the selected image into managed Notes page cover assets. */
export async function pickNotesPageCoverImageFile(): Promise<NotesPageCoverAssetMetadata | null> {
  const dbUrl = await ensureDbUrl();
  const asset = await invoke<NotesPageCoverAssetDto | null>("notes_pick_page_cover_file", { dbUrl });
  return asset ? mapNotesPageCoverAssetDto(asset) : null;
}

/** Save an uploaded or pasted image data URL into managed Notes page cover assets. */
export async function saveNotesPageCoverImageDataUrl(
  dataUrl: string,
  originalName?: string | null,
): Promise<NotesPageCoverAssetMetadata> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageCoverAssetDto(
    await invoke<NotesPageCoverAssetDto>("notes_save_page_cover_data_url", {
      dbUrl,
      dataUrl,
      originalName: originalName ?? null,
    }),
  );
}

/** Load a managed Notes page cover asset as a data URL for local rendering. */
export async function notesPageCoverAssetUrl(relativePath: string): Promise<string> {
  return loadAssetUrl("notes-page-cover", relativePath, async () => {
    const dbUrl = await ensureDbUrl();
    return invoke<string>("notes_page_cover_asset_data_url", { dbUrl, relativePath });
  });
}

/** Invalidates a managed Notes page cover after replacement or removal. */
export function invalidateNotesPageCoverAssetUrl(relativePath: string): void {
  invalidateAssetUrl("notes-page-cover", relativePath);
}
