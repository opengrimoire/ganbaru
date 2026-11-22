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
  type LocalContactCard,
  type LocalIdentity,
  type ContactsRowRevision,
  type ContactsSnapshot,
  type ContactTrustChoice,
} from "$lib/api/contacts";
import { listenSyncApplied } from "$lib/api/sync";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";

/**
 * Replicated tables whose changes alter the contacts snapshot: the lists, and the identity's card revision that the
 * card table holds.
 */
const CONTACT_LIST_TABLES: ReadonlySet<string> = new Set(["contacts", "contact_requests", "contact_card"]);
/** Replicated table that holds the card nonce and revision. */
const CONTACT_CARD_TABLE = "contact_card";

let identity = $state<LocalIdentity | null>(null);
let card = $state<ContactCardData | null>(null);
let contacts = $state<Contact[]>([]);
let requests = $state<ContactRequest[]>([]);
let loaded = $state(false);
let loading = $state(false);
let cardLoading = $state(false);
/**
 * The last command failure and whether a read or a write failed. A later read clears only a read failure, so a
 * background refresh never hides why an action failed; a later successful write clears either.
 */
let failure = $state.raw<{ kind: CommandKind; error: ContactsError } | null>(null);
let cardError = $state<ContactsError | null>(null);
let generation = 0;
let replicationListenerStarted = false;

/**
 * Orders the results of overlapping commands. A write's result always applies, because it carries that write, and
 * takes the next stamp when it lands. A read's result applies only when the read started after the shown result
 * was stamped, and keeps its start stamp, so an older read never replaces a newer write or read.
 */
interface ResultOrder {
  stamps: number;
  shown: number;
}

const snapshotOrder: ResultOrder = { stamps: 0, shown: 0 };
const cardOrder: ResultOrder = { stamps: 0, shown: 0 };
let pendingLoads = 0;
let pendingCardCommands = 0;

type CommandKind = "read" | "write";

/** Stamp taken when a command starts. */
function startCommand(order: ResultOrder): number {
  order.stamps += 1;
  return order.stamps;
}

/** Whether a result of a command that started at `started` may replace what is shown, and records it if so. */
function admitResult(order: ResultOrder, kind: CommandKind, started: number): boolean {
  if (kind === "read") {
    if (started <= order.shown) return false;
    order.shown = started;
    return true;
  }
  order.stamps += 1;
  order.shown = order.stamps;
  return true;
}

onActiveVaultIdentityChange(() => {
  generation += 1;
  identity = null;
  card = null;
  contacts = [];
  requests = [];
  loaded = false;
  pendingLoads = 0;
  pendingCardCommands = 0;
  loading = false;
  cardLoading = false;
  failure = null;
  cardError = null;
});

function applySnapshot(snapshot: ContactsSnapshot): void {
  identity = snapshot.identity;
  contacts = snapshot.contacts;
  requests = snapshot.requests;
  loaded = true;
}

/**
 * Runs a snapshot-returning command, keeping the result only if the vault did not change meanwhile and, for a read,
 * no newer result landed first.
 */
async function runSnapshotCommand(run: () => Promise<ContactsSnapshot>, kind: CommandKind): Promise<void> {
  const expected = generation;
  const started = startCommand(snapshotOrder);
  try {
    const snapshot = await run();
    if (expected !== generation || !admitResult(snapshotOrder, kind, started)) return;
    applySnapshot(snapshot);
    if (kind === "write" || failure?.kind === "read") failure = null;
  } catch (cause) {
    const error = normalizeContactsError(cause);
    if (expected === generation && (kind === "write" || started > snapshotOrder.shown)) failure = { kind, error };
    throw error;
  }
}

function mutate(run: () => Promise<ContactsSnapshot>): Promise<void> {
  return runSnapshotCommand(run, "write");
}

async function refresh(): Promise<void> {
  const expected = generation;
  pendingLoads += 1;
  loading = true;
  try {
    await runSnapshotCommand(listContacts, "read");
  } catch {
    // The error is already recorded for the UI.
  } finally {
    if (expected === generation) {
      pendingLoads -= 1;
      loading = pendingLoads > 0;
    }
  }
}

async function ensureLoaded(): Promise<void> {
  startContactsReplicationListener();
  if (loaded || loading) return;
  await refresh();
}

/**
 * Reload what this window already shows whenever sync applies contacts rows. The snapshot, with the identity's card
 * revision, reloads only once loaded, and the card is signed again only when it is open, so a replica never creates
 * state it has not asked for.
 */
function startContactsReplicationListener(): void {
  if (replicationListenerStarted) return;
  replicationListenerStarted = true;
  void listenSyncApplied((tables) => {
    if (loaded && tables.some((table) => CONTACT_LIST_TABLES.has(table))) void refresh();
    if (card !== null && tables.includes(CONTACT_CARD_TABLE)) {
      void runCardCommand(loadLocalCard, "read").catch(() => undefined);
    }
  }).catch((error: unknown) => {
    replicationListenerStarted = false;
    console.error("Contacts could not listen for sync", error);
  });
}

/** Runs a card command, ordered like {@link runSnapshotCommand}: signing the current card reads, regenerating writes. */
async function runCardCommand(run: () => Promise<LocalContactCard>, kind: CommandKind): Promise<void> {
  const expected = generation;
  const started = startCommand(cardOrder);
  pendingCardCommands += 1;
  cardLoading = true;
  try {
    const view = await run();
    if (expected !== generation || !admitResult(cardOrder, kind, started)) return;
    identity = view.identity;
    card = view.card;
    cardError = null;
  } catch (cause) {
    const cardFailure = normalizeContactsError(cause);
    if (expected === generation && (kind === "write" || started > cardOrder.shown)) {
      card = null;
      cardError = cardFailure;
    }
    throw cardFailure;
  } finally {
    if (expected === generation) {
      pendingCardCommands -= 1;
      cardLoading = pendingCardCommands > 0;
    }
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
    get error(): ContactsError | null { return failure?.error ?? null; },
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
      failure = null;
    },
    /** Signs the current card; creates the identity the first time the card is opened. */
    loadCard(): Promise<void> {
      startContactsReplicationListener();
      return runCardCommand(loadLocalCard, "read");
    },
    regenerateCard(): Promise<void> {
      return runCardCommand(regenerateLocalCard, "write");
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
        // Recorded in `failure`; the periodic native poll retries on its own.
      }
    },
  };
}
