import { describe, expect, it } from "vitest";
import {
  clampProfileImageCrop,
  isDefaultProfileImageCrop,
  isProfileImageCrop,
  PROFILE_IMAGE_DEFAULT_CROP,
  PROFILE_IMAGE_MAX_ZOOM,
  profileImageCropFromDrag,
  profileImageCropLayout,
  profileImageCropsEqual,
  profileImageCropWithZoom,
  profileInitials,
} from "./identity";

const LANDSCAPE = { width: 400, height: 200 };
const PORTRAIT = { width: 300, height: 600 };
const SQUARE = { width: 500, height: 500 };

describe("profile identity", () => {
  it("uses at most the first two name words for initials", () => {
    expect(profileInitials("You")).toBe("Y");
    expect(profileInitials("Alice Rivera")).toBe("AR");
    expect(profileInitials("  Ana Maria Lopez  ")).toBe("AM");
    expect(profileInitials("")).toBe("?");
  });
});

describe("profile image crop", () => {
  it("accepts only centers inside the image and zoom inside the supported range", () => {
    expect(isProfileImageCrop({ x: 0, y: 1, zoom: 1 })).toBe(true);
    expect(isProfileImageCrop({ x: 0.3, y: 0.7, zoom: PROFILE_IMAGE_MAX_ZOOM })).toBe(true);
    expect(isProfileImageCrop({ x: -0.1, y: 0.5, zoom: 1 })).toBe(false);
    expect(isProfileImageCrop({ x: 0.5, y: 1.2, zoom: 1 })).toBe(false);
    expect(isProfileImageCrop({ x: 0.5, y: 0.5, zoom: 0.5 })).toBe(false);
    expect(isProfileImageCrop({ x: 0.5, y: 0.5, zoom: PROFILE_IMAGE_MAX_ZOOM + 1 })).toBe(false);
    expect(isProfileImageCrop({ x: 0.5, y: 0.5, zoom: Number.NaN })).toBe(false);
    expect(isProfileImageCrop({ x: "0.5", y: 0.5, zoom: 1 })).toBe(false);
    expect(isProfileImageCrop(null)).toBe(false);
    expect(isProfileImageCrop([0.5, 0.5, 1])).toBe(false);
  });

  it("fills the square with the shorter image edge and centers the default crop", () => {
    expect(profileImageCropLayout(PROFILE_IMAGE_DEFAULT_CROP, LANDSCAPE)).toEqual({
      width: 2,
      height: 1,
      left: -0.5,
      top: 0,
    });
    expect(profileImageCropLayout(PROFILE_IMAGE_DEFAULT_CROP, PORTRAIT)).toEqual({
      width: 1,
      height: 2,
      left: 0,
      top: -0.5,
    });
    expect(profileImageCropLayout(PROFILE_IMAGE_DEFAULT_CROP, SQUARE)).toEqual({
      width: 1,
      height: 1,
      left: 0,
      top: 0,
    });
  });

  it("returns no layout until the natural image size is known", () => {
    expect(profileImageCropLayout(PROFILE_IMAGE_DEFAULT_CROP, { width: 0, height: 0 })).toBeNull();
  });

  it("keeps the square covered when the stored center is too close to an edge", () => {
    expect(clampProfileImageCrop({ x: 0, y: 0.5, zoom: 1 }, LANDSCAPE)).toEqual({ x: 0.25, y: 0.5, zoom: 1 });
    expect(clampProfileImageCrop({ x: 1, y: 0.5, zoom: 1 }, LANDSCAPE)).toEqual({ x: 0.75, y: 0.5, zoom: 1 });
    expect(clampProfileImageCrop({ x: 0.1, y: 0.9, zoom: 1 }, SQUARE)).toEqual({ x: 0.5, y: 0.5, zoom: 1 });
    expect(profileImageCropLayout({ x: 0, y: 0, zoom: 1 }, LANDSCAPE)).toEqual({
      width: 2,
      height: 1,
      left: 0,
      top: 0,
    });
  });

  it("moves the image with the pointer and stops at the image edges", () => {
    const right = profileImageCropFromDrag(PROFILE_IMAGE_DEFAULT_CROP, { x: 0.25, y: 0 }, LANDSCAPE);
    expect(right).toEqual({ x: 0.375, y: 0.5, zoom: 1 });
    expect(profileImageCropLayout(right, LANDSCAPE)?.left).toBeCloseTo(-0.25);

    expect(profileImageCropFromDrag(PROFILE_IMAGE_DEFAULT_CROP, { x: 5, y: 0 }, LANDSCAPE).x).toBe(0.25);
    expect(profileImageCropFromDrag(PROFILE_IMAGE_DEFAULT_CROP, { x: -5, y: 0 }, LANDSCAPE).x).toBe(0.75);
    expect(profileImageCropFromDrag(PROFILE_IMAGE_DEFAULT_CROP, { x: 0, y: 0.4 }, LANDSCAPE).y).toBe(0.5);
  });

  it("allows movement on both axes once zoomed past the fill scale", () => {
    const zoomed = profileImageCropWithZoom(PROFILE_IMAGE_DEFAULT_CROP, 2, SQUARE);
    expect(zoomed).toEqual({ x: 0.5, y: 0.5, zoom: 2 });
    expect(profileImageCropFromDrag(zoomed, { x: -0.5, y: 0.5 }, SQUARE)).toEqual({ x: 0.75, y: 0.25, zoom: 2 });
    expect(profileImageCropLayout({ x: 0.75, y: 0.25, zoom: 2 }, SQUARE)).toEqual({
      width: 2,
      height: 2,
      left: -1,
      top: 0,
    });
  });

  it("clamps zoom to its range and pulls the center back in when zooming out", () => {
    expect(profileImageCropWithZoom(PROFILE_IMAGE_DEFAULT_CROP, 0.2, SQUARE).zoom).toBe(1);
    expect(profileImageCropWithZoom(PROFILE_IMAGE_DEFAULT_CROP, 99, SQUARE).zoom).toBe(PROFILE_IMAGE_MAX_ZOOM);
    expect(profileImageCropWithZoom({ x: 0.85, y: 0.15, zoom: 4 }, 2, SQUARE)).toEqual({ x: 0.75, y: 0.25, zoom: 2 });
  });

  it("recognizes the default crop", () => {
    expect(isDefaultProfileImageCrop({ ...PROFILE_IMAGE_DEFAULT_CROP })).toBe(true);
    expect(isDefaultProfileImageCrop({ x: 0.5, y: 0.5, zoom: 1.5 })).toBe(false);
  });

  it("compares crops by every placement field", () => {
    const crop = { x: 0.3, y: 0.6, zoom: 2 };
    expect(profileImageCropsEqual(crop, { ...crop })).toBe(true);
    expect(profileImageCropsEqual(crop, { ...crop, x: 0.31 })).toBe(false);
    expect(profileImageCropsEqual(crop, { ...crop, y: 0.61 })).toBe(false);
    expect(profileImageCropsEqual(crop, { ...crop, zoom: 2.1 })).toBe(false);
  });
});
