import {
  acceptContactRequest,
  blockPerson,
  cancelContactRequest,
  declineContactRequest,
  listContacts,
  loadLocalCard,
  normalizeContactsError,
  regenerateLocalCard,
  removeContact,
  sendContactRequest,
  syncContactRequests,
  unblockPerson,
  updateContactTrust,
  type ContactCardData,
  type Contact,
  type ContactRequest,
  type ContactsError,
  type LocalIdentity,
  type ContactsRowRevision,
  type ContactsSnapshot,
  type ContactTrustChoice,
} from "$lib/api/contacts";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";

let identity = $state<LocalIdentity | null>(null);
let card = $state<ContactCardData | null>(null);
let contacts = $state<Contact[]>([]);
let requests = $state<ContactRequest[]>([]);
let loaded = $state(false);
let loading = $state(false);
let cardLoading = $state(false);
let error = $state<ContactsError | null>(null);
let cardError = $state<ContactsError | null>(null);
let generation = 0;

onActiveVaultIdentityChange(() => {
  generation += 1;
  identity = null;
  card = null;
  contacts = [];
  requests = [];
  loaded = false;
  loading = false;
  cardLoading = false;
  error = null;
  cardError = null;
});

function applySnapshot(snapshot: ContactsSnapshot): void {
  identity = snapshot.identity;
  contacts = snapshot.contacts;
  requests = snapshot.requests;
  loaded = true;
}

/** Runs a snapshot-returning command, keeping the result only if the vault did not change meanwhile. */
async function mutate(run: () => Promise<ContactsSnapshot>): Promise<void> {
  const expected = generation;
  try {
    const snapshot = await run();
    if (expected !== generation) return;
    applySnapshot(snapshot);
    error = null;
  } catch (cause) {
    const failure = normalizeContactsError(cause);
    if (expected === generation) error = failure;
    throw failure;
  }
}

async function refresh(): Promise<void> {
  const expected = generation;
  loading = true;
  try {
    await mutate(listContacts);
  } catch {
    // The error is already recorded for the UI.
  } finally {
    if (expected === generation) loading = false;
  }
}

async function ensureLoaded(): Promise<void> {
  if (loaded || loading) return;
  await refresh();
}

async function runCardCommand(run: () => Promise<Awaited<ReturnType<typeof loadLocalCard>>>): Promise<void> {
  const expected = generation;
  cardLoading = true;
  try {
    const view = await run();
    if (expected !== generation) return;
    identity = view.identity;
    card = view.card;
    cardError = null;
  } catch (cause) {
    const failure = normalizeContactsError(cause);
    if (expected === generation) {
      card = null;
      cardError = failure;
    }
    throw failure;
  } finally {
    if (expected === generation) cardLoading = false;
  }
}

function filterContacts(list: readonly Contact[], query: string): Contact[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return [...list];
  return list.filter((contact) => contact.displayName.toLocaleLowerCase().includes(needle));
}

/** Contacts runtime state: the local card, contacts, requests, and the actions that change them. */
export function getContacts() {
  return {
    get identity(): LocalIdentity | null { return identity; },
    get card(): ContactCardData | null { return card; },
    get contacts(): readonly Contact[] { return contacts; },
    get requests(): readonly ContactRequest[] { return requests; },
    get loaded(): boolean { return loaded; },
    get loading(): boolean { return loading; },
    get cardLoading(): boolean { return cardLoading; },
    get error(): ContactsError | null { return error; },
    get cardError(): ContactsError | null { return cardError; },
    get activeContacts(): Contact[] {
      return contacts.filter((contact) => contact.state === "active");
    },
    get blocked(): Contact[] {
      return contacts.filter((contact) => contact.state === "blocked");
    },
    get receivedPending(): ContactRequest[] {
      return requests.filter((request) => request.direction === "received" && request.state === "pending");
    },
    get sentPending(): ContactRequest[] {
      return requests.filter((request) => request.direction === "sent" && request.state === "pending");
    },
    filter(query: string): Contact[] {
      return filterContacts(contacts.filter((contact) => contact.state === "active"), query);
    },
    ensureLoaded,
    refresh,
    clearError(): void {
      error = null;
    },
    /** Signs the current card; creates the identity the first time the card is opened. */
    loadCard(): Promise<void> {
      return runCardCommand(loadLocalCard);
    },
    regenerateCard(): Promise<void> {
      return runCardCommand(regenerateLocalCard);
    },
    sendRequest(cardText: string, trust: ContactTrustChoice): Promise<void> {
      return mutate(() => sendContactRequest(cardText, trust));
    },
    accept(row: ContactsRowRevision, trust: ContactTrustChoice): Promise<void> {
      return mutate(() => acceptContactRequest(row, trust));
    },
    decline(row: ContactsRowRevision): Promise<void> {
      return mutate(() => declineContactRequest(row));
    },
    cancel(row: ContactsRowRevision): Promise<void> {
      return mutate(() => cancelContactRequest(row));
    },
    block(publicKey: string): Promise<void> {
      return mutate(() => blockPerson(publicKey));
    },
    unblock(row: ContactsRowRevision): Promise<void> {
      return mutate(() => unblockPerson(row));
    },
    updateTrust(row: ContactsRowRevision, trust: ContactTrustChoice): Promise<void> {
      return mutate(() => updateContactTrust(row, trust));
    },
    remove(row: ContactsRowRevision): Promise<void> {
      return mutate(() => removeContact(row));
    },
    /** Polls pending sent requests now; failures stay silent because the background poll retries. */
    async syncRequests(): Promise<void> {
      try {
        await mutate(syncContactRequests);
      } catch {
        // Recorded in `error`; the periodic native poll retries on its own.
      }
    },
  };
}
