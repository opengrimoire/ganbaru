import { describe, expect, it } from "vitest";
import { clipboardImageFiles, clipboardMayHoldNativeImage, clipboardPrefersImages } from "./clipboard-images";

interface FakeItem {
  kind: string;
  type: string;
  getAsFile(): File | null;
}

function transfer(files: File[], items: FakeItem[] = []): Pick<DataTransfer, "files" | "items"> {
  return {
    files: files as unknown as FileList,
    items: items as unknown as DataTransferItemList,
  };
}

function textData(data: Record<string, string>): Pick<DataTransfer, "getData"> {
  return { getData: (format: string) => data[format] ?? "" };
}

describe("clipboardImageFiles", () => {
  it("returns nothing without a clipboard", () => {
    expect(clipboardImageFiles(null)).toEqual([]);
  });

  it("keeps named image files and skips other files", () => {
    const photo = new File([new Uint8Array([1])], "photo.jpg", { type: "image/jpeg" });
    const text = new File(["hello"], "notes.txt", { type: "text/plain" });
    expect(clipboardImageFiles(transfer([photo, text]))).toEqual([photo]);
  });

  it("names unnamed images by their type", () => {
    const unnamed = new File([new Uint8Array([1])], "", { type: "image/webp" });
    const [named] = clipboardImageFiles(transfer([unnamed]));
    expect(named?.name).toBe("pasted-image.webp");
    expect(named?.type).toBe("image/webp");
  });

  it("reads items when the file list is empty and uses the advertised type", () => {
    const untyped = new File([new Uint8Array([1])], "");
    const files = clipboardImageFiles(transfer([], [
      { kind: "string", type: "text/plain", getAsFile: () => null },
      { kind: "file", type: "image/png", getAsFile: () => untyped },
    ]));
    expect(files.map((entry) => [entry.name, entry.type])).toEqual([["pasted-image.png", "image/png"]]);
  });

  it("does not count an image twice when it appears as a file and an item", () => {
    const photo = new File([new Uint8Array([1])], "photo.png", { type: "image/png" });
    const files = clipboardImageFiles(transfer([photo], [{ kind: "file", type: "image/png", getAsFile: () => photo }]));
    expect(files).toEqual([photo]);
  });
});

describe("clipboardPrefersImages", () => {
  it("needs at least one image", () => {
    expect(clipboardPrefersImages(textData({}), 0)).toBe(false);
  });

  it("prefers a copied image with no text or only its address", () => {
    expect(clipboardPrefersImages(textData({}), 1)).toBe(true);
    expect(clipboardPrefersImages(textData({ "text/plain": " https://example.com/cat.png " }), 1)).toBe(true);
  });

  it("prefers an image whose markup holds nothing but the image", () => {
    const html = "<meta charset=\"utf-8\"><img src=\"https://example.com/cat.png\" alt=\"Cat\">&nbsp;";
    expect(clipboardPrefersImages(textData({ "text/plain": "Cat", "text/html": html }), 1)).toBe(true);
  });

  it("keeps copied text that comes with a picture of itself", () => {
    expect(clipboardPrefersImages(textData({ "text/plain": "Quarterly totals\n12 15 18" }), 1)).toBe(false);
    const html = "<p>Quarterly totals</p><img src=\"chart.png\">";
    expect(clipboardPrefersImages(textData({ "text/plain": "Quarterly totals", "text/html": html }), 1)).toBe(false);
  });

  it("ignores script and style content when reading the markup", () => {
    const html = "<style>p { color: red }</style><img src=\"cat.png\"><script>alert(1)</script>";
    expect(clipboardPrefersImages(textData({ "text/plain": "cat", "text/html": html }), 1)).toBe(true);
  });
});

describe("clipboardMayHoldNativeImage", () => {
  it("asks the system clipboard when the paste carries no text", () => {
    expect(clipboardMayHoldNativeImage(textData({}))).toBe(true);
  });

  it("asks the system clipboard when the markup holds only an image", () => {
    expect(clipboardMayHoldNativeImage(textData({ "text/html": "<meta charset=\"utf-8\"><img src=\"cat.png\">" }))).toBe(true);
  });

  it("keeps any copied text, including a lone address or whitespace", () => {
    expect(clipboardMayHoldNativeImage(textData({ "text/plain": "https://example.com/cat.png" }))).toBe(false);
    expect(clipboardMayHoldNativeImage(textData({ "text/plain": " " }))).toBe(false);
    expect(clipboardMayHoldNativeImage(textData({ "text/html": "<p>Totals</p>" }))).toBe(false);
    expect(clipboardMayHoldNativeImage(textData({ "text/html": "<p>Totals</p><img src=\"chart.png\">" }))).toBe(false);
  });
});
