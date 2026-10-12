import type { NotesCalloutIcon, NotesIcon, NotesMediaBlockPayload, NotesPageCover } from "./assets";
import type { NotesDataSource, NotesDatabase, NotesDatabaseMentionRichText, NotesDatabaseView } from "./database";

export const NOTES_BLOCK_TYPES = [
  "paragraph",
  "heading_1",
  "heading_2",
  "heading_3",
  "heading_4",
  "heading_5",
  "heading_6",
  "bulleted_list_item",
  "numbered_list_item",
  "to_do",
  "toggle",
  "callout",
  "quote",
  "child_page",
  "child_database",
  "breadcrumb",
  "table_of_contents",
  "column_list",
  "column",
  "table",
  "table_row",
  "tab",
  "image",
  "video",
  "audio",
  "file",
  "pdf",
  "bookmark",
  "link_preview",
  "synced_block",
  "template",
  "button",
  "embed",
  "equation",
  "divider",
  "code",
  "unsupported",
] as const;

export type NotesBlockType = (typeof NOTES_BLOCK_TYPES)[number];

export const NOTES_COLORS = [
  "default",
  "gray",
  "brown",
  "orange",
  "yellow",
  "green",
  "blue",
  "purple",
  "pink",
  "red",
  "gray_background",
  "brown_background",
  "orange_background",
  "yellow_background",
  "green_background",
  "blue_background",
  "purple_background",
  "pink_background",
  "red_background",
] as const;

export type NotesColor = (typeof NOTES_COLORS)[number];

export type NotesParent =
  | { type: "workspace"; workspace: true }
  | { type: "page_id"; page_id: string }
  | { type: "block_id"; block_id: string }
  | { type: "data_source_id"; data_source_id: string };

export interface NotesRichTextLink {
  url: string;
}

export interface NotesRichTextAnnotations {
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  underline: boolean;
  code: boolean;
  color: NotesColor;
}

