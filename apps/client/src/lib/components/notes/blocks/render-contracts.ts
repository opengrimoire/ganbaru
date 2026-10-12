import type { ComponentProps } from "svelte";
import type NotesBlockRow from "./NotesBlockRow.svelte";
import type NotesColumnListBlock from "./NotesColumnListBlock.svelte";
import type NotesTabBlock from "./NotesTabBlock.svelte";

type RowProps = ComponentProps<typeof NotesBlockRow>;
type ColumnProps = ComponentProps<typeof NotesColumnListBlock>;
type TabProps = ComponentProps<typeof NotesTabBlock>;

export type NotesBlockRenderState = Pick<RowProps,
  | "breadcrumbItems"
  | "tableOfContentsItems"
  | "focusBlockId"
  | "focusRequestId"
  | "focusSelection"
  | "listOrdinals"
  | "mentionTargets"
>;

export type NotesBlockRenderActions = Pick<RowProps,
  | "onTextInput" | "onReplaceRichText" | "onInsertPageMention"
  | "onInsertDateMention" | "onInsertObjectMention" | "onApplyTextLink"
  | "onInsertInlineEquation" | "onPastePlainText" | "onPasteRichHtml"
  | "onApplyTextAnnotations" | "onCreateInlineComment" | "onCreateInlineSuggestion"
  | "onKeyboardAction" | "onUndo" | "onRedo" | "onAddBelow" | "onConvert"
  | "onConvertToToggleHeading" | "onColorChange" | "onCalloutIconChange" | "onCopyLink" | "onDuplicate"
  | "onUseTemplate" | "onAddTemplateChild" | "onUseButton" | "onAddButtonChild"
  | "onButtonIconChange" | "onButtonInsertPositionChange" | "onCreateLinkedDatabaseView"
  | "onConvertUnsupported" | "onComment" | "onMoveUp" | "onMoveDown"
  | "onMoveToPage" | "onDelete" | "onToggleTodo" | "onToggleOpen"
  | "onCodeLanguageChange" | "onBookmarkChange" | "onLinkPreviewUrlChange"
  | "onEmbedUrlChange" | "onEquationExpressionChange" | "onMediaChange"
  | "onTableCellRichTextChange" | "onAddTableRow" | "onRemoveTableRow"
  | "onAddTableColumn" | "onRemoveTableColumn" | "onSelectPage" | "onFocusBlock"
  | "onHandleMenuOpenChange"
>;

export type NotesBlockDragBindings = Pick<ColumnProps,
  | "draggingBlockId" | "dropPositionForBlock" | "onDragStart" | "onDragEnd"
  | "onDragOver" | "onDragLeave" | "onDrop"
>;

export type NotesBlockRenderLookups = Pick<ColumnProps,
  | "tableRowsForBlock" | "previousBlockTypeForBlock" | "isOnlyBlockForBlock"
  | "moveTargetsForBlock" | "templateStatusForBlock" | "buttonStatusForBlock"
>;

export type NotesColumnRenderActions = Pick<ColumnProps,
  | "onAddColumn" | "onRemoveColumn" | "onMoveColumn" | "onResizeColumn"
  | "onMoveBlockToColumn"
>;

export type NotesTabRenderActions = Pick<TabProps,
  | "onUpdateTabLabel" | "onUpdateTabIcon" | "onAddTab" | "onRemoveTab"
  | "onMoveTab" | "onMoveBlockToTab"
>;
