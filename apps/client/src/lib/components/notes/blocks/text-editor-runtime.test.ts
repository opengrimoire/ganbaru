import { describe, expect, it } from "vitest";
import {
  planNotesTextInputMenuState,
  requestedNotesTextControls,
} from "./text-editor-runtime.svelte";

describe("Notes text editor runtime", () => {
  it("requests only controls made visible by the current editor state", () => {
    expect(requestedNotesTextControls({
      textContextMenu: true,
      linkEditor: false,
      mentionMenu: true,
      slashMenu: false,
      templateControls: false,
      buttonControls: true,
      dateMentionEditor: false,
    })).toEqual(["text-context-menu", "mention-menu", "button-controls"]);
  });

  it("keeps every optional control behind its own lazy request", () => {
    expect(requestedNotesTextControls({
      textContextMenu: true,
      linkEditor: true,
      mentionMenu: true,
      slashMenu: true,
      templateControls: true,
      buttonControls: true,
      dateMentionEditor: true,
    })).toEqual([
      "text-context-menu",
      "link-editor",
      "mention-menu",
      "slash-menu",
      "template-controls",
      "button-controls",
      "date-mention-editor",
    ]);
  });

  it("opens slash search from text input without requiring a keydown event", () => {
    expect(planNotesTextInputMenuState("/", { start: 1, end: 1 }, false, true, ""))
      .toEqual({ slashOpen: true, mentionQuery: null });
    expect(planNotesTextInputMenuState("/heading", { start: 8, end: 8 }, true, true, "/"))
      .toEqual({ slashOpen: true, mentionQuery: null });
    expect(planNotesTextInputMenuState("/heading", { start: 8, end: 8 }, false, true, "/head"))
      .toEqual({ slashOpen: false, mentionQuery: null });
    expect(planNotesTextInputMenuState("/", { start: 1, end: 1 }, false, false, ""))
      .toEqual({ slashOpen: false, mentionQuery: null });
  });

  it("keeps slash and mention sessions mutually exclusive", () => {
    expect(planNotesTextInputMenuState(
      "/table",
      { start: 6, end: 6 },
      true,
      true,
    )).toEqual({ slashOpen: true, mentionQuery: null });

    expect(planNotesTextInputMenuState(
      "@page",
      { start: 5, end: 5 },
      false,
      true,
    )).toEqual({
      slashOpen: false,
      mentionQuery: { start: 0, end: 5, query: "page" },
    });
  });

  it("suppresses mention state for code blocks and unknown selections", () => {
    expect(planNotesTextInputMenuState(
      "@page",
      { start: 5, end: 5 },
      false,
      false,
    )).toEqual({ slashOpen: false, mentionQuery: null });
    expect(planNotesTextInputMenuState("@page", null, false, true))
      .toEqual({ slashOpen: false, mentionQuery: null });
  });
});
