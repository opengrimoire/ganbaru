import type {
  NotesBlock,
  NotesBlockType,
  NotesBlockUpdate,
  NotesColor,
  NotesDateMentionValue,
  NotesMediaBlockPayload,
  NotesRichText,
} from "$lib/notes/types";
import { blockColor, canBlockHaveColor } from "./color";
import {
  applyRichTextAnnotations,
  applyRichTextLink,
  insertDateMentionRichText,
  insertEquationRichText,
  insertObjectMentionRichText,
  insertPageMentionRichText,
  richTextAnnotationsForSelection,
  richTextLinkRangeForSelection,
  replacePlainTextPreservingRichText,
  richTextPlainText,
  type NotesRichTextAnnotationPatch,
  type NotesRichTextAnnotationRange,
  type NotesRichTextLinkRange,
  type NotesObjectMentionTarget,
} from "$lib/notes/rich-text/core";
import {
  createManagedMediaPayload,
  type NotesFileAssetMetadata,
} from "$lib/notes/block-types/media";
import {
  DEFAULT_COLOR,
  DEFAULT_CODE_LANGUAGE,
  type NotesHeadingBlockType,
  type NotesTextPayloadOptions,
  createRichText,
  createTodoPayload,
  createCalloutPayload,
  createCodePayload,
  createBookmarkPayload,
  createLinkPreviewPayload,
  createButtonPayload,
  createEmbedPayload,
  createEquationPayload,
  createMediaPayload,
  createBlockUpdate,
  createTextPayloadFromRichText,
} from "./payloads";
import {
  blockPlainText,
  blockIndent,
  blockEditableRichText,
} from "./queries";

function createMediaPayloadFromExistingSource(
  existing: NotesMediaBlockPayload,
  url: string,
  caption: string,
  name?: string,
): NotesMediaBlockPayload {
  const trimmedUrl = url.trim();
  if (trimmedUrl || existing.type === "external") return createMediaPayload(url, caption, name);
  const trimmedCaption = caption.trim();
  const trimmedName = name?.trim();
  const editableFields = {
    caption: trimmedCaption ? [createRichText(trimmedCaption)] : [],
    ...(trimmedName ? { name: trimmedName } : {}),
  };
  if (existing.type === "file_upload") {
    return {
      type: "file_upload",
      file_upload: existing.file_upload,
      ...editableFields,
    };
  }
  return {
    type: "file",
    file: trimmedName ? { ...existing.file, name: trimmedName } : existing.file,
    ...editableFields,
  };
}

export type NotesMediaAssetChange =
  | { type: "attach"; asset: NotesFileAssetMetadata }
  | { type: "preserve" }
  | { type: "clear" };

function createMediaPayloadFromChange(
  existing: NotesMediaBlockPayload,
  url: string,
  caption: string,
  name: string | undefined,
  assetChange?: NotesMediaAssetChange,
): NotesMediaBlockPayload {
  if (assetChange?.type === "attach") {
    return createManagedMediaPayload(assetChange.asset, caption, name);
  }
  if (assetChange?.type === "clear") {
    return createMediaPayload("", caption, name);
  }
  if (assetChange?.type === "preserve") {
    return createMediaPayloadFromExistingSource(existing, "", caption, name);
  }
  return createMediaPayloadFromExistingSource(existing, url, caption, name);
}

