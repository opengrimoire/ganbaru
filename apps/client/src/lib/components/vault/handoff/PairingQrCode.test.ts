// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PairingInvitation } from "$lib/api/vault-handoff";
import PairingQrCode from "./PairingQrCode.svelte";

const writeTextToClipboard = vi.fn<(value: string) => Promise<void>>();
vi.mock("$lib/utils/clipboard", () => ({ writeTextToClipboard: (value: string) => writeTextToClipboard(value) }));

const CODE = "ganbaru-pair:abcdefghijklmnopqrstuvwxyz";

function invitation(expiresInMs: number): PairingInvitation {
  return {
    invitation: CODE,
    qr: { width: 2, modules: [true, false, false, true] },
    endpoint: "192.168.1.2:4000",
    expiresAtMs: Date.now() + expiresInMs,
  };
}

describe("PairingQrCode", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    writeTextToClipboard.mockReset();
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    component = null;
  });

  it("shows the text code by its ends and copies the whole code", async () => {
    writeTextToClipboard.mockResolvedValue(undefined);
    component = mount(PairingQrCode, { target, props: { invitation: invitation(60_000), view: "code" } });
    flushSync();
    expect(target.querySelector('svg[role="img"]')).toBeNull();
    expect(target.querySelector("code")?.textContent).toBe("ganbaru-pair:abcd••••wxyz");
    target.querySelector("button")?.click();
    await vi.waitFor(() => expect(target.querySelector("button")?.textContent?.trim()).toBe("Pairing code copied"));
    expect(writeTextToClipboard).toHaveBeenCalledWith(CODE);
  });

  it("says when the code could not be copied", async () => {
    writeTextToClipboard.mockRejectedValue(new Error("denied"));
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    component = mount(PairingQrCode, { target, props: { invitation: invitation(60_000), view: "code" } });
    flushSync();
    target.querySelector("button")?.click();
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')?.textContent).toBe("The pairing code could not be copied."));
  });

  it("asks for a fresh invitation once when the code view expires", () => {
    const onExpired = vi.fn();
    component = mount(PairingQrCode, { target, props: { invitation: invitation(-1), view: "code", onExpired } });
    flushSync();
    flushSync();
    expect(onExpired).toHaveBeenCalledTimes(1);
  });
});
