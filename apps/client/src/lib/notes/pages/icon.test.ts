import { describe, expect, it } from "vitest";
import {
  createNotesCustomEmojiPageIcon,
  createNotesEmojiPageIcon,
  createNotesExternalPageIcon,
  createNotesLocalFilePageIcon,
  createNotesNativePageIcon,
  notesPageIconAssetPath,
  notesPageIconExternalUrl,
  notesPageIconLabel,
  notesPageIconText,
} from "./icon";

describe("notes page icons", () => {
  it("creates an emoji icon payload", () => {
    expect(createNotesEmojiPageIcon(" 📌 ")).toEqual({ type: "emoji", emoji: "📌" });
  });

  it("rejects blank emoji icons", () => {
    expect(() => createNotesEmojiPageIcon(" ")).toThrow("page icon emoji must not be empty");
  });

  it("creates native icon, custom emoji, external image, and local file payloads", () => {
    expect(createNotesNativePageIcon(" home ", "blue")).toEqual({
      type: "icon",
      icon: { name: "home", color: "blue" },
    });
    expect(
      createNotesCustomEmojiPageIcon({
        id: " emoji-a ",
        name: " Focus ",
        assetPath: "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
      }),
    ).toEqual({
      type: "custom_emoji",
      custom_emoji: {
        id: "emoji-a",
        name: "Focus",
        url: "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        ganbaru_asset_path: "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
      },
    });
    expect(createNotesExternalPageIcon(" https://example.com/icon.webp ")).toEqual({
      type: "external",
      external: { url: "https://example.com/icon.webp" },
    });
    expect(
      createNotesLocalFilePageIcon({
        relativePath: "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
        originalName: " icon.png ",
        contentType: "image/png",
        byteSize: 42,
        sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      }),
    ).toEqual({
      type: "file",
      file: {
        url: "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
        name: "icon.png",
        content_type: "image/png",
        byte_size: 42,
        sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ganbaru_asset_path: "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
      },
    });
  });

  it("rejects unsafe external and local file icons", () => {
    expect(() =>
      createNotesCustomEmojiPageIcon({
        id: "emoji-a",
        assetPath: "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
      }),
    ).toThrow("page custom emoji asset path must stay under project-icons");
    expect(() =>
      createNotesCustomEmojiPageIcon({
        id: "emoji-a",
        url: "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
      }),
    ).toThrow("page custom emoji managed URL must include an asset path");
    expect(() => createNotesExternalPageIcon("http://example.com/icon.png")).toThrow(
      "page icon URL must be a supported HTTPS image URL",
    );
    expect(() => createNotesExternalPageIcon("https://example.com/icon.svg")).toThrow(
      "page icon URL must be a supported HTTPS image URL",
    );
    expect(() =>
      createNotesLocalFilePageIcon({
        relativePath: "notes/page-icons/bad.svg",
        contentType: "image/svg+xml",
        byteSize: 42,
        sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      }),
    ).toThrow("page icon asset path must stay under notes/page-icons");
  });

  it("returns display text only for emoji icons", () => {
    expect(notesPageIconText({ type: "emoji", emoji: "📌" })).toBe("📌");
    expect(notesPageIconText({ type: "external", external: { url: "https://example.com/a.png" } })).toBeNull();
    expect(notesPageIconText(null)).toBeNull();
  });

  it("returns render source metadata for image icons", () => {
    expect(notesPageIconAssetPath({
      type: "file",
      file: {
        url: "ganbaru-asset:notes/page-icons/a.png",
        ganbaru_asset_path: "notes/page-icons/a.png",
      },
    })).toBe("notes/page-icons/a.png");
    expect(notesPageIconExternalUrl({ type: "external", external: { url: "https://example.com/a.png" } })).toBe(
      "https://example.com/a.png",
    );
    expect(notesPageIconLabel({ type: "icon", icon: { name: "home" } })).toBe("home");
  });
});
