/** Returns the first grapheme-like character from each of the first two name words. */
export function profileInitials(displayName: string): string {
  const initials = displayName
    .trim()
    .split(/\s+/u)
    .slice(0, 2)
    .map((part) => Array.from(part)[0] ?? "")
    .join("")
    .toLocaleUpperCase();
  return initials || "?";
}

/**
 * Square crop of a profile image. `x` and `y` are the crop center in normalized image
 * coordinates, and `zoom` multiplies the scale that fills the square with the image.
 */
export interface ProfileImageCrop {
  x: number;
  y: number;
  zoom: number;
}

/** Natural pixel size of a profile image. */
export interface ProfileImageSize {
  width: number;
  height: number;
}

/** Image placement as fractions of the square avatar side. */
export interface ProfileImageCropLayout {
  width: number;
  height: number;
  left: number;
  top: number;
}

export const PROFILE_IMAGE_MIN_ZOOM = 1;
export const PROFILE_IMAGE_MAX_ZOOM = 4;
export const PROFILE_IMAGE_DEFAULT_CROP: Readonly<ProfileImageCrop> = { x: 0.5, y: 0.5, zoom: 1 };

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function isUnitInterval(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;
}

/** Validate a stored crop before it reaches rendering. */
export function isProfileImageCrop(value: unknown): value is ProfileImageCrop {
  if (typeof value !== "object" || value === null) return false;
  const { x, y, zoom } = value as Record<string, unknown>;
  return isUnitInterval(x)
    && isUnitInterval(y)
    && typeof zoom === "number"
    && Number.isFinite(zoom)
    && zoom >= PROFILE_IMAGE_MIN_ZOOM
    && zoom <= PROFILE_IMAGE_MAX_ZOOM;
}

/** Return true when two crops place the picture identically. */
export function profileImageCropsEqual(a: ProfileImageCrop, b: ProfileImageCrop): boolean {
  return a.x === b.x && a.y === b.y && a.zoom === b.zoom;
}

/** Return true when a crop renders the same as the centered, unzoomed default. */
export function isDefaultProfileImageCrop(crop: ProfileImageCrop): boolean {
  return profileImageCropsEqual(crop, PROFILE_IMAGE_DEFAULT_CROP);
}

/** Size of the zoomed image relative to the avatar side; the shorter edge fills it at zoom 1. */
function scaledImageExtent(image: ProfileImageSize, zoom: number): ProfileImageSize | null {
  if (!(image.width > 0) || !(image.height > 0)) return null;
  const fill = zoom / Math.min(image.width, image.height);
  return { width: image.width * fill, height: image.height * fill };
}

/** Keep the crop center far enough from each edge that the square stays covered. */
function clampCenter(center: number, extent: number): number {
  const half = 0.5 / extent;
  return clamp(center, half, 1 - half);
}

/** Clamp zoom to its range and the center so the image always covers the square. */
export function clampProfileImageCrop(crop: ProfileImageCrop, image: ProfileImageSize): ProfileImageCrop {
  const zoom = clamp(crop.zoom, PROFILE_IMAGE_MIN_ZOOM, PROFILE_IMAGE_MAX_ZOOM);
  const extent = scaledImageExtent(image, zoom);
  if (!extent) return { x: clamp(crop.x, 0, 1), y: clamp(crop.y, 0, 1), zoom };
  return { x: clampCenter(crop.x, extent.width), y: clampCenter(crop.y, extent.height), zoom };
}

/** Place the image inside a square avatar so the crop center sits at the square center. */
export function profileImageCropLayout(
  crop: ProfileImageCrop,
  image: ProfileImageSize,
): ProfileImageCropLayout | null {
  const clamped = clampProfileImageCrop(crop, image);
  const extent = scaledImageExtent(image, clamped.zoom);
  if (!extent) return null;
  return {
    width: extent.width,
    height: extent.height,
    left: 0.5 - clamped.x * extent.width,
    top: 0.5 - clamped.y * extent.height,
  };
}

/**
 * Move the image with a pointer. `delta` is measured in avatar sides, so dragging right by
 * one full side moves the visible image right by that distance.
 */
export function profileImageCropFromDrag(
  crop: ProfileImageCrop,
  delta: { x: number; y: number },
  image: ProfileImageSize,
): ProfileImageCrop {
  const extent = scaledImageExtent(image, crop.zoom);
  if (!extent) return clampProfileImageCrop(crop, image);
  return clampProfileImageCrop(
    { x: crop.x - delta.x / extent.width, y: crop.y - delta.y / extent.height, zoom: crop.zoom },
    image,
  );
}

/** Change zoom around the current crop center, clamping the center at the new scale. */
export function profileImageCropWithZoom(
  crop: ProfileImageCrop,
  zoom: number,
  image: ProfileImageSize,
): ProfileImageCrop {
  return clampProfileImageCrop({ ...crop, zoom }, image);
}
