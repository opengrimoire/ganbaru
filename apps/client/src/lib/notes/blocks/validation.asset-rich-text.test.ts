import { describe, expect, it } from "vitest";
import { parseNotesBlock, parseNotesPage, parseNotesRichTextArray } from "./validation";
import { baseBlock, basePage, baseRichText } from "./validation.fixtures";

describe("notes asset-rich-text boundary validation", () => {
  it("parses page emoji icons", () => {
      const page = parseNotesPage({
        ...basePage,
        icon: { type: "emoji", emoji: "📌" },
      });

      expect(page.icon).toEqual({ type: "emoji", emoji: "📌" });
    });

  it("rejects empty page emoji icons", () => {
      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: { type: "emoji", emoji: "" },
        }),
      ).toThrow("page.icon.emoji must not be empty");
    });

  it("parses expanded page icon payloads", () => {
      expect(parseNotesPage({
        ...basePage,
        icon: { type: "icon", icon: { name: "home", color: "blue" } },
      }).icon).toEqual({ type: "icon", icon: { name: "home", color: "blue" } });

      expect(parseNotesPage({
        ...basePage,
        icon: {
          type: "custom_emoji",
          custom_emoji: {
            id: "emoji-a",
            name: "Focus",
            url: "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            ganbaru_asset_path: "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
          },
        },
      }).icon).toEqual({
        type: "custom_emoji",
        custom_emoji: {
          id: "emoji-a",
          name: "Focus",
          url: "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
          ganbaru_asset_path: "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        },
      });

      expect(parseNotesPage({
        ...basePage,
        icon: { type: "external", external: { url: "https://example.com/icon.png" } },
      }).icon).toEqual({ type: "external", external: { url: "https://example.com/icon.png" } });

      expect(parseNotesPage({
        ...basePage,
        icon: {
          type: "file",
          file: {
            url: "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp",
            name: "Focus",
            content_type: "image/webp",
            byte_size: 42,
            sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ganbaru_asset_path: "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp",
          },
        },
      }).icon).toEqual({
        type: "file",
        file: {
          url: "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp",
          name: "Focus",
          content_type: "image/webp",
          byte_size: 42,
          sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
          ganbaru_asset_path: "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp",
        },
      });
    });

  it("rejects unsafe expanded page icon payloads", () => {
      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: { type: "external", external: { url: "http://example.com/icon.png" } },
        }),
      ).toThrow("page.icon.external.url must be a supported HTTPS image URL");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: { type: "custom_emoji", custom_emoji: { id: "", name: "Missing id" } },
        }),
      ).toThrow("page.icon.custom_emoji.id must not be empty");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: {
            type: "custom_emoji",
            custom_emoji: {
              id: "wrong-directory",
              url: "ganbaru-asset:notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            },
          },
        }),
      ).toThrow("page.icon.custom_emoji.url must stay under a managed image asset directory");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: {
            type: "custom_emoji",
            custom_emoji: {
              id: "missing-asset-path",
              url: "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            },
          },
        }),
      ).toThrow("page.icon.custom_emoji.url must reference the managed icon asset path");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-icons/bad.svg",
              content_type: "image/svg+xml",
              byte_size: 42,
              sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
              ganbaru_asset_path: "notes/page-icons/bad.svg",
            },
          },
        }),
      ).toThrow("page.icon.file.ganbaru_asset_path must stay under a managed image asset directory");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-icons/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.webp",
              content_type: "image/webp",
              byte_size: 42,
              sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
              ganbaru_asset_path: "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp",
            },
          },
        }),
      ).toThrow("page.icon.file.url must reference the managed icon asset path");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp",
            },
          },
        }),
      ).toThrow("page.icon.file.url must include managed asset metadata");
    });

  it("parses paragraph icons used by tab labels", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "paragraph",
        paragraph: {
          rich_text: [baseRichText],
          color: "default",
          icon: { type: "icon", icon: { name: "star", color: "yellow" } },
        },
      });

      expect(block.type).toBe("paragraph");
      if (block.type === "paragraph") {
        expect(block.paragraph.icon).toEqual({
          type: "icon",
          icon: { name: "star", color: "yellow" },
        });
      }
    });

  it("parses local managed media file objects", () => {
      const assetPath = "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";
      const block = parseNotesBlock({
        ...baseBlock,
        type: "image",
        image: {
          type: "file",
          file: {
            url: `ganbaru-asset:${assetPath}`,
            name: "local.png",
            content_type: "image/png",
            byte_size: 42,
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ganbaru_asset_path: assetPath,
          },
          caption: [baseRichText],
          name: "local.png",
        },
      });

      expect(block.type).toBe("image");
      if (block.type !== "image" || block.image.type !== "file") {
        throw new Error("Expected local image file block");
      }
      expect(block.image.file).toEqual({
        url: `ganbaru-asset:${assetPath}`,
        name: "local.png",
        content_type: "image/png",
        byte_size: 42,
        sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ganbaru_asset_path: assetPath,
      });
    });

  it("rejects unsafe local managed media file objects", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "pdf",
          pdf: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.pdf",
              name: "brief.pdf",
              content_type: "text/plain",
              byte_size: 42,
              sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              ganbaru_asset_path:
                "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.pdf",
            },
            caption: [],
          },
        }),
      ).toThrow("block.pdf.file.content_type must match the local media block type");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "image",
          image: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
              content_type: "image/png",
              byte_size: 42,
              sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              ganbaru_asset_path:
                "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            },
            caption: [],
          },
        }),
      ).toThrow("block.image.file.ganbaru_asset_path must stay under the managed Notes file directory");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "image",
          image: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            },
            caption: [],
          },
        }),
      ).toThrow("block.image.file.url must include managed asset metadata");
    });

  it("parses link preview payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "link_preview",
        link_preview: {
          url: "https://github.com/example/repo/pull/123",
        },
      });

      expect(block.type).toBe("link_preview");
      if (block.type === "link_preview") {
        expect(block.link_preview.url).toBe("https://github.com/example/repo/pull/123");
      }
    });

  it("rejects link preview URLs with control characters", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "link_preview",
          link_preview: {
            url: "https://example.com/bad\u0008",
          },
        }),
      ).toThrow("block.link_preview.url must not contain control characters");
    });

  it("parses page covers as image file objects", () => {
      const externalPage = parseNotesPage({
        ...basePage,
        icon: null,
        cover: { type: "external", external: { url: "https://example.com/cover.jpg" } },
      });

      expect(externalPage.cover).toEqual({
        type: "external",
        external: { url: "https://example.com/cover.jpg" },
      });

      const importedFilePage = parseNotesPage({
        ...basePage,
        icon: null,
        cover: {
          type: "file",
          file: {
            url: "https://example.com/imported-cover.webp",
            expiry_time: "2026-07-01T12:00:00.000Z",
          },
        },
      });
      expect(importedFilePage.cover).toEqual({
        type: "file",
        file: {
          url: "https://example.com/imported-cover.webp",
          expiry_time: "2026-07-01T12:00:00.000Z",
        },
      });

      const localFilePage = parseNotesPage({
        ...basePage,
        icon: null,
        cover: {
          type: "file",
          file: {
            url: "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            name: "Cover",
            content_type: "image/png",
            byte_size: 42,
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ganbaru_asset_path: "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
          },
        },
      });
      expect(localFilePage.cover).toEqual({
        type: "file",
        file: {
          url: "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
          name: "Cover",
          content_type: "image/png",
          byte_size: 42,
          sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          ganbaru_asset_path: "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        },
      });

      const fileUploadPage = parseNotesPage({
        ...basePage,
        icon: null,
        cover: {
          type: "file_upload",
          file_upload: { id: "55555555-5555-4555-8555-555555555555" },
        },
      });
      expect(fileUploadPage.cover).toEqual({
        type: "file_upload",
        file_upload: { id: "55555555-5555-4555-8555-555555555555" },
      });
    });

  it("rejects unsafe page cover file objects", () => {
      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: null,
          cover: { type: "external", external: { url: "https://example.com/file.pdf" } },
        }),
      ).toThrow("page.cover.external.url must be a supported HTTPS image URL");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: null,
          cover: {
            type: "file",
            file: {
              url: "https://example.com/file.pdf",
              expiry_time: "2026-07-01T12:00:00.000Z",
            },
          },
        }),
      ).toThrow("page.cover.file.url must be a supported HTTPS image URL");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: null,
          cover: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-covers/bad.svg",
              content_type: "image/svg+xml",
              byte_size: 42,
              sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              ganbaru_asset_path: "notes/page-covers/bad.svg",
            },
          },
        }),
      ).toThrow("page.cover.file.ganbaru_asset_path must stay under a managed image asset directory");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: null,
          cover: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-covers/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
              content_type: "image/png",
              byte_size: 42,
              sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              ganbaru_asset_path: "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            },
          },
        }),
      ).toThrow("page.cover.file.url must reference the managed cover asset path");

      expect(() =>
        parseNotesPage({
          ...basePage,
          icon: null,
          cover: {
            type: "file",
            file: {
              url: "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            },
          },
        }),
      ).toThrow("page.cover.file.url must include managed asset metadata");
    });

  it("parses page mention rich text", () => {
      const richText = parseNotesRichTextArray(
        [
          {
            type: "mention",
            mention: {
              type: "page",
              page: { id: "22222222-2222-4222-8222-222222222222" },
            },
            annotations: {
              bold: false,
              italic: false,
              strikethrough: false,
              underline: false,
              code: false,
              color: "default",
            },
            plain_text: "Target page",
            href: "http://localhost:1420/?view=notes#notes?page=22222222-2222-4222-8222-222222222222",
          },
        ],
        "rich_text",
      );

      expect(richText[0]).toMatchObject({
        type: "mention",
        mention: { type: "page", page: { id: "22222222-2222-4222-8222-222222222222" } },
      });
    });

  it("reads app object mentions with readable ids and rejects empty ones", () => {
    const objectMention = (id: string) => ({
      type: "mention",
      mention: { type: "ganbaru_object", ganbaru_object: { type: "project", id } },
      annotations: { bold: false, italic: false, strikethrough: false, underline: false, code: false, color: "default" },
      plain_text: "Chores",
      href: null,
    });

    expect(parseNotesRichTextArray([objectMention("project-routine-chores")], "rich_text")[0]).toMatchObject({
      mention: { type: "ganbaru_object", ganbaru_object: { type: "project", id: "project-routine-chores" } },
    });
    expect(() => parseNotesRichTextArray([objectMention(" ")], "rich_text")).toThrow("must not be empty");
  });

  it("parses linked text rich text", () => {
      const richText = parseNotesRichTextArray(
        [
          {
            type: "text",
            text: {
              content: "docs",
              link: { url: "https://example.com/docs" },
            },
            annotations: {
              bold: false,
              italic: false,
              strikethrough: false,
              underline: false,
              code: false,
              color: "default",
            },
            plain_text: "docs",
            href: "https://example.com/docs",
          },
        ],
        "rich_text",
      );

      expect(richText[0]).toMatchObject({
        type: "text",
        text: { link: { url: "https://example.com/docs" } },
        href: "https://example.com/docs",
      });
    });

  it("parses email linked text rich text", () => {
      const richText = parseNotesRichTextArray(
        [
          {
            type: "text",
            text: {
              content: "email",
              link: { url: "mailto:team@example.com" },
            },
            annotations: {
              bold: false,
              italic: false,
              strikethrough: false,
              underline: false,
              code: false,
              color: "default",
            },
            plain_text: "email",
            href: "mailto:team@example.com",
          },
        ],
        "rich_text",
      );

      expect(richText[0]).toMatchObject({
        type: "text",
        text: { link: { url: "mailto:team@example.com" } },
        href: "mailto:team@example.com",
      });
    });

  it("parses annotated text rich text", () => {
      const richText = parseNotesRichTextArray(
        [
          {
            type: "text",
            text: {
              content: "important",
              link: null,
            },
            annotations: {
              bold: true,
              italic: true,
              strikethrough: false,
              underline: true,
              code: false,
              color: "blue_background",
            },
            plain_text: "important",
            href: null,
          },
        ],
        "rich_text",
      );

      expect(richText[0]).toMatchObject({
        type: "text",
        annotations: {
          bold: true,
          italic: true,
          underline: true,
          color: "blue_background",
        },
      });
    });

  it("parses inline equation rich text", () => {
      const richText = parseNotesRichTextArray(
        [
          {
            type: "equation",
            equation: {
              expression: "\\frac{a}{b}",
            },
            annotations: {
              bold: false,
              italic: false,
              strikethrough: false,
              underline: false,
              code: false,
              color: "default",
            },
            plain_text: "\\frac{a}{b}",
            href: null,
          },
        ],
        "rich_text",
      );

      expect(richText[0]).toMatchObject({
        type: "equation",
        equation: { expression: "\\frac{a}{b}" },
        plain_text: "\\frac{a}{b}",
      });
    });

  it("rejects invalid inline equation rich text", () => {
      expect(() =>
        parseNotesRichTextArray(
          [
            {
              type: "equation",
              equation: {
                expression: "bad\u0008",
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "bad",
              href: null,
            },
          ],
          "rich_text",
        ),
      ).toThrow("rich_text[0].equation.expression must not be empty, too long, or contain control characters");
    });

  it("rejects unsupported annotation colors", () => {
      expect(() =>
        parseNotesRichTextArray(
          [
            {
              type: "text",
              text: {
                content: "bad",
                link: null,
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "rainbow",
              },
              plain_text: "bad",
              href: null,
            },
          ],
          "rich_text",
        ),
      ).toThrow("rich_text[0].annotations.color must be a supported Notion color");
    });

  it("reads saved local page and database links and rejects malformed identities", () => {
    const url = `#notes?page=${basePage.id}&block=${baseBlock.id}`;
    const item = { ...baseRichText, text: { content: "Tasks", link: { url } }, plain_text: "Tasks", href: url };
    expect(parseNotesRichTextArray([item], "rich_text")).toEqual([item]);
    for (const invalid of ["#notes?page=bad", `#notes?page=${basePage.id}&block=bad`, "#other"])
      expect(() => parseNotesRichTextArray([{ ...item, text: { ...item.text, link: { url: invalid } } }], "rich_text")).toThrow();
  });

  it("rejects unsafe linked text URLs", () => {
      expect(() =>
        parseNotesRichTextArray(
          [
            {
              type: "text",
              text: {
                content: "bad",
                link: { url: "javascript:alert(1)" },
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "bad",
              href: null,
            },
          ],
          "rich_text",
        ),
      ).toThrow("rich_text[0].text.link.url must be a valid HTTP, HTTPS, or email URL");
    });

  it("parses date reminder rich text", () => {
      const richText = parseNotesRichTextArray(
        [
          {
            type: "mention",
            mention: {
              type: "date",
              date: {
                start: "2026-06-30",
                end: null,
                time_zone: null,
                ganbaru_reminder: { enabled: true },
              },
            },
            annotations: {
              bold: false,
              italic: false,
              strikethrough: false,
              underline: false,
              code: false,
              color: "default",
            },
            plain_text: "Remind Today",
            href: null,
          },
        ],
        "rich_text",
      );

      expect(richText[0]).toMatchObject({
        type: "mention",
        mention: {
          type: "date",
          date: {
            start: "2026-06-30",
            ganbaru_reminder: { enabled: true },
          },
        },
      });
    });

  it("rejects invalid date mention starts", () => {
      expect(() =>
        parseNotesRichTextArray(
          [
            {
              type: "mention",
              mention: {
                type: "date",
                date: { start: "2026-99-30" },
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "Bad date",
              href: null,
            },
          ],
          "rich_text",
        ),
      ).toThrow("rich_text[0].mention.date.start must be an ISO date or date-time");
    });

  it("parses expanded mention rich text targets", () => {
      const richText = parseNotesRichTextArray(
          [
            {
              type: "mention",
              mention: {
                type: "user",
                user: {
                  object: "user",
                  id: "11111111-1111-4111-8111-111111111111",
                },
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "Alice",
              href: null,
            },
            {
              type: "mention",
              mention: {
                type: "database",
                database: { id: "22222222-2222-4222-8222-222222222222" },
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "Database",
              href: null,
            },
            {
              type: "mention",
              mention: {
                type: "ganbaru_object",
                ganbaru_object: {
                  type: "calendar_event",
                  id: "event-1",
                },
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "Review",
              href: null,
            },
          ],
          "rich_text",
      );

      expect(richText.map((item) => item.type === "mention" ? item.mention.type : item.type))
        .toEqual(["user", "database", "ganbaru_object"]);
    });

  it("rejects unsupported mention rich text targets", () => {
      expect(() =>
        parseNotesRichTextArray(
          [
            {
              type: "mention",
              mention: {
                type: "link_preview",
                link_preview: { url: "https://example.com" },
              },
              annotations: {
                bold: false,
                italic: false,
                strikethrough: false,
                underline: false,
                code: false,
                color: "default",
              },
              plain_text: "Example",
              href: null,
            },
          ],
          "rich_text",
        ),
      ).toThrow("rich_text[0].mention.type is unsupported");
    });
});