/** Convert a loaded block to a Notion-shaped update payload while replacing its rich text. */
function blockWithRichTextPayload(
  block: NotesBlock,
  richText: readonly NotesRichText[],
): NotesBlockUpdate {
  switch (block.type) {
    case "paragraph":
      return {
        type: block.type,
        paragraph: createTextPayloadFromRichText(richText, blockColor(block), {
          icon: block.paragraph.icon,
        }),
      };
    case "heading_1":
      return {
        type: block.type,
        heading_1: createTextPayloadFromRichText(richText, blockColor(block), {
          isToggleable: block.heading_1.is_toggleable,
          open: block.heading_1.ganbaru_open,
        }),
      };
    case "heading_2":
      return {
        type: block.type,
        heading_2: createTextPayloadFromRichText(richText, blockColor(block), {
          isToggleable: block.heading_2.is_toggleable,
          open: block.heading_2.ganbaru_open,
        }),
      };
    case "heading_3":
      return {
        type: block.type,
        heading_3: createTextPayloadFromRichText(richText, blockColor(block), {
          isToggleable: block.heading_3.is_toggleable,
          open: block.heading_3.ganbaru_open,
        }),
      };
    case "heading_4":
      return {
        type: block.type,
        heading_4: createTextPayloadFromRichText(richText, blockColor(block), {
          isToggleable: block.heading_4.is_toggleable,
          open: block.heading_4.ganbaru_open,
        }),
      };
    case "heading_5":
      return {
        type: block.type,
        heading_5: createTextPayloadFromRichText(richText, blockColor(block), {
          isToggleable: block.heading_5.is_toggleable,
          open: block.heading_5.ganbaru_open,
        }),
      };
    case "heading_6":
      return {
        type: block.type,
        heading_6: createTextPayloadFromRichText(richText, blockColor(block), {
          isToggleable: block.heading_6.is_toggleable,
          open: block.heading_6.ganbaru_open,
        }),
      };
    case "bulleted_list_item":
      return {
        type: block.type,
        bulleted_list_item: createTextPayloadFromRichText(richText, blockColor(block)),
      };
    case "numbered_list_item":
      return {
        type: block.type,
        numbered_list_item: createTextPayloadFromRichText(richText, blockColor(block)),
      };
    case "to_do":
      return {
        type: block.type,
        to_do: {
          ...createTextPayloadFromRichText(richText, blockColor(block)),
          checked: block.to_do.checked,
        },
      };
    case "toggle":
      return {
        type: block.type,
        toggle: {
          ...createTextPayloadFromRichText(richText, blockColor(block)),
          ganbaru_open: block.toggle.ganbaru_open ?? true,
        },
      };
    case "callout":
      return {
        type: block.type,
        callout: {
          ...createTextPayloadFromRichText(richText, blockColor(block)),
          icon: block.callout.icon,
        },
      };
    case "quote":
      return { type: block.type, quote: createTextPayloadFromRichText(richText, blockColor(block)) };
    case "child_page":
      return { type: block.type, child_page: block.child_page };
    case "child_database":
      return { type: block.type, child_database: block.child_database };
    case "breadcrumb":
      return { type: block.type, breadcrumb: block.breadcrumb };
    case "table_of_contents":
      return { type: block.type, table_of_contents: block.table_of_contents };
    case "column_list":
      return { type: block.type, column_list: block.column_list };
    case "column":
      return { type: block.type, column: block.column };
    case "table":
      return { type: block.type, table: block.table };
    case "table_row":
      return { type: block.type, table_row: block.table_row };
    case "tab":
      return { type: block.type, tab: block.tab };
    case "image":
      return { type: block.type, image: block.image };
    case "video":
      return { type: block.type, video: block.video };
    case "audio":
      return { type: block.type, audio: block.audio };
    case "file":
      return { type: block.type, file: block.file };
    case "pdf":
      return { type: block.type, pdf: block.pdf };
    case "bookmark":
      return { type: block.type, bookmark: block.bookmark };
    case "link_preview":
      return { type: block.type, link_preview: block.link_preview };
    case "synced_block":
      return { type: block.type, synced_block: block.synced_block };
    case "template":
      return {
        type: block.type,
        template: {
          rich_text: richText.length > 0 ? [...richText] : [createRichText("")],
        },
      };
    case "button":
      return {
        type: block.type,
        button: {
          ...block.button,
          rich_text: richText.length > 0 ? [...richText] : [createRichText("")],
        },
      };
    case "embed":
      return { type: block.type, embed: block.embed };
    case "equation":
      return { type: block.type, equation: block.equation };
    case "code":
      return {
        type: block.type,
        code: {
          ...block.code,
          rich_text: [...richText],
        },
      };
    case "divider":
      return { type: block.type, divider: {} };
    case "unsupported":
      return { type: block.type, unsupported: block.unsupported };
  }
}

