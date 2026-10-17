// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { mobileIdentityPortal } from "./identity-slot";

describe("mobileIdentityPortal", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  function hostWithSwitcher(): { host: HTMLDivElement; switcher: HTMLDivElement } {
    const host = document.createElement("div");
    const switcher = document.createElement("div");
    host.append(switcher);
    document.body.append(host);
    return { host, switcher };
  }

  it("moves the switcher into the top bar slot when the shell provides one", () => {
    const { host, switcher } = hostWithSwitcher();
    const slot = document.createElement("div");
    document.body.append(slot);
    mobileIdentityPortal(switcher, slot);
    expect(switcher.parentElement).toBe(slot);
    expect(host.contains(switcher)).toBe(false);
  });

  it("keeps the switcher in place without a slot and returns it there when the slot disappears", () => {
    const { host, switcher } = hostWithSwitcher();
    const action = mobileIdentityPortal(switcher, null);
    expect(switcher.parentElement).toBe(host);

    const slot = document.createElement("div");
    action.update(slot);
    expect(switcher.parentElement).toBe(slot);
    action.update(null);
    expect(switcher.parentElement).toBe(host);
  });

  it("removes the switcher and its placeholder on destroy", () => {
    const { host, switcher } = hostWithSwitcher();
    const slot = document.createElement("div");
    const action = mobileIdentityPortal(switcher, slot);
    action.destroy();
    expect(slot.childNodes).toHaveLength(0);
    expect(host.childNodes).toHaveLength(0);
  });
});
