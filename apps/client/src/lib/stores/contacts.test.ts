import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ContactsSnapshot, LocalContactCard, LocalIdentity } from "$lib/api/contacts";

const api = vi.hoisted(() => ({
  listContacts: vi.fn(),
  blockPerson: vi.fn(),
  loadLocalCard: vi.fn(),
  regenerateLocalCard: vi.fn(),
}));

vi.mock("$lib/api/contacts", async (importOriginal) => {
  const original = await importOriginal<typeof import("$lib/api/contacts")>();
  return {
    ...original,
    listContacts: api.listContacts,
    blockPerson: api.blockPerson,
    loadLocalCard: api.loadLocalCard,
    regenerateLocalCard: api.regenerateLocalCard,
  };
});

vi.mock("$lib/api/sync", () => ({ listenSyncApplied: vi.fn(async () => () => undefined) }));

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((settle) => { resolve = settle; });
  return { promise, resolve };
}

function identity(cardRevision: number): LocalIdentity {
  return { publicKey: "key", contactId: "contact", cardRevision, privateKeyAvailable: true };
}

function snapshot(cardRevision: number): ContactsSnapshot {
  return { identity: identity(cardRevision), contacts: [], requests: [] };
}

function cardView(cardRevision: number): LocalContactCard {
  return { identity: identity(cardRevision) };
}

async function load() {
  const { getContacts } = await import("./contacts.svelte");
  return getContacts();
}

describe("contacts store", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("keeps a mutation's snapshot over a refresh that started before it finished", async () => {
    const read = deferred<ContactsSnapshot>();
    api.listContacts.mockReturnValueOnce(read.promise);
    api.blockPerson.mockResolvedValueOnce(snapshot(2));
    const contacts = await load();

    const refreshing = contacts.refresh();
    await contacts.block("key");
    read.resolve(snapshot(1));
    await refreshing;

    expect(contacts.identity?.cardRevision).toBe(2);
  });

  it("keeps the latest refresh when an earlier one finishes after it", async () => {
    const earlier = deferred<ContactsSnapshot>();
    const later = deferred<ContactsSnapshot>();
    api.listContacts.mockReturnValueOnce(earlier.promise).mockReturnValueOnce(later.promise);
    const contacts = await load();

    const first = contacts.refresh();
    const second = contacts.refresh();
    later.resolve(snapshot(3));
    await second;
    earlier.resolve(snapshot(1));
    await first;

    expect(contacts.identity?.cardRevision).toBe(3);
  });

  it("stays loading until every overlapping refresh finishes", async () => {
    const earlier = deferred<ContactsSnapshot>();
    const later = deferred<ContactsSnapshot>();
    api.listContacts.mockReturnValueOnce(earlier.promise).mockReturnValueOnce(later.promise);
    const contacts = await load();

    const first = contacts.refresh();
    const second = contacts.refresh();
    earlier.resolve(snapshot(1));
    await first;
    expect(contacts.loading).toBe(true);

    later.resolve(snapshot(2));
    await second;
    expect(contacts.loading).toBe(false);
  });

  it("stays card loading until every overlapping card command finishes", async () => {
    const opened = deferred<LocalContactCard>();
    const regenerated = deferred<LocalContactCard>();
    api.loadLocalCard.mockReturnValueOnce(opened.promise);
    api.regenerateLocalCard.mockReturnValueOnce(regenerated.promise);
    const contacts = await load();

    const opening = contacts.loadCard();
    const regenerating = contacts.regenerateCard();
    regenerated.resolve(cardView(4));
    await regenerating;
    expect(contacts.cardLoading).toBe(true);

    opened.resolve(cardView(3));
    await opening;
    expect(contacts.cardLoading).toBe(false);
    expect(contacts.identity?.cardRevision).toBe(4);
  });
});