/** Convert a loaded block to a Notion-shaped update payload while replacing text. */
export function blockWithText(block: NotesBlock, text: string): NotesBlockUpdate {
  return blockWithRichText(block, replacePlainTextPreservingRichText(blockEditableRichText(block), text));
}

export function blockWithPageMention(
  block: NotesBlock,
  start: number,
  end: number,
  pageId: string,
  title: string,
  href: string | null,
): NotesBlockUpdate {
  return blockWithRichText(
    block,
    insertPageMentionRichText(blockEditableRichText(block), start, end, pageId, title, href),
  );
}

export function blockWithDateMention(
  block: NotesBlock,
  start: number,
  end: number,
  date: NotesDateMentionValue,
  title: string,
): NotesBlockUpdate {
  return blockWithRichText(
    block,
    insertDateMentionRichText(blockEditableRichText(block), start, end, date, title),
  );
}

export function blockWithObjectMention(
  block: NotesBlock,
  start: number,
  end: number,
  target: NotesObjectMentionTarget,
): NotesBlockUpdate {
  return blockWithRichText(
    block,
    insertObjectMentionRichText(blockEditableRichText(block), start, end, target),
  );
}

export function blockWithInlineEquation(
  block: NotesBlock,
  start: number,
  end: number,
  expression: string,
): NotesBlockUpdate {
  return blockWithRichText(
    block,
    insertEquationRichText(blockEditableRichText(block), start, end, expression),
  );
}

export function blockTextLinkRangeForSelection(
  block: NotesBlock,
  selectionStart: number,
  selectionEnd: number,
): NotesRichTextLinkRange {
  return richTextLinkRangeForSelection(blockEditableRichText(block), selectionStart, selectionEnd);
}

export function blockTextAnnotationsForSelection(
  block: NotesBlock,
  selectionStart: number,
  selectionEnd: number,
): NotesRichTextAnnotationRange {
  return richTextAnnotationsForSelection(blockEditableRichText(block), selectionStart, selectionEnd);
}

export function blockWithTextLink(
  block: NotesBlock,
  start: number,
  end: number,
  url: string | null,
): NotesBlockUpdate {
  return blockWithRichText(
    block,
    applyRichTextLink(blockEditableRichText(block), start, end, url),
  );
}

export function blockWithTextAnnotations(
  block: NotesBlock,
  start: number,
  end: number,
  patch: NotesRichTextAnnotationPatch,
): NotesBlockUpdate {
  return blockWithRichText(
    block,
    applyRichTextAnnotations(blockEditableRichText(block), start, end, patch),
  );
}

export function blockWithBookmark(
  block: NotesBlock,
  url: string,
  caption: string,
): NotesBlockUpdate {
  if (block.type !== "bookmark") return blockWithText(block, blockPlainText(block));
  return {
    type: "bookmark",
    bookmark: { ...createBookmarkPayload(url, caption), ganbaru_indent: blockIndent(block) || undefined },
  };
}

export function blockWithEmbedUrl(block: NotesBlock, url: string): NotesBlockUpdate {
  if (block.type !== "embed") return blockWithText(block, blockPlainText(block));
  return {
    type: "embed",
    embed: { ...createEmbedPayload(url), ganbaru_indent: blockIndent(block) || undefined },
  };
}

export function blockWithLinkPreviewUrl(block: NotesBlock, url: string): NotesBlockUpdate {
  if (block.type !== "link_preview") return blockWithText(block, blockPlainText(block));
  return {
    type: "link_preview",
    link_preview: { ...createLinkPreviewPayload(url), ganbaru_indent: blockIndent(block) || undefined },
  };
}

