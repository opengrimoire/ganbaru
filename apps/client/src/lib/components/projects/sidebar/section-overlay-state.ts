import type { ProjectView } from "$lib/projects/types";

/** The one floating menu a project section shows: its actions, with the group list folded or unfolded, or its add-view menu. */
export type ProjectSectionMenu =
  | { kind: "none" }
  | { kind: "actions"; moveOpen: boolean }
  | { kind: "add" };

export type ProjectSectionMenuEvent =
  | { type: "openActions" }
  | { type: "closeActions" }
  | { type: "toggleMove" }
  | { type: "toggleAdd" }
  | { type: "closeAdd" };

/** The confirmation a project section asks for before a destructive action. */
export type ProjectSectionConfirmation =
  | { kind: "none" }
  | { kind: "deleteProject" }
  | { kind: "removeView"; view: ProjectView };

export const NO_PROJECT_SECTION_MENU: ProjectSectionMenu = { kind: "none" };
export const NO_PROJECT_SECTION_CONFIRMATION: ProjectSectionConfirmation = { kind: "none" };

/**
 * Applies one menu event. Opening either menu replaces the other, the actions menu always opens with its group list
 * folded, and an event aimed at a menu that is not open returns the same state.
 */
export function transitionProjectSectionMenu(
  state: ProjectSectionMenu,
  event: ProjectSectionMenuEvent,
): ProjectSectionMenu {
  switch (event.type) {
    case "openActions":
      return { kind: "actions", moveOpen: false };
    case "closeActions":
      return state.kind === "actions" ? NO_PROJECT_SECTION_MENU : state;
    case "toggleMove":
      return state.kind === "actions" ? { kind: "actions", moveOpen: !state.moveOpen } : state;
    case "toggleAdd":
      return state.kind === "add" ? NO_PROJECT_SECTION_MENU : { kind: "add" };
    case "closeAdd":
      return state.kind === "add" ? NO_PROJECT_SECTION_MENU : state;
  }
}
