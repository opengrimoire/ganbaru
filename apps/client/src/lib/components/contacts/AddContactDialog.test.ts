// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from "vitest";
import type { ParsedContactCard } from "$lib/api/contacts";
import AddContactDialog from "./AddContactDialog.svelte";

const contactsApi = vi.hoisted(() => ({
  parseContactCard: vi.fn<(text: string) => Promise<ParsedContactCard>>(),
  sendRequest: vi.fn<(text: string) => Promise<void>>(),
}));

vi.mock("$lib/api/contacts", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/contacts")>(),
  parseContactCard: contactsApi.parseContactCard,
}));

const scanner = vi.hoisted(() => ({ mobile: false, fail: () => {} }));

vi.mock("$lib/platform", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/platform")>();
  return {
    ...actual,
    get BUILD_PLATFORM_PROFILE() {
      return scanner.mobile ? actual.platformProfileFor("android") : actual.BUILD_PLATFORM_PROFILE;
    },
  };
});

vi.mock("$lib/components/vault/handoff/MobilePairingScanner.svelte", () => ({
  default: (_anchor: unknown, props: { onError: (cause: unknown) => void }) => {
    scanner.fail = () => props.onError(new Error("camera busy"));
  },
}));

vi.mock("$lib/stores/contacts.svelte", () => ({
  getContacts: () => ({ sendRequest: contactsApi.sendRequest }),
}));

function card(displayName: string, verificationCode: string): ParsedContactCard {
  return {
    displayName,
    color: 3,
    verificationCode,
    publicKey: `key-${displayName}`,
    contactId: `contact-${displayName}`,
    hasEndpoint: true,
    isSelf: false,
    existingState: null,
    pendingSent: false,
  };
}

describe("AddContactDialog", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    contactsApi.parseContactCard.mockReset();
    contactsApi.sendRequest.mockReset();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(AddContactDialog, { target, props: { onClose: () => {} } });
    flushSync();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  function typeCode(text: string): void {
    const input = document.querySelector<HTMLTextAreaElement>("textarea");
    if (!input) throw new Error("The code field did not render");
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  function sendButton(): HTMLButtonElement {
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim() === "Send request");
    if (!button) throw new Error("The send button did not render");
    return button;
  }

  it("hides the previous card and refuses to send while a changed code is still being read", async () => {
    contactsApi.parseContactCard.mockResolvedValueOnce(card("Alice", "1111"));
    typeCode("GANB-alice");
    await vi.waitFor(() => expect(document.body.textContent).toContain("1111"));
    expect(sendButton().disabled).toBe(false);

    contactsApi.parseContactCard.mockReturnValueOnce(new Promise<ParsedContactCard>(() => {}));
    typeCode("GANB-bob");

    expect(document.body.textContent).not.toContain("1111");
    expect(sendButton().disabled).toBe(true);
  });

  it("drops a send failure that lands after the code changed", async () => {
    contactsApi.parseContactCard.mockResolvedValueOnce(card("Alice", "1111"));
    typeCode("GANB-alice");
    await vi.waitFor(() => expect(sendButton().disabled).toBe(false));
    let failSend = () => {};
    contactsApi.sendRequest.mockReturnValueOnce(new Promise<void>((_resolve, reject) => {
      failSend = () => reject(new Error("offline"));
    }));
    sendButton().click();
    flushSync();

    contactsApi.parseContactCard.mockReturnValueOnce(new Promise<ParsedContactCard>(() => {}));
    typeCode("GANB-bob");
    failSend();
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();

    expect(document.body.textContent).not.toContain("The request could not be sent");
  });

  it("replaces a failed scan notice with what a pasted code says", async () => {
    if (component) await unmount(component);
    scanner.mobile = true;
    onTestFinished(() => { scanner.mobile = false; });
    component = mount(AddContactDialog, { target: target ?? document.body, props: { onClose: () => {} } });
    flushSync();
    const scanButton = [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim() === "Scan QR code");
    if (!scanButton) throw new Error("The scan button did not render");
    scanButton.click();
    flushSync();
    scanner.fail();
    flushSync();
    expect(document.body.textContent).toContain("The camera could not be started");

    contactsApi.parseContactCard.mockResolvedValueOnce({ ...card("Alice", "1111"), existingState: "active" });
    typeCode("GANB-alice");
    await vi.waitFor(() => expect(document.body.textContent).toContain("1111"));

    expect(document.body.textContent).toContain("Already a contact");
    expect(document.body.textContent).not.toContain("The camera could not be started");
  });
});
