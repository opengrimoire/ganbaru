/** Asset class of managed Music artwork, as the sync manifest declares it. */
export const MUSIC_ARTWORK_ASSET_CLASS = "music/artwork";

/** File extensions the artwork class stores, one per supported content type. */
const MUSIC_ARTWORK_EXTENSIONS = ["png", "jpg", "webp", "gif"] as const;

const MANAGED_ARTWORK_REFERENCE = new RegExp(
  `^${MUSIC_ARTWORK_ASSET_CLASS}/[0-9a-f]{64}\\.(?:${MUSIC_ARTWORK_EXTENSIONS.join("|")})$`,
);

/**
 * Whether a value is a managed artwork reference, `music/artwork/{sha256}.{ext}`, which names a
 * vault asset rather than a device path. The native side checks the file again before reading it.
 */
export function isManagedMusicArtworkReference(value: string | null | undefined): value is string {
  return typeof value === "string" && MANAGED_ARTWORK_REFERENCE.test(value);
}
