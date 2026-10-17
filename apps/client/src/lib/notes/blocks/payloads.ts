import type {
  NotesBlockType,
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesBookmarkBlockPayload,
  NotesButtonBlockPayload,
  NotesCalloutBlockPayload,
  NotesChildDatabaseBlockPayload,
  NotesChildPageBlockPayload,
  NotesCodeBlockPayload,
  NotesColumnBlockPayload,
  NotesColor,
  NotesEmbedBlockPayload,
  NotesEquationBlockPayload,
  NotesLinkPreviewBlockPayload,
  NotesMediaBlockPayload,
  NotesRichText,
  NotesSyncedBlockPayload,
  NotesTabBlockPayload,
  NotesTableBlockPayload,
  NotesTableRowBlockPayload,
  NotesTemplateBlockPayload,
  NotesTextBlockPayload,
  NotesToggleBlockPayload,
  NotesTodoBlockPayload,
  NotesUnsupportedBlockPayload,
} from "$lib/notes/types";
import {
  createTextRichText,
} from "$lib/notes/rich-text/core";

export const DEFAULT_COLOR: NotesColor = "default";
const DEFAULT_CALLOUT_COLOR: NotesColor = "gray_background";
export const DEFAULT_CODE_LANGUAGE = "plain text";
export const DEFAULT_TABLE_WIDTH = 2;
export const DEFAULT_TABLE_ROW_COUNT = 2;

export type NotesHeadingBlockType = "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6";

export interface NotesTextPayloadOptions {
  color?: NotesColor;
  isToggleable?: boolean;
  open?: boolean;
}

/** Create the minimum public Notion text rich text object Ganbaru edits. */
export function createRichText(content: string): NotesRichText {
  return createTextRichText(content);
}

export function createTextPayload(
  content: string,
  colorOrOptions: NotesColor | NotesTextPayloadOptions = DEFAULT_COLOR,
): NotesTextBlockPayload {
  const options = typeof colorOrOptions === "string"
    ? { color: colorOrOptions }
    : colorOrOptions;
  return {
    rich_text: [createRichText(content)],
    color: options.color ?? DEFAULT_COLOR,
    ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
    ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
  };
}

export function createTodoPayload(
  content: string,
  checked = false,
  color: NotesColor = DEFAULT_COLOR,
): NotesTodoBlockPayload {
  return {
    ...createTextPayload(content, color),
    checked,
  };
}

export function createTogglePayload(
  content: string,
  open = true,
  color: NotesColor = DEFAULT_COLOR,
): NotesToggleBlockPayload {
  return {
    ...createTextPayload(content, color),
    ganbaru_open: open,
  };
}

export function createCalloutPayload(
  content: string,
  color: NotesColor = DEFAULT_CALLOUT_COLOR,
): NotesCalloutBlockPayload {
  return {
    ...createTextPayload(content, color),
    icon: { type: "emoji", emoji: "💡" },
  };
}

export function createCodePayload(
  content: string,
  language = DEFAULT_CODE_LANGUAGE,
): NotesCodeBlockPayload {
  return {
    rich_text: [createRichText(content)],
    caption: [],
    language,
  };
}

export function createChildPagePayload(title: string): NotesChildPageBlockPayload {
  return { title };
}

export interface NotesChildDatabasePayloadOptions {
  databaseId?: string;
  dataSourceId?: string;
  viewId?: string;
}

export function createChildDatabasePayload(
  title: string,
  options: NotesChildDatabasePayloadOptions = {},
): NotesChildDatabaseBlockPayload {
  return {
    title,
    ...(options.databaseId === undefined ? {} : { database_id: options.databaseId }),
    ...(options.dataSourceId === undefined ? {} : { data_source_id: options.dataSourceId }),
    ...(options.viewId === undefined ? {} : { view_id: options.viewId }),
  };
}

export function createColumnPayload(widthRatio?: number): NotesColumnBlockPayload {
  if (widthRatio === undefined) return {};
  if (!Number.isFinite(widthRatio) || widthRatio <= 0) return {};
  return { width_ratio: Math.min(1, widthRatio) };
}

