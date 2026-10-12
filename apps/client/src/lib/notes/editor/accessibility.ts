export function notesRichTextEditorDomId(blockId: string): string {
  return `notes-rich-editor-${blockId}`;
}

export function notesRichTextEditorStatusDomId(blockId: string): string {
  return `notes-rich-editor-status-${blockId}`;
}

export function notesMentionMenuDomId(blockId: string): string {
  return `notes-mention-menu-${blockId}`;
}

export function notesMentionOptionDomId(blockId: string, index: number): string {
  return `notes-mention-option-${blockId}-${index}`;
}

export function notesSlashMenuDomId(blockId: string): string {
  return `notes-slash-menu-${blockId}`;
}

export function notesSlashMenuItemDomId(blockId: string, index: number): string {
  return `notes-slash-menu-${blockId}-item-${Math.max(0, Math.trunc(index))}`;
}

export function notesRichTextEditorControls(
  blockId: string,
  mentionOpen: boolean,
  slashOpen: boolean,
): string | undefined {
  if (mentionOpen) return notesMentionMenuDomId(blockId);
  if (slashOpen) return notesSlashMenuDomId(blockId);
  return undefined;
}

export function notesRichTextEditorActiveDescendant(
  blockId: string,
  mentionOpen: boolean,
  activeIndex: number,
  resultCount: number,
): string | undefined {
  if (!mentionOpen || activeIndex < 0 || activeIndex >= resultCount) return undefined;
  return notesMentionOptionDomId(blockId, activeIndex);
}
