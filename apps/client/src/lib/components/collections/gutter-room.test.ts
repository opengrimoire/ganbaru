// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { collectionGutterRoom } from "./gutter-room";

function placed(element: HTMLElement, left: number): HTMLElement {
  element.getBoundingClientRect = () => new DOMRect(left, 0, 100, 20);
  return element;
}

describe("collection gutter room", () => {
  afterEach(() => { document.body.replaceChildren(); });

  it("measures up to the nearest ancestor that clips horizontal overflow", () => {
    const outer = placed(document.createElement("div"), 0);
    outer.style.overflowX = "hidden";
    const scroller = placed(document.createElement("div"), 40);
    scroller.style.overflowX = "auto";
    const padded = placed(document.createElement("div"), 40);
    const content = placed(document.createElement("div"), 72);
    padded.append(content);
    scroller.append(padded);
    outer.append(scroller);
    document.body.append(outer);

    expect(collectionGutterRoom(content)).toEqual({ room: 32, clip: scroller });
  });

  it("reports no room when the content starts at or before the clipping edge", () => {
    const scroller = placed(document.createElement("div"), 50);
    scroller.style.overflowX = "clip";
    const content = placed(document.createElement("div"), 40);
    scroller.append(content);
    document.body.append(scroller);

    expect(collectionGutterRoom(content).room).toBe(0);
  });

  it("falls back to the viewport edge when no ancestor clips", () => {
    const content = placed(document.createElement("div"), 24);
    document.body.append(content);

    expect(collectionGutterRoom(content)).toEqual({ room: 24, clip: null });
  });
});