export function blockWithEquationExpression(
  block: NotesBlock,
  expression: string,
): NotesBlockUpdate {
  if (block.type !== "equation") return blockWithText(block, blockPlainText(block));
  return {
    type: "equation",
    equation: { ...createEquationPayload(expression), ganbaru_indent: blockIndent(block) || undefined },
  };
}

export function blockWithMedia(
  block: NotesBlock,
  url: string,
  caption: string,
  name?: string,
  assetChange?: NotesMediaAssetChange,
): NotesBlockUpdate {
  if (block.type === "image") {
    return {
      type: "image",
      image: { ...createMediaPayloadFromChange(block.image, url, caption, name, assetChange), ganbaru_indent: blockIndent(block) || undefined },
    };
  }
  if (block.type === "video") {
    return {
      type: "video",
      video: { ...createMediaPayloadFromChange(block.video, url, caption, name, assetChange), ganbaru_indent: blockIndent(block) || undefined },
    };
  }
  if (block.type === "audio") {
    return {
      type: "audio",
      audio: { ...createMediaPayloadFromChange(block.audio, url, caption, name, assetChange), ganbaru_indent: blockIndent(block) || undefined },
    };
  }
  if (block.type === "file") {
    return {
      type: "file",
      file: { ...createMediaPayloadFromChange(block.file, url, caption, name, assetChange), ganbaru_indent: blockIndent(block) || undefined },
    };
  }
  if (block.type === "pdf") {
    return {
      type: "pdf",
      pdf: { ...createMediaPayloadFromChange(block.pdf, url, caption, name, assetChange), ganbaru_indent: blockIndent(block) || undefined },
    };
  }
  return blockWithText(block, blockPlainText(block));
}

export function blockWithTodoChecked(block: NotesBlock, checked: boolean): NotesBlockUpdate {
  if (block.type !== "to_do") return blockWithText(block, blockPlainText(block));
  return {
    type: "to_do",
    to_do: { ...block.to_do, checked },
  };
}

export function blockWithCodeLanguage(block: NotesBlock, language: string): NotesBlockUpdate {
  const content = blockPlainText(block);
  return {
    type: "code",
    code: {
      ...(block.type === "code" ? block.code : createCodePayload(content)),
      language: language.trim() || DEFAULT_CODE_LANGUAGE,
      ganbaru_indent: blockIndent(block) || undefined,
    },
  };
}

export function blockWithToggleOpen(block: NotesBlock, open: boolean): NotesBlockUpdate {
  if (block.type !== "toggle") return blockWithText(block, blockPlainText(block));
  return {
    type: "toggle",
    toggle: {
      ...block.toggle,
      ganbaru_open: open,
    },
  };
}

export function blockWithHeadingToggleable(
  block: NotesBlock,
  headingType: NotesHeadingBlockType,
  isToggleable = true,
): NotesBlockUpdate {
  const color = canBlockHaveColor(block.type) ? blockColor(block) : DEFAULT_COLOR;
  return blockUpdateWithIndent(createBlockUpdateFromRichText(headingType, blockEditableRichText(block), color, {
    isToggleable,
    open: isToggleable ? true : undefined,
  }), blockIndent(block));
}

