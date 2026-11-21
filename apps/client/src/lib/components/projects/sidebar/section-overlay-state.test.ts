import { describe, expect, it } from "vitest";
import {
  NO_PROJECT_SECTION_MENU,
  transitionProjectSectionMenu,
  type ProjectSectionMenu,
} from "./section-overlay-state";

const folded: ProjectSectionMenu = { kind: "actions", moveOpen: false };
const unfolded: ProjectSectionMenu = { kind: "actions", moveOpen: true };
const add: ProjectSectionMenu = { kind: "add" };

describe("transitionProjectSectionMenu", () => {
  it("opens the actions menu folded from every state", () => {
    for (const state of [NO_PROJECT_SECTION_MENU, folded, unfolded, add]) {
      expect(transitionProjectSectionMenu(state, { type: "openActions" })).toEqual(folded);
    }
  });

  it("closes only the actions menu", () => {
    expect(transitionProjectSectionMenu(unfolded, { type: "closeActions" })).toBe(NO_PROJECT_SECTION_MENU);
    expect(transitionProjectSectionMenu(add, { type: "closeActions" })).toBe(add);
    expect(transitionProjectSectionMenu(NO_PROJECT_SECTION_MENU, { type: "closeActions" })).toBe(NO_PROJECT_SECTION_MENU);
  });

  it("folds and unfolds the group list only inside the actions menu", () => {
    expect(transitionProjectSectionMenu(folded, { type: "toggleMove" })).toEqual(unfolded);
    expect(transitionProjectSectionMenu(unfolded, { type: "toggleMove" })).toEqual(folded);
    expect(transitionProjectSectionMenu(add, { type: "toggleMove" })).toBe(add);
    expect(transitionProjectSectionMenu(NO_PROJECT_SECTION_MENU, { type: "toggleMove" })).toBe(NO_PROJECT_SECTION_MENU);
  });

  it("toggles the add-view menu and lets it replace the actions menu", () => {
    expect(transitionProjectSectionMenu(NO_PROJECT_SECTION_MENU, { type: "toggleAdd" })).toEqual(add);
    expect(transitionProjectSectionMenu(unfolded, { type: "toggleAdd" })).toEqual(add);
    expect(transitionProjectSectionMenu(add, { type: "toggleAdd" })).toBe(NO_PROJECT_SECTION_MENU);
  });

  it("closes only the add-view menu", () => {
    expect(transitionProjectSectionMenu(add, { type: "closeAdd" })).toBe(NO_PROJECT_SECTION_MENU);
    expect(transitionProjectSectionMenu(folded, { type: "closeAdd" })).toBe(folded);
  });
});
