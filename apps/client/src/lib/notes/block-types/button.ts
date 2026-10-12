import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import { childIdsForParent, type NotesTreeState } from "$lib/notes/blocks/tree";
import type {
  NotesButtonBlockPayload,
  NotesButtonInsertPosition,
  NotesIcon,
} from "$lib/notes/types";

export type NotesButtonUseUnavailableReason = "empty" | "child_page" | null;

export interface NotesButtonBlockStatus {
  childCount: number;
  containsChildPage: boolean;
  canUse: boolean;
  useUnavailableReason: NotesButtonUseUnavailableReason;
}

export const EMPTY_NOTES_BUTTON_BLOCK_STATUS: NotesButtonBlockStatus = {
  childCount: 0,
  containsChildPage: false,
  canUse: false,
  useUnavailableReason: "empty",
};

export const NOTES_BUTTON_NATIVE_ICON_CHOICES = [
  "mouse-pointer-click",
  "plus",
  "copy",
  "list-plus",
  "check",
  "star",
  "calendar-days",
  "lightbulb",
] as const;

export type NotesButtonNativeIconChoice = (typeof NOTES_BUTTON_NATIVE_ICON_CHOICES)[number];

export const NOTES_BUTTON_UNSUPPORTED_ACTIONS = [
  "database_edit",
  "webhook",
  "destructive_automation",
] as const;

export type NotesButtonUnsupportedAction = (typeof NOTES_BUTTON_UNSUPPORTED_ACTIONS)[number];

export const NOTES_BUTTON_UNSUPPORTED_ACTION_AVAILABILITY: Record<
  NotesButtonUnsupportedAction,
  false
> = {
  database_edit: false,
  webhook: false,
  destructive_automation: false,
};

export function activeButtonChildIds(state: NotesTreeState, blockId: string): string[] {
  return childIdsForParent(state, blockId).filter((childId) => {
    const child = state.blocksById[childId];
    return child !== undefined && !child.in_trash;
  });
}

export function notesButtonBlockStatus(
  state: NotesTreeState,
  blockId: string,
): NotesButtonBlockStatus {
  const childIds = activeButtonChildIds(state, blockId);
  const containsChildPage = childIds.some((childId) =>
    collectLoadedBlockSubtreeIds(state, childId).some(
      (subtreeId) => state.blocksById[subtreeId]?.type === "child_page",
    ),
  );
  const useUnavailableReason: NotesButtonUseUnavailableReason = childIds.length === 0
    ? "empty"
    : containsChildPage
      ? "child_page"
      : null;
  return {
    childCount: childIds.length,
    containsChildPage,
    canUse: useUnavailableReason === null,
    useUnavailableReason,
  };
}

export function notesButtonPrimaryInsertPosition(
  payload: NotesButtonBlockPayload,
): NotesButtonInsertPosition {
  return payload.actions[0]?.position ?? "below_button";
}

export function notesButtonWithIcon(
  payload: NotesButtonBlockPayload,
  icon: NotesIcon | null,
): NotesButtonBlockPayload {
  return {
    ...payload,
    icon,
    rich_text: [...payload.rich_text],
    actions: payload.actions.map((action) => ({ ...action })),
  };
}

export function notesButtonWithPrimaryInsertPosition(
  payload: NotesButtonBlockPayload,
  position: NotesButtonInsertPosition,
): NotesButtonBlockPayload {
  const [firstAction, ...restActions] = payload.actions;
  const updatedFirstAction = {
    type: "insert_blocks" as const,
    source: "children" as const,
    position,
  };
  return {
    ...payload,
    rich_text: [...payload.rich_text],
    icon: payload.icon,
    actions: [
      firstAction ? { ...firstAction, position } : updatedFirstAction,
      ...restActions.map((action) => ({ ...action })),
    ],
  };
}
