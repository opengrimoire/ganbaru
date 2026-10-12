// @vitest-environment jsdom
import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CellHarness from "./CollectionCellHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

/** Mount a cell and return its handlers so tests can observe which control a click reaches. */
function mountCell(primary: "button" | "input" | "disabled-input") {
  const onPrimary = vi.fn();
  const onSecondary = vi.fn();
  component = mount(CellHarness, { target: document.body, props: { primary, onPrimary, onSecondary } });
  return { onPrimary, onSecondary };
}

function element(testId: string): HTMLElement {
  return document.querySelector<HTMLElement>(`[data-testid="${testId}"]`)!;
}

describe("collection cell primary field", () => {
  it("focuses the primary text field when the click lands on cell padding or static content", () => {
    mountCell("input");
    const input = document.querySelector("input")!;
    element("cell").click();
    expect(document.activeElement).toBe(input);
    input.blur();
    element("static").click();
    expect(document.activeElement).toBe(input);
  });

  it("leaves clicks on other controls to those controls", () => {
    const { onSecondary } = mountCell("input");
    element("secondary").click();
    expect(onSecondary).toHaveBeenCalledOnce();
    expect(document.activeElement).not.toBe(document.querySelector("input"));
  });

  it("does not synthesize clicks on primary buttons, whose hit area already covers the cell", () => {
    const { onPrimary } = mountCell("button");
    element("cell").click();
    expect(onPrimary).not.toHaveBeenCalled();
  });

  it("ignores disabled primary fields", () => {
    mountCell("disabled-input");
    element("cell").click();
    expect(document.activeElement).toBe(document.body);
  });
});
