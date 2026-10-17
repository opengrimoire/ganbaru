import { describe, expect, it } from "vitest";
import { notesCommentAttachmentFile, notesCommentAttachmentFromAsset } from "./comment-attachments";

const assetPath = "notes/files/f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1.png";

describe("comment attachments", () => {
  it("stores a managed file in the shape the backend references", () => {
    expect(notesCommentAttachmentFromAsset({
      relativePath: assetPath,
      originalName: " diagram.png ",
      contentType: "image/png",
      byteSize: 42,
      sha256: "f1",
      kind: "image",
    })).toEqual({
      category: "file",
      name: "diagram.png",
      file: {
        url: `ganbaru-asset:${assetPath}`,
        name: "diagram.png",
        content_type: "image/png",
        byte_size: 42,
        sha256: "f1",
        ganbaru_asset_path: assetPath,
      },
    });
  });

  it("names an unnamed asset after its file", () => {
    const stored = notesCommentAttachmentFromAsset({
      relativePath: assetPath,
      originalName: null,
      contentType: "image/png",
      byteSize: 42,
      sha256: "f1",
      kind: "image",
    });

    expect(stored.name).toBe(assetPath.slice("notes/files/".length));
  });

  it("reads managed files back for rendering", () => {
    const stored = notesCommentAttachmentFromAsset({
      relativePath: assetPath,
      originalName: "diagram.png",
      contentType: "image/png",
      byteSize: 42,
      sha256: "f1",
      kind: "image",
    });

    expect(notesCommentAttachmentFile(stored)).toEqual({
      name: "diagram.png",
      contentType: "image/png",
      byteSize: 42,
      assetPath,
      externalUrl: null,
      isImage: true,
    });
  });

  it("reads imported web references and rejects unsafe or foreign payloads", () => {
    expect(notesCommentAttachmentFile({
      category: "productivity",
      file: { url: "https://example.com/brief.pdf" },
    })).toMatchObject({ name: "brief.pdf", externalUrl: "https://example.com/brief.pdf", assetPath: null, isImage: false });
    expect(notesCommentAttachmentFile({ file: { url: "javascript:alert(1)" } })).toBeNull();
    expect(notesCommentAttachmentFile({ file: { ganbaru_asset_path: "assets/profile/a.png", url: "ganbaru-asset:assets/profile/a.png" } })).toBeNull();
    expect(notesCommentAttachmentFile({ type: "file_upload", file_upload: { id: "x" } })).toBeNull();
    expect(notesCommentAttachmentFile({ name: "missing" })).toBeNull();
  });
});