export function blockWithHeadingToggleOpen(
  block: NotesBlock,
  open: boolean,
): NotesBlockUpdate {
  if (block.type === "heading_1" && block.heading_1.is_toggleable === true) {
    return {
      type: "heading_1",
      heading_1: {
        ...block.heading_1,
        ganbaru_open: open,
      },
    };
  }
  if (block.type === "heading_2" && block.heading_2.is_toggleable === true) {
    return {
      type: "heading_2",
      heading_2: {
        ...block.heading_2,
        ganbaru_open: open,
      },
    };
  }
  if (block.type === "heading_3" && block.heading_3.is_toggleable === true) {
    return {
      type: "heading_3",
      heading_3: {
        ...block.heading_3,
        ganbaru_open: open,
      },
    };
  }
  if (block.type === "heading_4" && block.heading_4.is_toggleable === true) {
    return {
      type: "heading_4",
      heading_4: {
        ...block.heading_4,
        ganbaru_open: open,
      },
    };
  }
  if (block.type === "heading_5" && block.heading_5.is_toggleable === true) {
    return {
      type: "heading_5",
      heading_5: {
        ...block.heading_5,
        ganbaru_open: open,
      },
    };
  }
  if (block.type === "heading_6" && block.heading_6.is_toggleable === true) {
    return {
      type: "heading_6",
      heading_6: {
        ...block.heading_6,
        ganbaru_open: open,
      },
    };
  }
  return blockWithText(block, blockPlainText(block));
}

export function blockConvertedToType(block: NotesBlock, type: NotesBlockType): NotesBlockUpdate {
  const color = canBlockHaveColor(type) ? blockColor(block) : DEFAULT_COLOR;
  if (type === "divider") return createBlockUpdate(type, "", color);
  return blockUpdateWithIndent(createBlockUpdateFromRichText(type, blockEditableRichText(block), color), blockIndent(block));
}

function createBlockUpdateFromRichText(
  type: NotesBlockType,
  richText: readonly NotesRichText[],
  color: NotesColor = DEFAULT_COLOR,
  options: NotesTextPayloadOptions = {},
): NotesBlockUpdate {
  switch (type) {
    case "paragraph":
      return { type, paragraph: createTextPayloadFromRichText(richText, color) };
    case "heading_1":
      return {
        type,
        heading_1: createTextPayloadFromRichText(richText, color, options),
      };
    case "heading_2":
      return {
        type,
        heading_2: createTextPayloadFromRichText(richText, color, options),
      };
    case "heading_3":
      return {
        type,
        heading_3: createTextPayloadFromRichText(richText, color, options),
      };
    case "heading_4":
      return {
        type,
        heading_4: createTextPayloadFromRichText(richText, color, options),
      };
    case "heading_5":
      return {
        type,
        heading_5: createTextPayloadFromRichText(richText, color, options),
      };
    case "heading_6":
      return {
        type,
        heading_6: createTextPayloadFromRichText(richText, color, options),
      };
    case "bulleted_list_item":
      return { type, bulleted_list_item: createTextPayloadFromRichText(richText, color) };
    case "numbered_list_item":
      return { type, numbered_list_item: createTextPayloadFromRichText(richText, color) };
    case "to_do":
      return {
        type,
        to_do: {
          ...createTextPayloadFromRichText(richText, color),
          checked: false,
        },
      };
    case "toggle":
      return {
        type,
        toggle: {
          ...createTextPayloadFromRichText(richText, color),
          ganbaru_open: true,
        },
      };
    case "callout":
      return {
        type,
        callout: {
          ...createTextPayloadFromRichText(richText, color),
          icon: createCalloutPayload("").icon,
        },
      };
    case "quote":
      return { type, quote: createTextPayloadFromRichText(richText, color) };
    case "code":
      return {
        type,
        code: {
          rich_text: richText.length > 0 ? [...richText] : [createRichText("")],
          caption: [],
          language: DEFAULT_CODE_LANGUAGE,
        },
      };
    case "template":
      return {
        type,
        template: {
          rich_text: richText.length > 0 ? [...richText] : [createRichText("")],
        },
      };
    case "button": {
      const plainText = richTextPlainText(richText);
      const payload = createButtonPayload(plainText);
      return {
        type,
        button: {
          ...payload,
          rich_text: plainText.trim() ? [...richText] : payload.rich_text,
        },
      };
    }
    default:
      return createBlockUpdate(type, richTextPlainText(richText), color, options);
  }
}