export interface NotesTextRichText {
  type: "text";
  text: {
    content: string;
    link: NotesRichTextLink | null;
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export interface NotesPageMentionRichText {
  type: "mention";
  mention: {
    type: "page";
    page: {
      id: string;
    };
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export interface NotesUserMentionRichText {
  type: "mention";
  mention: {
    type: "user";
    user: {
      object: "user";
      id: string;
    };
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export const NOTES_LOCAL_OBJECT_MENTION_TYPES = [
  "project",
  "project_task",
  "calendar_event",
] as const;

export type NotesLocalObjectMentionType =
  (typeof NOTES_LOCAL_OBJECT_MENTION_TYPES)[number];

export interface NotesLocalObjectMentionRichText {
  type: "mention";
  mention: {
    type: "ganbaru_object";
    ganbaru_object: {
      type: NotesLocalObjectMentionType;
      id: string;
    };
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export interface NotesDateMentionReminder {
  enabled: boolean;
}

/** How a date mention spells its date once edited. */
export const NOTES_DATE_MENTION_FORMATS = ["full", "short", "numeric", "iso"] as const;

export type NotesDateMentionFormat = (typeof NOTES_DATE_MENTION_FORMATS)[number];

export interface NotesDateMentionValue {
  start: string;
  end?: string | null;
  time_zone?: string | null;
  ganbaru_reminder?: NotesDateMentionReminder | null;
  /** The spelling chosen in the date editor; mentions without one keep the text they were inserted with. */
  ganbaru_format?: NotesDateMentionFormat | null;
}

export interface NotesDateMentionRichText {
  type: "mention";
  mention: {
    type: "date";
    date: NotesDateMentionValue;
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export interface NotesEquationRichText {
  type: "equation";
  equation: {
    expression: string;
  };
  annotations: NotesRichTextAnnotations;
  plain_text: string;
  href: string | null;
}

export type NotesRichText =
  | NotesTextRichText
  | NotesPageMentionRichText
  | NotesUserMentionRichText
  | NotesDatabaseMentionRichText
  | NotesLocalObjectMentionRichText
  | NotesDateMentionRichText
  | NotesEquationRichText;

/** Visual indentation relative to the structural parent, independent of block content. */
export interface NotesBlockLayout {
  ganbaru_indent?: number;
}

export interface NotesTextBlockPayload extends NotesBlockLayout {
  rich_text: NotesRichText[];
  color?: NotesColor;
  is_toggleable?: boolean;
  ganbaru_open?: boolean;
  icon?: NotesIcon | null;
}

export interface NotesTodoBlockPayload extends NotesTextBlockPayload {
  checked: boolean;
}

export interface NotesToggleBlockPayload extends NotesTextBlockPayload {
  ganbaru_open?: boolean;
}

export type NotesPageIcon = NotesIcon;

export interface NotesCalloutBlockPayload extends NotesTextBlockPayload {
  icon: NotesCalloutIcon;
}

export interface NotesCodeBlockPayload extends NotesBlockLayout {
  rich_text: NotesRichText[];
  caption: NotesRichText[];
  language: string;
}

export interface NotesChildPageBlockPayload extends NotesBlockLayout {
  title: string;
}

export interface NotesChildDatabaseBlockPayload extends NotesBlockLayout {
  editing_locked?: boolean;
  title: string;
  database_id?: string;
  data_source_id?: string;
  view_id?: string;
}

export type NotesDividerBlockPayload = NotesBlockLayout & Record<string, unknown>;

export type NotesBreadcrumbBlockPayload = NotesBlockLayout & Record<string, unknown>;

export interface NotesTableOfContentsBlockPayload extends NotesBlockLayout {
  color?: NotesColor;
}

export type NotesColumnListBlockPayload = NotesBlockLayout & Record<string, unknown>;

export interface NotesColumnBlockPayload extends NotesBlockLayout {
  width_ratio?: number;
}

export interface NotesTableBlockPayload extends NotesBlockLayout {
  table_width: number;
  has_column_header: boolean;
  has_row_header: boolean;
}

export type NotesTableCell = NotesRichText[];

export interface NotesTableRowBlockPayload extends NotesBlockLayout {
  cells: NotesTableCell[];
}

export type NotesTabBlockPayload = NotesBlockLayout & Record<string, unknown>;

export interface NotesBookmarkBlockPayload extends NotesBlockLayout {
  caption: NotesRichText[];
  url: string;
}

export interface NotesLinkPreviewBlockPayload extends NotesBlockLayout {
  url: string;
}

export interface NotesSyncedBlockReference {
  type: "block_id";
  block_id: string;
}

export interface NotesSyncedBlockPayload extends NotesBlockLayout {
  synced_from: NotesSyncedBlockReference | null;
}

export interface NotesTemplateBlockPayload extends NotesBlockLayout {
  rich_text: NotesRichText[];
}

export const NOTES_BUTTON_INSERT_POSITIONS = [
  "below_button",
  "above_button",
  "top_of_page",
  "bottom_of_page",
] as const;

export type NotesButtonInsertPosition = (typeof NOTES_BUTTON_INSERT_POSITIONS)[number];

export interface NotesButtonInsertBlocksAction {
  type: "insert_blocks";
  source: "children";
  position: NotesButtonInsertPosition;
}

export type NotesButtonAction = NotesButtonInsertBlocksAction;

export interface NotesButtonBlockPayload extends NotesBlockLayout {
  rich_text: NotesRichText[];
  icon: NotesIcon | null;
  actions: NotesButtonAction[];
}

export interface NotesEmbedBlockPayload extends NotesBlockLayout {
  url: string;
}

export interface NotesEquationBlockPayload extends NotesBlockLayout {
  expression: string;
}

export type NotesUnsupportedBlockPayload = NotesBlockLayout & Record<string, unknown> & {
  block_type?: string;
  source_type?: string;
  raw?: Record<string, unknown>;
  warnings?: string[];
};

export interface NotesPage {
  object: "page";
  id: string;
  created_time: string;
  last_edited_time: string;
  parent: NotesParent;
  folder_id: string | null;
  in_trash: boolean;
  archived?: boolean;
  icon: NotesPageIcon | null;
  cover: NotesPageCover | null;
  properties: Record<string, unknown>;
  url: string | null;
  public_url: string | null;
  source_provider: string | null;
  source_object_id: string | null;
  source_workspace_id: string | null;
  source_last_edited_time: string | null;
}

export interface NotesFolder {
  object: "folder";
  id: string;
  project_id: string;
  parent_folder_id: string | null;
  name: string;
  created_time: string;
  last_edited_time: string;
}

export interface NotesPageTemplate {
  object: "page_template";
  id: string;
  name: string;
  source_page_id: string | null;
  properties: Record<string, unknown>;
  icon: NotesPageIcon | null;
  cover: NotesPageCover | null;
  block_count: number;
  created_time: string;
  last_edited_time: string;
}

interface NotesBlockBase<Type extends NotesBlockType> {
  object: "block";
  id: string;
  /** Opaque canonical revision. Optimistic blocks receive it after their first commit. */
  edit_revision?: string;
  parent: NotesParent;
  created_time: string;
  last_edited_time: string;
  has_children: boolean;
  in_trash: boolean;
  archived?: boolean;
  type: Type;
  source_provider: string | null;
  source_object_id: string | null;
  source_last_edited_time: string | null;
}

export type NotesParagraphBlock = NotesBlockBase<"paragraph"> & {
  paragraph: NotesTextBlockPayload;
};

export type NotesHeading1Block = NotesBlockBase<"heading_1"> & {
  heading_1: NotesTextBlockPayload;
};

export type NotesHeading2Block = NotesBlockBase<"heading_2"> & {
  heading_2: NotesTextBlockPayload;
};

export type NotesHeading3Block = NotesBlockBase<"heading_3"> & {
  heading_3: NotesTextBlockPayload;
};

export type NotesHeading4Block = NotesBlockBase<"heading_4"> & {
  heading_4: NotesTextBlockPayload;
};

export type NotesHeading5Block = NotesBlockBase<"heading_5"> & {
  heading_5: NotesTextBlockPayload;
};

export type NotesHeading6Block = NotesBlockBase<"heading_6"> & {
  heading_6: NotesTextBlockPayload;
};

export type NotesBulletedListItemBlock = NotesBlockBase<"bulleted_list_item"> & {
  bulleted_list_item: NotesTextBlockPayload;
};

export type NotesNumberedListItemBlock = NotesBlockBase<"numbered_list_item"> & {
  numbered_list_item: NotesTextBlockPayload;
};

export type NotesTodoBlock = NotesBlockBase<"to_do"> & {
  to_do: NotesTodoBlockPayload;
};

export type NotesToggleBlock = NotesBlockBase<"toggle"> & {
  toggle: NotesToggleBlockPayload;
};

export type NotesCalloutBlock = NotesBlockBase<"callout"> & {
  callout: NotesCalloutBlockPayload;
};

export type NotesQuoteBlock = NotesBlockBase<"quote"> & {
  quote: NotesTextBlockPayload;
};

export type NotesChildPageBlock = NotesBlockBase<"child_page"> & {
  child_page: NotesChildPageBlockPayload;
};

export type NotesChildDatabaseBlock = NotesBlockBase<"child_database"> & {
  child_database: NotesChildDatabaseBlockPayload;
};

export type NotesBreadcrumbBlock = NotesBlockBase<"breadcrumb"> & {
  breadcrumb: NotesBreadcrumbBlockPayload;
};

export type NotesTableOfContentsBlock = NotesBlockBase<"table_of_contents"> & {
  table_of_contents: NotesTableOfContentsBlockPayload;
};

export type NotesColumnListBlock = NotesBlockBase<"column_list"> & {
  column_list: NotesColumnListBlockPayload;
};

export type NotesColumnBlock = NotesBlockBase<"column"> & {
  column: NotesColumnBlockPayload;
};

export type NotesTableBlock = NotesBlockBase<"table"> & {
  table: NotesTableBlockPayload;
};

export type NotesTableRowBlock = NotesBlockBase<"table_row"> & {
  table_row: NotesTableRowBlockPayload;
};

export type NotesTabBlock = NotesBlockBase<"tab"> & {
  tab: NotesTabBlockPayload;
};

export type NotesImageBlock = NotesBlockBase<"image"> & {
  image: NotesMediaBlockPayload;
};

export type NotesVideoBlock = NotesBlockBase<"video"> & {
  video: NotesMediaBlockPayload;
};

export type NotesAudioBlock = NotesBlockBase<"audio"> & {
  audio: NotesMediaBlockPayload;
};

export type NotesFileBlock = NotesBlockBase<"file"> & {
  file: NotesMediaBlockPayload;
};

export type NotesPdfBlock = NotesBlockBase<"pdf"> & {
  pdf: NotesMediaBlockPayload;
};

export type NotesBookmarkBlock = NotesBlockBase<"bookmark"> & {
  bookmark: NotesBookmarkBlockPayload;
};

export type NotesLinkPreviewBlock = NotesBlockBase<"link_preview"> & {
  link_preview: NotesLinkPreviewBlockPayload;
};

export type NotesSyncedBlock = NotesBlockBase<"synced_block"> & {
  synced_block: NotesSyncedBlockPayload;
};

export type NotesTemplateBlock = NotesBlockBase<"template"> & {
  template: NotesTemplateBlockPayload;
};

export type NotesButtonBlock = NotesBlockBase<"button"> & {
  button: NotesButtonBlockPayload;
};

export type NotesEmbedBlock = NotesBlockBase<"embed"> & {
  embed: NotesEmbedBlockPayload;
};

export type NotesEquationBlock = NotesBlockBase<"equation"> & {
  equation: NotesEquationBlockPayload;
};

export type NotesDividerBlock = NotesBlockBase<"divider"> & {
  divider: NotesDividerBlockPayload;
};

export type NotesCodeBlock = NotesBlockBase<"code"> & {
  code: NotesCodeBlockPayload;
};

export type NotesUnsupportedBlock = NotesBlockBase<"unsupported"> & {
  unsupported: NotesUnsupportedBlockPayload;
};

export type NotesBlock =
  | NotesParagraphBlock
  | NotesHeading1Block
  | NotesHeading2Block
  | NotesHeading3Block
  | NotesHeading4Block
  | NotesHeading5Block
  | NotesHeading6Block
  | NotesBulletedListItemBlock
  | NotesNumberedListItemBlock
  | NotesTodoBlock
  | NotesToggleBlock
  | NotesCalloutBlock
  | NotesQuoteBlock
  | NotesChildPageBlock
  | NotesChildDatabaseBlock
  | NotesBreadcrumbBlock
  | NotesTableOfContentsBlock
  | NotesColumnListBlock
  | NotesColumnBlock
  | NotesTableBlock
  | NotesTableRowBlock
  | NotesTabBlock
  | NotesImageBlock
  | NotesVideoBlock
  | NotesAudioBlock
  | NotesFileBlock
  | NotesPdfBlock
  | NotesBookmarkBlock
  | NotesLinkPreviewBlock
  | NotesSyncedBlock
  | NotesTemplateBlock
  | NotesButtonBlock
  | NotesEmbedBlock
  | NotesEquationBlock
  | NotesDividerBlock
  | NotesCodeBlock
  | NotesUnsupportedBlock;

export type NotesBlockWrite =
  | { id: string; type: "paragraph"; paragraph: NotesTextBlockPayload }
  | { id: string; type: "heading_1"; heading_1: NotesTextBlockPayload }
  | { id: string; type: "heading_2"; heading_2: NotesTextBlockPayload }
  | { id: string; type: "heading_3"; heading_3: NotesTextBlockPayload }
  | { id: string; type: "heading_4"; heading_4: NotesTextBlockPayload }
  | { id: string; type: "heading_5"; heading_5: NotesTextBlockPayload }
  | { id: string; type: "heading_6"; heading_6: NotesTextBlockPayload }
  | { id: string; type: "bulleted_list_item"; bulleted_list_item: NotesTextBlockPayload }
  | { id: string; type: "numbered_list_item"; numbered_list_item: NotesTextBlockPayload }
  | { id: string; type: "to_do"; to_do: NotesTodoBlockPayload }
  | { id: string; type: "toggle"; toggle: NotesToggleBlockPayload }
  | { id: string; type: "callout"; callout: NotesCalloutBlockPayload }
  | { id: string; type: "quote"; quote: NotesTextBlockPayload }
  | { id: string; type: "child_page"; child_page: NotesChildPageBlockPayload }
  | { id: string; type: "child_database"; child_database: NotesChildDatabaseBlockPayload }
  | { id: string; type: "breadcrumb"; breadcrumb: NotesBreadcrumbBlockPayload }
  | {
      id: string;
      type: "table_of_contents";
      table_of_contents: NotesTableOfContentsBlockPayload;
    }
  | { id: string; type: "column_list"; column_list: NotesColumnListBlockPayload }
  | { id: string; type: "column"; column: NotesColumnBlockPayload }
  | { id: string; type: "table"; table: NotesTableBlockPayload }
  | { id: string; type: "table_row"; table_row: NotesTableRowBlockPayload }
  | { id: string; type: "tab"; tab: NotesTabBlockPayload }
  | { id: string; type: "image"; image: NotesMediaBlockPayload }
  | { id: string; type: "video"; video: NotesMediaBlockPayload }
  | { id: string; type: "audio"; audio: NotesMediaBlockPayload }
  | { id: string; type: "file"; file: NotesMediaBlockPayload }
  | { id: string; type: "pdf"; pdf: NotesMediaBlockPayload }
  | { id: string; type: "bookmark"; bookmark: NotesBookmarkBlockPayload }
  | { id: string; type: "link_preview"; link_preview: NotesLinkPreviewBlockPayload }
  | { id: string; type: "synced_block"; synced_block: NotesSyncedBlockPayload }
  | { id: string; type: "template"; template: NotesTemplateBlockPayload }
  | { id: string; type: "button"; button: NotesButtonBlockPayload }
  | { id: string; type: "embed"; embed: NotesEmbedBlockPayload }
  | { id: string; type: "equation"; equation: NotesEquationBlockPayload }
  | { id: string; type: "divider"; divider: NotesDividerBlockPayload }
  | { id: string; type: "code"; code: NotesCodeBlockPayload }
  | { id: string; type: "unsupported"; unsupported: NotesUnsupportedBlockPayload };

export type NotesBlockUpdate = NotesBlockWrite extends infer Block
  ? Block extends NotesBlockWrite
    ? Omit<Block, "id">
    : never
  : never;

export interface NotesPageCreate {
  id: string;
  title: string;
  parent: NotesParent;
  folder_id: string | null;
  first_block_id: string;
  after_block_id?: string | null;
  properties?: Record<string, unknown> | null;
}

export interface NotesChildPageFromBlockCreate {
  first_block_id: string;
  title?: string | null;
  properties?: Record<string, unknown> | null;
}

export interface NotesDuplicatePageRequest {
  title?: string | null;
}

export interface NotesMovePageRequest {
  parent: NotesParent;
  folder_id: string | null;
}

export interface NotesFolderCreate {
  id: string;
  project_id: string;
  parent_folder_id: string | null;
  name: string;
}

export interface NotesFolderUpdate {
  name: string;
  parent_folder_id: string | null;
}

export interface NotesPageTemplateCreateFromPageRequest {
  id: string;
  source_page_id: string;
  name: string;
}

export interface NotesPageTemplateApplyRequest {
  parent: NotesParent;
  title?: string | null;
}

export interface NotesPageTemplateUpdateRequest {
  name?: string;
  source_page_id?: string;
}

export interface NotesPageTemplateDuplicateRequest {
  id: string;
  name: string;
}

export interface NotesPageUpdate {
  title?: string;
  parent?: NotesParent;
  properties?: Record<string, unknown>;
  icon?: NotesPageIcon | null;
  cover?: NotesPageCover | null;
}

export interface NotesLinkedDatabaseCreateRequest {
  id: string;
  view_id: string;
  source_block_id: string;
  title?: string | null;
  parent?: NotesParent;
  after_block_id?: string | null;
  replace_block_id?: string | null;
}

export interface NotesDatabaseDuplicateRequest {
  id: string;
  source_block_id: string;
  parent?: NotesParent;
  after_block_id?: string | null;
  replace_block_id?: string | null;
}

/** Minimal database ownership metadata without loading row pages or view contents. */
export interface NotesDatabaseReference {
  editing_locked: boolean;
  block_id: string;
  page_id: string;
  source_block_id: string;
  source_page_id: string;
  title: string;
  owned_data_source_count: number;
  is_linked: boolean;
}

export interface NotesCreatedDatabase {
  database: NotesDatabase;
  data_source: NotesDataSource;
  view: NotesDatabaseView;
  block: NotesChildDatabaseBlock;
}

export interface NotesAppendBlockChildrenRequest {
  parent: NotesParent;
  after: string | null;
  children: NotesBlockWrite[];
}

export interface NotesMoveBlockRequest {
  parent: NotesParent;
  after: string | null;
  before?: string | null;
}

export interface NotesMoveBlocksRequest {
  block_ids: string[];
  parent: NotesParent;
  after: string | null;
  before?: string | null;
}

export interface NotesDuplicateBlockIdPair {
  source_id: string;
  duplicate_id: string;
}

export interface NotesDuplicateBlockRequest {
  duplicated_block_ids: NotesDuplicateBlockIdPair[];
}

export interface NotesDuplicateBlocksRequest {
  block_ids: string[];
  duplicated_block_ids: NotesDuplicateBlockIdPair[];
  parent: NotesParent;
  after: string | null;
  before?: string | null;
  include_trashed_sources?: boolean;
}

export interface NotesTrashBlocksRequest {
  block_ids: string[];
  in_trash?: boolean;
}
