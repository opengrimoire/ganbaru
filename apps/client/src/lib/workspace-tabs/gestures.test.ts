import { describe, expect, it } from "vitest";
import { openInNewTabIntent, type OpenInNewTabEvent } from "./gestures";

function click(overrides: Partial<OpenInNewTabEvent> = {}): OpenInNewTabEvent {
  return { type: "click", button: 0, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...overrides };
}

describe("open in new tab gestures", () => {
  it("ignores a plain click", () => {
    expect(openInNewTabIntent(click(), "Linux x86_64")).toBeNull();
  });

  it("opens in the background on Ctrl+click and in front on Ctrl+Shift+click", () => {
    expect(openInNewTabIntent(click({ ctrlKey: true }), "Linux x86_64")).toEqual({ background: true });
    expect(openInNewTabIntent(click({ ctrlKey: true, shiftKey: true }), "Win32")).toEqual({ background: false });
  });

  it("uses Cmd instead of Ctrl on Apple platforms", () => {
    expect(openInNewTabIntent(click({ metaKey: true }), "MacIntel")).toEqual({ background: true });
    expect(openInNewTabIntent(click({ ctrlKey: true }), "MacIntel")).toBeNull();
    expect(openInNewTabIntent(click({ metaKey: true }), "Linux x86_64")).toBeNull();
  });

  it("rejects Alt and Shift on their own", () => {
    expect(openInNewTabIntent(click({ ctrlKey: true, altKey: true }), "Linux x86_64")).toBeNull();
    expect(openInNewTabIntent(click({ shiftKey: true }), "Linux x86_64")).toBeNull();
  });

  it("opens on a middle click, in front when Shift is held", () => {
    expect(openInNewTabIntent(click({ type: "auxclick", button: 1 }), "Linux x86_64")).toEqual({ background: true });
    expect(openInNewTabIntent(click({ type: "auxclick", button: 1, shiftKey: true }), "Linux x86_64")).toEqual({ background: false });
  });

  it("ignores other auxiliary buttons", () => {
    expect(openInNewTabIntent(click({ type: "auxclick", button: 2 }), "Linux x86_64")).toBeNull();
    expect(openInNewTabIntent(click({ type: "auxclick", button: 3 }), "Linux x86_64")).toBeNull();
  });
});