/** Convert a loaded block into a full update payload preserving its current typed payload. */
export function blockUpdateFromBlock(block: NotesBlock): NotesBlockUpdate {
  switch (block.type) {
    case "paragraph":
      return { type: block.type, paragraph: structuredClone(block.paragraph) };
    case "heading_1":
      return { type: block.type, heading_1: structuredClone(block.heading_1) };
    case "heading_2":
      return { type: block.type, heading_2: structuredClone(block.heading_2) };
    case "heading_3":
      return { type: block.type, heading_3: structuredClone(block.heading_3) };
    case "heading_4":
      return { type: block.type, heading_4: structuredClone(block.heading_4) };
    case "heading_5":
      return { type: block.type, heading_5: structuredClone(block.heading_5) };
    case "heading_6":
      return { type: block.type, heading_6: structuredClone(block.heading_6) };
    case "bulleted_list_item":
      return { type: block.type, bulleted_list_item: structuredClone(block.bulleted_list_item) };
    case "numbered_list_item":
      return { type: block.type, numbered_list_item: structuredClone(block.numbered_list_item) };
    case "to_do":
      return { type: block.type, to_do: structuredClone(block.to_do) };
    case "toggle":
      return { type: block.type, toggle: structuredClone(block.toggle) };
    case "callout":
      return { type: block.type, callout: structuredClone(block.callout) };
    case "quote":
      return { type: block.type, quote: structuredClone(block.quote) };
    case "child_page":
      return { type: block.type, child_page: structuredClone(block.child_page) };
    case "child_database":
      return { type: block.type, child_database: structuredClone(block.child_database) };
    case "breadcrumb":
      return { type: block.type, breadcrumb: structuredClone(block.breadcrumb) };
    case "table_of_contents":
      return { type: block.type, table_of_contents: structuredClone(block.table_of_contents) };
    case "column_list":
      return { type: block.type, column_list: structuredClone(block.column_list) };
    case "column":
      return { type: block.type, column: structuredClone(block.column) };
    case "table":
      return { type: block.type, table: structuredClone(block.table) };
    case "table_row":
      return { type: block.type, table_row: structuredClone(block.table_row) };
    case "tab":
      return { type: block.type, tab: structuredClone(block.tab) };
    case "image":
      return { type: block.type, image: structuredClone(block.image) };
    case "video":
      return { type: block.type, video: structuredClone(block.video) };
    case "audio":
      return { type: block.type, audio: structuredClone(block.audio) };
    case "file":
      return { type: block.type, file: structuredClone(block.file) };
    case "pdf":
      return { type: block.type, pdf: structuredClone(block.pdf) };
    case "bookmark":
      return { type: block.type, bookmark: structuredClone(block.bookmark) };
    case "link_preview":
      return { type: block.type, link_preview: structuredClone(block.link_preview) };
    case "synced_block":
      return { type: block.type, synced_block: structuredClone(block.synced_block) };
    case "template":
      return { type: block.type, template: structuredClone(block.template) };
    case "button":
      return { type: block.type, button: structuredClone(block.button) };
    case "embed":
      return { type: block.type, embed: structuredClone(block.embed) };
    case "equation":
      return { type: block.type, equation: structuredClone(block.equation) };
    case "divider":
      return { type: block.type, divider: structuredClone(block.divider) };
    case "code":
      return { type: block.type, code: structuredClone(block.code) };
    case "unsupported":
      return { type: block.type, unsupported: structuredClone(block.unsupported) };
  }
}

