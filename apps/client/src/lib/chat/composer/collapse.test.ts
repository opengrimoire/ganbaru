import { describe, expect, it } from "vitest";
import { composerCollapsed, type ComposerCollapseState } from "./collapse";

const idle: ComposerCollapseState = {
  collapsible: true,
  focusWithin: false,
  hasContent: false,
  optionSet: false,
  menuOpen: false,
  showsStatus: false,
};

describe("composerCollapsed", () => {
  it("collapses an idle, empty, unfocused composer that may collapse", () => {
    expect(composerCollapsed(idle)).toBe(true);
  });

  it("never collapses a composer that is not collapsible", () => {
    expect(composerCollapsed({ ...idle, collapsible: false })).toBe(false);
  });

  it.each([
    ["focus is inside it", { focusWithin: true }],
    ["the draft has content", { hasContent: true }],
    ["a send option is set", { optionSet: true }],
    ["a menu is open", { menuOpen: true }],
    ["it shows a status", { showsStatus: true }],
  ] satisfies [string, Partial<ComposerCollapseState>][])("stays open while %s", (_reason, change) => {
    expect(composerCollapsed({ ...idle, ...change })).toBe(false);
  });
});
