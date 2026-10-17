// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import {
  CLOSED_NOTES_BLOCK_HANDLE_MENUS,
  notesBlockHandleActionMenuStyle,
  notesBlockContextMenuPoint,
  notesBlockHandleMenuStateAfterAction,
  notesBlockHandleMenuStateAfterMoveToggle,
} from "./handle";
import type { NotesBlockHandleAction } from "./handle";

describe("notes block context menu routing", () => {
  it("opens block actions from the current row surface", () => {
    const row = document.createElement("div");
    row.dataset.notesSelectableBlockId = "parent";
    const surface = document.createElement("div");
    row.append(surface);
    let point: { x: number; y: number } | null = null;
    row.addEventListener("contextmenu", (event) => {
      point = notesBlockContextMenuPoint(event);
    });

    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 120,
      clientY: 90,
    });
    surface.dispatchEvent(event);

    expect(point).toEqual({ x: 120, y: 90 });
    expect(event.defaultPrevented).toBe(true);
  });

  it("leaves nested rows and editable text to their own menus", () => {
    const row = document.createElement("div");
    row.dataset.notesSelectableBlockId = "parent";
    const nested = document.createElement("div");
    nested.dataset.notesSelectableBlockId = "child";
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");
    row.append(nested, editor);
    const points: ({ x: number; y: number } | null)[] = [];
    row.addEventListener("contextmenu", (event) => {
      points.push(notesBlockContextMenuPoint(event));
    });

    const nestedEvent = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    const editorEvent = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    nested.dispatchEvent(nestedEvent);
    editor.dispatchEvent(editorEvent);

    expect(points).toEqual([null, null]);
    expect(nestedEvent.defaultPrevented).toBe(false);
    expect(editorEvent.defaultPrevented).toBe(false);
  });

  it("keeps copy and color actions visible while closing nested move state", () => {
    const state = {
      menuOpen: true,
      moveMenuOpen: true,
    };

    expect(notesBlockHandleMenuStateAfterAction(state, "copy_link")).toEqual({
      menuOpen: true,
      moveMenuOpen: false,
    });
    expect(notesBlockHandleMenuStateAfterAction(state, "color")).toEqual({
      menuOpen: true,
      moveMenuOpen: false,
    });
  });

  it("closes all handle menus for actions that hand focus back to the editor", () => {
    const state = {
      menuOpen: true,
      moveMenuOpen: true,
    };
    const closingActions: NotesBlockHandleAction[] = [
      "turn_into",
      "duplicate",
      "comment",
      "move_up",
      "move_down",
      "move_to_page",
      "delete",
    ];

    for (const action of closingActions) {
      expect(notesBlockHandleMenuStateAfterAction(state, action)).toEqual(
        CLOSED_NOTES_BLOCK_HANDLE_MENUS,
      );
    }
  });

  it("toggles the move target panel inside the action menu", () => {
    const actionMenuOpen = {
      menuOpen: true,
      moveMenuOpen: false,
    };

    expect(notesBlockHandleMenuStateAfterMoveToggle(actionMenuOpen)).toEqual({
      menuOpen: true,
      moveMenuOpen: true,
    });
    expect(
      notesBlockHandleMenuStateAfterMoveToggle(
        {
          ...actionMenuOpen,
          moveMenuOpen: true,
        },
      ),
    ).toEqual(actionMenuOpen);
  });

  it("clamps the action menu inside narrow viewports", () => {
    const style = notesBlockHandleActionMenuStyle({
      triggerRect: {
        top: 120,
        right: 284,
        bottom: 144,
        left: 260,
      },
      viewportWidth: 280,
      viewportHeight: 180,
    });

    expect(style).toContain("left:32px");
    expect(style).toContain("width:240px");
    expect(style).toContain("max-height:108px");
  });
});