export function createTablePayload(
  tableWidth = DEFAULT_TABLE_WIDTH,
  hasColumnHeader = false,
  hasRowHeader = false,
): NotesTableBlockPayload {
  return {
    table_width: Math.max(1, Math.trunc(tableWidth)),
    has_column_header: hasColumnHeader,
    has_row_header: hasRowHeader,
  };
}

export function createTabPayload(): NotesTabBlockPayload {
  return {};
}

export function createTableCell(content: string): NotesRichText[] {
  return content.length > 0 ? [createRichText(content)] : [];
}

export function createTableRowPayload(
  cells: readonly (string | readonly NotesRichText[])[],
): NotesTableRowBlockPayload {
  return {
    cells: cells.map((cell) => (typeof cell === "string" ? createTableCell(cell) : [...cell])),
  };
}

export function createEmptyTableRowPayload(
  width = DEFAULT_TABLE_WIDTH,
): NotesTableRowBlockPayload {
  return createTableRowPayload(Array.from({ length: Math.max(1, Math.trunc(width)) }, () => ""));
}

export function createBookmarkPayload(
  url: string,
  caption = "",
): NotesBookmarkBlockPayload {
  const trimmedCaption = caption.trim();
  return {
    caption: trimmedCaption ? [createRichText(trimmedCaption)] : [],
    url,
  };
}

export function createLinkPreviewPayload(url: string): NotesLinkPreviewBlockPayload {
  return { url };
}

/** Create an original synced block or a duplicate reference to another block. */
export function createSyncedBlockPayload(
  syncedFromBlockId?: string | null,
): NotesSyncedBlockPayload {
  const blockId = syncedFromBlockId?.trim();
  if (!blockId) return { synced_from: null };
  return {
    synced_from: {
      type: "block_id",
      block_id: blockId,
    },
  };
}

export function createTemplatePayload(content: string): NotesTemplateBlockPayload {
  return {
    rich_text: [createRichText(content)],
  };
}

export function createButtonPayload(content: string): NotesButtonBlockPayload {
  return {
    rich_text: [createRichText(content.trim() || "Button")],
    icon: { type: "icon", icon: { name: "mouse-pointer-click", color: "gray" } },
    actions: [
      {
        type: "insert_blocks",
        source: "children",
        position: "below_button",
      },
    ],
  };
}

export function createEmbedPayload(url: string): NotesEmbedBlockPayload {
  return { url };
}

export function createEquationPayload(expression: string): NotesEquationBlockPayload {
  return { expression };
}

export function createMediaPayload(
  url = "",
  caption = "",
  name?: string,
): NotesMediaBlockPayload {
  const trimmedCaption = caption.trim();
  return {
    type: "external",
    external: { url: url.trim() },
    caption: trimmedCaption ? [createRichText(trimmedCaption)] : [],
    ...(name?.trim() ? { name: name.trim() } : {}),
  };
}

export function createUnsupportedPayload(blockType = "unsupported"): NotesUnsupportedBlockPayload {
  return { block_type: blockType.trim() || "unsupported" };
}

