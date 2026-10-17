import { describe, expect, it } from "vitest";
import { parseNullablePageCover } from "$lib/notes/validation/assets";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  createNotesExternalPageCover,
  createNotesLocalFilePageCover,
  notesPageCoverAssetPath,
  createNotesDesignCover,
  notesCoverDesignBackground,
  notesCoverObjectPosition,
  notesCoverFocalPointFromDrag,
  notesPageCoverUrl,
  NOTES_COVER_DESIGNS,
} from "./cover";

describe("notes page covers", () => {
  it("creates an external image cover payload", () => {
    expect(createNotesExternalPageCover(" https://example.com/cover.png ")).toEqual({
      type: "external",
      external: { url: "https://example.com/cover.png" },
    });
  });

  it("rejects blank and non-image cover URLs", () => {
    expect(() => createNotesExternalPageCover(" ")).toThrow("page cover URL must not be empty");
    expect(() => createNotesExternalPageCover("https://example.com/file.pdf")).toThrow(
      "page cover URL must be a supported HTTPS image URL",
    );
  });

  it("creates local file cover payloads from managed assets", () => {
    expect(
      createNotesLocalFilePageCover({
        relativePath: "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp",
        originalName: " cover.webp ",
        contentType: "image/webp",
        byteSize: 42,
        sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      }),
    ).toEqual({
      type: "file",
      file: {
        url: "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp",
        name: "cover.webp",
        content_type: "image/webp",
        byte_size: 42,
        sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ganbaru_asset_path: "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp",
      },
    });
  });

  it("rejects unsafe local file cover assets", () => {
    expect(() =>
      createNotesLocalFilePageCover({
        relativePath: "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp",
        contentType: "image/webp",
        byteSize: 42,
        sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      }),
    ).toThrow("page cover asset path must stay under notes/page-covers");
    expect(() =>
      createNotesLocalFilePageCover({
        relativePath: "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp",
        contentType: "image/svg+xml",
        byteSize: 42,
        sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      }),
    ).toThrow("page cover asset must be a PNG, JPG, or WebP image");
  });

  it("returns preview URLs only for renderable cover sources", () => {
    expect(notesPageCoverUrl({ type: "external", external: { url: "https://example.com/cover.jpg" } })).toBe(
      "https://example.com/cover.jpg",
    );
    expect(
      notesPageCoverUrl({
        type: "file",
        file: { url: "https://example.com/cover.jpg", expiry_time: "2026-06-30T12:00:00.000Z" },
      }),
    ).toBe("https://example.com/cover.jpg");
    expect(notesPageCoverUrl({ type: "file_upload", file_upload: { id: "11111111-1111-4111-8111-111111111111" } })).toBeNull();
    expect(
      notesPageCoverAssetPath({
        type: "file",
        file: {
          url: "ganbaru-asset:notes/page-covers/a.png",
          ganbaru_asset_path: "notes/page-covers/a.png",
        },
      }),
    ).toBe("notes/page-covers/a.png");
  });

  it("preserves every editable design and palette identity through validation", () => {
    for (const pattern of NOTES_COVER_DESIGNS) {
      const automatic = createNotesDesignCover(pattern, "default");
      expect(parseNullablePageCover(automatic, "cover")).toEqual(automatic);
      for (let color = 0; color < PALETTE_SIZE; color += 1) {
        const cover = createNotesDesignCover(pattern, color);
        expect(parseNullablePageCover(JSON.parse(JSON.stringify(cover)), "cover")).toEqual(cover);
        expect(notesPageCoverUrl(cover)).toBeNull();
        expect(notesPageCoverAssetPath(cover)).toBeNull();
      }
    }
  });

  it("rejects unknown designs and invalid palette slots rather than replacing them", () => {
    for (const color of [-1, PALETTE_SIZE, 0.5, NaN, Infinity, "2", null]) {
      expect(() => parseNullablePageCover({ type: "design", design: { pattern: "grid", color } }, "cover")).toThrow();
    }
    expect(() => parseNullablePageCover({ type: "design", design: { pattern: "unknown", color: 0 } }, "cover")).toThrow();
  });

  it("derives every cover background from the currently resolved theme color", () => {
    expect(notesCoverDesignBackground("botanical", "#123456")).not.toContain("gradient");
    for (const pattern of NOTES_COVER_DESIGNS) {
      expect(notesCoverDesignBackground(pattern, "#123456")).toContain("#123456");
      expect(notesCoverDesignBackground(pattern, "#abcdef")).not.toContain("#123456");
    }
  });

  it("preserves image focal points and rejects invalid coordinates", () => {
    const image = createNotesExternalPageCover("https://example.com/cover.png");
    const cover = { ...image, focal_point: { x: 0, y: 1 } };
    expect(parseNullablePageCover(cover, "cover")).toEqual(cover);
    for (const point of [null, {}, { x: -0.1, y: 0 }, { x: 0, y: 1.1 }, { x: NaN, y: 0 }, { x: "0", y: 0 }]) {
      expect(() => parseNullablePageCover({ ...image, focal_point: point }, "cover")).toThrow();
    }
    expect(() => parseNullablePageCover({ ...createNotesDesignCover("grid", 0), focal_point: { x: 0, y: 0 } }, "cover")).toThrow();
  });

  it("centers the subject where possible and clamps the crop at image edges", () => {
    const image = { width: 1000, height: 1000 };
    const wide = { width: 1000, height: 200 };
    expect(notesCoverObjectPosition({ x: 0.5, y: 0.3 }, image, wide)).toBe("50% 25%");
    expect(notesCoverObjectPosition({ x: 0.5, y: 0 }, image, wide)).toBe("50% 0%");
    expect(notesCoverObjectPosition({ x: 0.5, y: 1 }, image, wide)).toBe("50% 100%");
    expect(notesCoverObjectPosition({ x: 0.3, y: 0.5 }, image, { width: 200, height: 1000 })).toBe("25% 50%");
    expect(notesCoverObjectPosition({ x: 0, y: 0 }, image, image)).toBe("50% 50%");
    expect(notesCoverObjectPosition({ x: 0, y: 0 }, { width: 0, height: 0 }, wide)).toBe("50% 50%");
  });

  it("moves the visible image with the drag and clamps both edges", () => {
    const image = { width: 1000, height: 1000 };
    const viewport = { width: 1000, height: 200 };
    const focal = { x: 0.5, y: 0.5 };
    expect(notesCoverFocalPointFromDrag(focal, { x: 100, y: 100 }, image, viewport)).toEqual({ x: 0.5, y: 0.4 });
    expect(notesCoverFocalPointFromDrag(focal, { x: 0, y: 2000 }, image, viewport)).toEqual({ x: 0.5, y: 0.1 });
    expect(notesCoverFocalPointFromDrag(focal, { x: 0, y: -2000 }, image, viewport)).toEqual({ x: 0.5, y: 0.9 });
    expect(notesCoverFocalPointFromDrag(focal, { x: 100, y: 0 }, image, { width: 200, height: 1000 })).toEqual({ x: 0.4, y: 0.5 });
  });

  it("starts dragging from the visible edge without jumping or a dead zone", () => {
    const image = { width: 1000, height: 1000 };
    const viewport = { width: 500, height: 100 };
    expect(notesCoverFocalPointFromDrag({ x: 0.5, y: 0 }, { x: 0, y: -10 }, image, viewport)).toEqual({ x: 0.5, y: 0.12 });
    expect(notesCoverFocalPointFromDrag({ x: 0.5, y: 1 }, { x: 0, y: 10 }, image, viewport)).toEqual({ x: 0.5, y: 0.88 });
  });

  it("preserves positioning on uncropped axes and before dimensions are available", () => {
    const focal = { x: 0.2, y: 0.8 };
    const image = { width: 1000, height: 1000 };
    expect(notesCoverFocalPointFromDrag(focal, { x: 50, y: 50 }, image, image)).toEqual(focal);
    expect(notesCoverFocalPointFromDrag(focal, { x: 50, y: 50 }, { width: 0, height: 0 }, image)).toEqual(focal);
    expect(notesCoverFocalPointFromDrag(focal, { x: 0, y: 0 }, image, { width: 500, height: 100 })).toEqual(focal);
  });
});
