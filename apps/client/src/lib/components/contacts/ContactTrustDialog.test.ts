// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Contact } from "$lib/api/contacts";
import ContactTrustDialog from "./ContactTrustDialog.svelte";

const contactsStore = vi.hoisted(() => ({
  updateTrust: vi.fn<(row: { id: string; expectedRevision: number }, trust: { inviteTrust: string; messageTrust: string }) => Promise<void>>(),
}));
vi.mock("$lib/stores/contacts.svelte", () => ({ getContacts: () => contactsStore }));

const timestamp = "2026-11-12T12:00:00.000Z";
const contact: Contact = {
  id: "contact:ana",
  publicKey: "key-ana",
  displayName: "Ana",
  color: 2,
  state: "active",
  inviteTrust: "not_allowed",
  inviteTrustExpiresAt: null,
  messageTrust: "not_allowed",
  messageTrustExpiresAt: null,
  acceptedAt: timestamp,
  blockedAt: null,
  revision: 4,
  createdAt: timestamp,
  updatedAt: timestamp,
};

describe("ContactTrustDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    document.body.replaceChildren();
    component = undefined;
    target = undefined;
    contactsStore.updateTrust.mockReset();
  });

  it("does not ask the parent to close again when a save finishes after the dialog closed", async () => {
    let finishSave: () => void = () => undefined;
    contactsStore.updateTrust.mockReturnValue(new Promise<void>((resolve) => { finishSave = resolve; }));
    const onClose = vi.fn<() => void>();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ContactTrustDialog, { target, props: { contact, onClose } });
    flushSync();

    document.querySelector<HTMLButtonElement>(".contacts-dialog-button.primary")?.click();
    flushSync();
    expect(contactsStore.updateTrust).toHaveBeenCalledWith({ id: contact.id, expectedRevision: 4 }, expect.anything());

    await unmount(component);
    component = undefined;
    finishSave();
    await Promise.resolve();
    await Promise.resolve();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes itself once a save finishes while it is open", async () => {
    contactsStore.updateTrust.mockResolvedValue();
    const onClose = vi.fn<() => void>();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ContactTrustDialog, { target, props: { contact, onClose } });
    flushSync();

    document.querySelector<HTMLButtonElement>(".contacts-dialog-button.primary")?.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(onClose).toHaveBeenCalledOnce();
  });
});
