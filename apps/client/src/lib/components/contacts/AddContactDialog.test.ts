// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
});