export function applyBlockUpdate(block: NotesBlock, update: NotesBlockUpdate): NotesBlock {
  const base = {
    object: "block" as const,
    edit_revision: block.edit_revision,
    id: block.id,
    parent: block.parent,
    created_time: block.created_time,
    last_edited_time: new Date().toISOString(),
    has_children: block.has_children,
    in_trash: block.in_trash,
    archived: block.archived,
    source_provider: block.source_provider,
    source_object_id: block.source_object_id,
    source_last_edited_time: block.source_last_edited_time,
  };
  switch (update.type) {
    case "paragraph":
      return { ...base, type: update.type, paragraph: update.paragraph };
    case "heading_1":
      return { ...base, type: update.type, heading_1: update.heading_1 };
    case "heading_2":
      return { ...base, type: update.type, heading_2: update.heading_2 };
    case "heading_3":
      return { ...base, type: update.type, heading_3: update.heading_3 };
    case "heading_4":
      return { ...base, type: update.type, heading_4: update.heading_4 };
    case "heading_5":
      return { ...base, type: update.type, heading_5: update.heading_5 };
    case "heading_6":
      return { ...base, type: update.type, heading_6: update.heading_6 };
    case "bulleted_list_item":
      return { ...base, type: update.type, bulleted_list_item: update.bulleted_list_item };
    case "numbered_list_item":
      return { ...base, type: update.type, numbered_list_item: update.numbered_list_item };
    case "to_do":
      return { ...base, type: update.type, to_do: update.to_do };
    case "toggle":
      return { ...base, type: update.type, toggle: update.toggle };
    case "callout":
      return { ...base, type: update.type, callout: update.callout };
    case "quote":
      return { ...base, type: update.type, quote: update.quote };
    case "child_page":
      return { ...base, type: update.type, child_page: update.child_page };
    case "child_database":
      return { ...base, type: update.type, child_database: update.child_database };
    case "breadcrumb":
      return { ...base, type: update.type, breadcrumb: update.breadcrumb };
    case "table_of_contents":
      return { ...base, type: update.type, table_of_contents: update.table_of_contents };
    case "column_list":
      return { ...base, type: update.type, column_list: update.column_list };
    case "column":
      return { ...base, type: update.type, column: update.column };
    case "table":
      return { ...base, type: update.type, table: update.table };
    case "table_row":
      return { ...base, type: update.type, table_row: update.table_row };
    case "image":
      return { ...base, type: update.type, image: update.image };
    case "video":
      return { ...base, type: update.type, video: update.video };
    case "audio":
      return { ...base, type: update.type, audio: update.audio };
    case "file":
      return { ...base, type: update.type, file: update.file };
    case "pdf":
      return { ...base, type: update.type, pdf: update.pdf };
    case "bookmark":
      return { ...base, type: update.type, bookmark: update.bookmark };
    case "link_preview":
      return { ...base, type: update.type, link_preview: update.link_preview };
    case "synced_block":
      return { ...base, type: update.type, synced_block: update.synced_block };
    case "template":
      return { ...base, type: update.type, template: update.template };
    case "button":
      return { ...base, type: update.type, button: update.button };
    case "tab":
      return { ...base, type: update.type, tab: update.tab };
    case "embed":
      return { ...base, type: update.type, embed: update.embed };
    case "equation":
      return { ...base, type: update.type, equation: update.equation };
    case "divider":
      return { ...base, type: update.type, divider: update.divider };
    case "code":
      return { ...base, type: update.type, code: update.code };
    case "unsupported":
      return { ...base, type: update.type, unsupported: update.unsupported };
  }
}

/** Preserve indentation across ordinary typing and formatting updates. */
export function blockWithRichText(block: NotesBlock, richText: readonly NotesRichText[]): NotesBlockUpdate {
  return blockUpdateWithIndent(blockWithRichTextPayload(block, richText), blockIndent(block));
}

/** Apply explicit indentation without changing text, annotations, or list markers. */
export function blockUpdateWithIndent(update: NotesBlockUpdate, indent: number): NotesBlockUpdate {
  if (!Number.isSafeInteger(indent) || indent < 0) throw new Error("Invalid Notes indentation");
  const record: Record<string, unknown> = { ...update };
  const payload = record[update.type];
  if (typeof payload !== "object" || payload === null) throw new Error("Missing Notes block payload");
  return { ...update, [update.type]: { ...payload, ganbaru_indent: indent > 0 ? indent : undefined } };
}