/** Create the typed payload shared by block creation and updates. */
function createDefaultBlockUpdate(
  type: NotesBlockType = "paragraph",
  content = "",
  color: NotesColor = DEFAULT_COLOR,
): NotesBlockUpdate {
  switch (type) {
    case "paragraph":
      return { type, paragraph: createTextPayload(content, color) };
    case "heading_1":
      return { type, heading_1: createTextPayload(content, color) };
    case "heading_2":
      return { type, heading_2: createTextPayload(content, color) };
    case "heading_3":
      return { type, heading_3: createTextPayload(content, color) };
    case "heading_4":
      return { type, heading_4: createTextPayload(content, color) };
    case "heading_5":
      return { type, heading_5: createTextPayload(content, color) };
    case "heading_6":
      return { type, heading_6: createTextPayload(content, color) };
    case "bulleted_list_item":
      return { type, bulleted_list_item: createTextPayload(content, color) };
    case "numbered_list_item":
      return { type, numbered_list_item: createTextPayload(content, color) };
    case "to_do":
      return { type, to_do: createTodoPayload(content, false, color) };
    case "toggle":
      return { type, toggle: createTogglePayload(content, true, color) };
    case "callout":
      return {
        type,
        callout: createCalloutPayload(
          content,
          color === DEFAULT_COLOR ? DEFAULT_CALLOUT_COLOR : color,
        ),
      };
    case "quote":
      return { type, quote: createTextPayload(content, color) };
    case "child_page":
      return { type, child_page: createChildPagePayload(content.trim()) };
    case "child_database":
      return { type, child_database: createChildDatabasePayload(content.trim()) };
    case "breadcrumb":
      return { type, breadcrumb: {} };
    case "table_of_contents":
      return { type, table_of_contents: { color } };
    case "column_list":
      return { type, column_list: {} };
    case "column":
      return { type, column: createColumnPayload() };
    case "table":
      return { type, table: createTablePayload() };
    case "table_row":
      return { type, table_row: createEmptyTableRowPayload() };
    case "tab":
      return { type, tab: createTabPayload() };
    case "image":
      return { type, image: createMediaPayload(content.trim()) };
    case "video":
      return { type, video: createMediaPayload(content.trim()) };
    case "audio":
      return { type, audio: createMediaPayload(content.trim()) };
    case "file":
      return { type, file: createMediaPayload(content.trim()) };
    case "pdf":
      return { type, pdf: createMediaPayload(content.trim()) };
    case "bookmark":
      return { type, bookmark: createBookmarkPayload(content.trim()) };
    case "link_preview":
      return { type, link_preview: createLinkPreviewPayload(content.trim()) };
    case "synced_block":
      return { type, synced_block: createSyncedBlockPayload() };
    case "template":
      return { type, template: createTemplatePayload(content) };
    case "button":
      return { type, button: createButtonPayload(content) };
    case "embed":
      return { type, embed: createEmbedPayload(content.trim()) };
    case "equation":
      return { type, equation: createEquationPayload(content) };
    case "divider":
      return { type, divider: {} };
    case "code":
      return { type, code: createCodePayload(content) };
    case "unsupported":
      return { type, unsupported: createUnsupportedPayload() };
  }
}

/** Build a Notion-shaped block create payload for the given type. */
export function createBlockWrite(
  id: string,
  type: NotesBlockType = "paragraph",
  content = "",
  color: NotesColor = DEFAULT_COLOR,
): NotesBlockWrite {
  return { id, ...createDefaultBlockUpdate(type, content, color) };
}

/** Build a block update without manufacturing a temporary block identity. */
export function createBlockUpdate(
  type: NotesBlockType = "paragraph",
  content = "",
  color: NotesColor = DEFAULT_COLOR,
  options: NotesTextPayloadOptions = {},
): NotesBlockUpdate {
  const update = createDefaultBlockUpdate(type, content, color);
  if (type === "heading_1" && update.type === "heading_1") {
    return {
      ...update,
      heading_1: {
        ...update.heading_1,
        ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
        ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
      },
    };
  }
  if (type === "heading_2" && update.type === "heading_2") {
    return {
      ...update,
      heading_2: {
        ...update.heading_2,
        ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
        ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
      },
    };
  }
  if (type === "heading_3" && update.type === "heading_3") {
    return {
      ...update,
      heading_3: {
        ...update.heading_3,
        ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
        ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
      },
    };
  }
  if (type === "heading_4" && update.type === "heading_4") {
    return {
      ...update,
      heading_4: {
        ...update.heading_4,
        ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
        ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
      },
    };
  }
  if (type === "heading_5" && update.type === "heading_5") {
    return {
      ...update,
      heading_5: {
        ...update.heading_5,
        ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
        ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
      },
    };
  }
  if (type === "heading_6" && update.type === "heading_6") {
    return {
      ...update,
      heading_6: {
        ...update.heading_6,
        ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
        ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
      },
    };
  }
  return update;
}

export function createTextPayloadFromRichText(
  richText: readonly NotesRichText[],
  color: NotesColor = DEFAULT_COLOR,
  options: Pick<NotesTextPayloadOptions, "isToggleable" | "open"> & {
    icon?: NotesTextBlockPayload["icon"];
  } = {},
): NotesTextBlockPayload {
  return {
    rich_text: richText.length > 0 ? [...richText] : [createRichText("")],
    color,
    ...(options.isToggleable === undefined ? {} : { is_toggleable: options.isToggleable }),
    ...(options.open === undefined ? {} : { ganbaru_open: options.open }),
    ...(options.icon === undefined ? {} : { icon: options.icon }),
  };
}
