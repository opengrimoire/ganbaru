import { afterEach, describe, expect, it, vi } from "vitest";
import { QuickNoteImageError } from "$lib/api/quick-notes";
import {
  appendQuickNoteImages,
  importQuickNoteImageFiles,
  quickNoteImageSlots,
  restoreQuickNoteImage,
} from "./image-import";
import { QUICK_NOTE_IMAGE_LIMIT, QUICK_NOTE_IMAGE_MAX_BYTES, type QuickNoteImage } from "./types";

function image(sha256: string): QuickNoteImage {
  return { sha256, contentType: "image/png", byteSize: 10, width: 40, height: 30 };
}

function images(count: number, prefix = "a"): QuickNoteImage[] {
  return Array.from({ length: count }, (_, index) => image(`${prefix}${index}`));
}

function file(name: string, type: string, bytes: number[] = [1, 2, 3]): File {
  return new File([new Uint8Array(bytes)], name, { type });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Quick note image slots", () => {
  it("counts the images a note can still take while imports run", () => {
    expect(quickNoteImageSlots(0, 0)).toBe(QUICK_NOTE_IMAGE_LIMIT);
    expect(quickNoteImageSlots(20, 3)).toBe(QUICK_NOTE_IMAGE_LIMIT - 23);
    expect(quickNoteImageSlots(QUICK_NOTE_IMAGE_LIMIT, 2)).toBe(0);
  });
});

describe("Appending Quick note images", () => {
  it("adds new images after the current ones and skips ones the note has", () => {
    const next = appendQuickNoteImages([image("a"), image("b")], [image("b"), image("c"), image("c")]);
    expect(next.map((entry) => entry.sha256)).toEqual(["a", "b", "c"]);
  });

  it("stops at the image limit", () => {
    const next = appendQuickNoteImages(images(QUICK_NOTE_IMAGE_LIMIT - 1), images(3, "b"));
    expect(next).toHaveLength(QUICK_NOTE_IMAGE_LIMIT);
    expect(next.at(-1)?.sha256).toBe("b0");
  });
});

describe("Restoring a removed Quick note image", () => {
  it("puts the image back at its old position", () => {
    const next = restoreQuickNoteImage([image("a"), image("c")], image("b"), 1);
    expect(next.map((entry) => entry.sha256)).toEqual(["a", "b", "c"]);
  });

  it("bounds the position to the current images", () => {
    expect(restoreQuickNoteImage([image("a")], image("b"), 9).map((entry) => entry.sha256)).toEqual(["a", "b"]);
    expect(restoreQuickNoteImage([image("a")], image("b"), -4).map((entry) => entry.sha256)).toEqual(["b", "a"]);
  });

  it("leaves the images alone when the note has the image again or is full", () => {
    expect(restoreQuickNoteImage([image("a"), image("b")], image("b"), 0).map((entry) => entry.sha256))
      .toEqual(["a", "b"]);
    expect(restoreQuickNoteImage(images(QUICK_NOTE_IMAGE_LIMIT), image("z"), 0)).toHaveLength(QUICK_NOTE_IMAGE_LIMIT);
  });
});

describe("Importing Quick note image files", () => {
  it("imports files in order and passes their bytes", async () => {
    const importer = vi.fn(async (bytes: Uint8Array) => image(`sha${bytes[0] ?? 0}`));
    const pick = await importQuickNoteImageFiles(
      [file("one.png", "image/png", [1]), file("two.jpg", "image/jpeg", [2])],
      5,
      importer,
    );
    expect(pick).toEqual({ images: [image("sha1"), image("sha2")], rejected: [], skipped: 0 });
    expect(importer).toHaveBeenCalledTimes(2);
  });

  it("rejects unsupported and oversized files before reading them", async () => {
    const oversized = file("huge.png", "image/png");
    Object.defineProperty(oversized, "size", { value: QUICK_NOTE_IMAGE_MAX_BYTES + 1 });
    const importer = vi.fn(async () => image("x"));
    const pick = await importQuickNoteImageFiles([file("scan.bmp", "image/bmp"), oversized], 5, importer);
    expect(pick.rejected).toEqual([
      { fileName: "scan.bmp", code: "unsupported" },
      { fileName: "huge.png", code: "too_large" },
    ]);
    expect(importer).not.toHaveBeenCalled();
  });

  it("leaves untyped files to the native check", async () => {
    const importer = vi.fn(async () => image("x"));
    const pick = await importQuickNoteImageFiles([file("unknown", "")], 1, importer);
    expect(pick.images).toEqual([image("x")]);
  });

  it("reports native error codes and keeps importing the remaining files", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const importer = vi.fn<(bytes: Uint8Array) => Promise<QuickNoteImage>>()
      .mockRejectedValueOnce(new QuickNoteImageError("unsupported", "not an image"))
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(image("ok"));
    const pick = await importQuickNoteImageFiles(
      [file("a.png", "image/png"), file("b.png", "image/png"), file("c.png", "image/png")],
      3,
      importer,
    );
    expect(pick.images).toEqual([image("ok")]);
    expect(pick.rejected).toEqual([
      { fileName: "a.png", code: "unsupported" },
      { fileName: "b.png", code: "failed" },
    ]);
    expect(warn).toHaveBeenCalledOnce();
  });

  it("skips files past the available slots", async () => {
    const importer = vi.fn(async () => image("x"));
    const pick = await importQuickNoteImageFiles(
      [file("a.png", "image/png"), file("b.png", "image/png"), file("c.png", "image/png")],
      1,
      importer,
    );
    expect(pick.skipped).toBe(2);
    expect(importer).toHaveBeenCalledOnce();
    expect((await importQuickNoteImageFiles([file("a.png", "image/png")], -2, importer)).skipped).toBe(1);
  });
});
