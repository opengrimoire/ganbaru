// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { lockAncestorScroll } from "./scroll-lock";

describe("lockAncestorScroll", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  function tree(): { outer: HTMLElement; plain: HTMLElement; inner: HTMLElement; modal: HTMLElement } {
    const outer = document.createElement("main");
    outer.style.overflowY = "auto";
    const plain = document.createElement("section");
    const inner = document.createElement("div");
    inner.style.overflowY = "scroll";
    const modal = document.createElement("div");
    inner.append(modal);
    plain.append(inner);
    outer.append(plain);
    document.body.append(outer);
    return { outer, plain, inner, modal };
  }

  it("hides the overflow of every scrolling ancestor and leaves the others alone", () => {
    const { outer, plain, inner, modal } = tree();
    lockAncestorScroll(modal);
    expect(outer.style.overflowY).toBe("hidden");
    expect(inner.style.overflowY).toBe("hidden");
    expect(plain.style.overflowY).toBe("");
  });

  it("restores the inline overflow each ancestor had", () => {
    const { outer, inner, modal } = tree();
    const unlock = lockAncestorScroll(modal);
    unlock();
    expect(outer.style.overflowY).toBe("auto");
    expect(inner.style.overflowY).toBe("scroll");
  });

  it("does nothing for an element without scrolling ancestors", () => {
    const modal = document.createElement("div");
    document.body.append(modal);
    const unlock = lockAncestorScroll(modal);
    unlock();
    expect(document.body.style.overflowY).toBe("");
  });
});
