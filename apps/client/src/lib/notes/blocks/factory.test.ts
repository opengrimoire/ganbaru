import { describe, expect, it } from "vitest";
import {
  blockUpdateFromBlock,
  blockConvertedToType,
  blockWithHeadingToggleable,
  blockWithMedia,
  createRichText,
  createSyncedBlockPayload,
} from "./factory";
import { createManagedMediaPayload, type NotesFileAssetMetadata } from "$lib/notes/block-types/media";
import type {
  NotesBlock,
  NotesImageBlock,
  NotesPdfBlock,
  NotesRichText,
  NotesRichTextAnnotations,
  NotesSyncedBlock,
} from "$lib/notes/types";

const baseAnnotations: NotesRichTextAnnotations = {
  bold: false,
  italic: false,
  strikethrough: false,
  underline: false,
  code: false,
  color: "default",
};

function richTextFixture(): NotesRichText[] {
  return [
    {
      type: "text",
      text: {
        content: "Docs",
        link: { url: "https://example.com" },
      },
      annotations: {
        ...baseAnnotations,
        bold: true,
        color: "blue",
      },
      plain_text: "Docs",
      href: "https://example.com",
    },
    {
      type: "equation",
      equation: {
        expression: "x+1",
      },
      annotations: baseAnnotations,
      plain_text: "x+1",
      href: null,
    },
  ];
}

function paragraphBlock(richText: NotesRichText[]): NotesBlock {
  return {
    object: "block",
    id: "block-1",
    parent: {
      type: "page_id",
      page_id: "page-1",
    },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "paragraph",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    paragraph: {
      rich_text: richText,
      color: "blue",
    },
  };
}

function fileUploadPdfBlock(): NotesPdfBlock {
  return {
    object: "block",
    id: "block-2",
    parent: {
      type: "page_id",
      page_id: "page-1",
    },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "pdf",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    pdf: {
      type: "file_upload",
      file_upload: {
        id: "11111111-1111-4111-8111-111111111111",
      },
      caption: [createRichText("Old caption")],
      name: "Old name",
    },
  };
}

function localImageAsset(): NotesFileAssetMetadata {
  return {
    relativePath: "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
    originalName: "local.png",
    contentType: "image/png",
    byteSize: 42,
    sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    kind: "image",
  };
}

function localImageBlock(): NotesImageBlock {
  return {
    object: "block",
    id: "block-4",
    parent: {
      type: "page_id",
      page_id: "page-1",
    },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "image",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    image: createManagedMediaPayload(localImageAsset(), "Old caption"),
  };
}

function duplicateSyncedBlock(): NotesSyncedBlock {
  return {
    object: "block",
    id: "block-3",
    parent: {
      type: "page_id",
      page_id: "page-1",
    },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "synced_block",
    source_provider: "notion",
    source_object_id: "source-synced-block",
    source_last_edited_time: "2026-01-01T00:00:00.000Z",
    synced_block: createSyncedBlockPayload("11111111-1111-4111-8111-111111111111"),
  };
}

describe("notes block factory conversions", () => {
  it("preserves rich text objects when converting to another text block", () => {
    const richText = richTextFixture();
    const converted = blockConvertedToType(paragraphBlock(richText), "heading_2");

    expect(converted.type).toBe("heading_2");
    if (converted.type !== "heading_2") throw new Error("Expected heading 2 conversion");
    expect(converted.heading_2.rich_text).toEqual(richText);
    expect(converted.heading_2.color).toBe("blue");
  });

  it("preserves rich text objects when converting to a toggle heading", () => {
    const richText = richTextFixture();
    const converted = blockWithHeadingToggleable(paragraphBlock(richText), "heading_3", true);

    expect(converted.type).toBe("heading_3");
    if (converted.type !== "heading_3") throw new Error("Expected heading 3 conversion");
    expect(converted.heading_3.rich_text).toEqual(richText);
    expect(converted.heading_3.color).toBe("blue");
    expect(converted.heading_3.is_toggleable).toBe(true);
    expect(converted.heading_3.ganbaru_open).toBe(true);
  });

  it("preserves imported media sources when editing caption without a new URL", () => {
    const update = blockWithMedia(fileUploadPdfBlock(), "", "New caption", "");

    expect(update.type).toBe("pdf");
    if (update.type !== "pdf") throw new Error("Expected PDF update");
    expect(update.pdf).toEqual({
      type: "file_upload",
      file_upload: {
        id: "11111111-1111-4111-8111-111111111111",
      },
      caption: [createRichText("New caption")],
    });
  });

  it("replaces imported media sources when the user provides an external URL", () => {
    const update = blockWithMedia(
      fileUploadPdfBlock(),
      "https://example.com/replacement.pdf",
      "Replacement",
    );

    expect(update.type).toBe("pdf");
    if (update.type !== "pdf") throw new Error("Expected PDF update");
    expect(update.pdf).toEqual({
      type: "external",
      external: {
        url: "https://example.com/replacement.pdf",
      },
      caption: [createRichText("Replacement")],
    });
  });

  it("attaches managed local media assets", () => {
    const update = blockWithMedia(
      fileUploadPdfBlock(),
      "",
      "Local PDF",
      "brief.pdf",
      {
        type: "attach",
        asset: {
          relativePath: "notes/files/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.pdf",
          originalName: "brief.pdf",
          contentType: "application/pdf",
          byteSize: 64,
          sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
          kind: "pdf",
        },
      },
    );

    expect(update.type).toBe("pdf");
    if (update.type !== "pdf") throw new Error("Expected PDF update");
    expect(update.pdf).toEqual({
      type: "file",
      file: {
        url: "ganbaru-asset:notes/files/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.pdf",
        name: "brief.pdf",
        content_type: "application/pdf",
        byte_size: 64,
        sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ganbaru_asset_path: "notes/files/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.pdf",
      },
      caption: [createRichText("Local PDF")],
      name: "brief.pdf",
    });
  });

  it("preserves managed local media sources while editing caption and name", () => {
    const update = blockWithMedia(
      localImageBlock(),
      "",
      "New caption",
      "renamed.png",
      { type: "preserve" },
    );

    expect(update.type).toBe("image");
    if (update.type !== "image") throw new Error("Expected image update");
    expect(update.image).toEqual(
      createManagedMediaPayload(localImageAsset(), "New caption", "renamed.png"),
    );
  });

  it("clears managed local media sources", () => {
    const update = blockWithMedia(localImageBlock(), "", "", undefined, { type: "clear" });

    expect(update.type).toBe("image");
    if (update.type !== "image") throw new Error("Expected image update");
    expect(update.image).toEqual({
      type: "external",
      external: { url: "" },
      caption: [],
    });
  });

  it("preserves imported duplicate synced block references in full update payloads", () => {
    const update = blockUpdateFromBlock(duplicateSyncedBlock());

    expect(update).toEqual({
      type: "synced_block",
      synced_block: createSyncedBlockPayload("11111111-1111-4111-8111-111111111111"),
    });
  });
});
