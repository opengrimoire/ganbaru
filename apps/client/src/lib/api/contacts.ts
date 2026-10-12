import { invoke } from "@tauri-apps/api/core";
import {
  readArray,
  readBoolean,
  readEnum,
  readNullable,
  readRecord,
  readSafeInteger,
  readString,
  readUtcTimestamp,
} from "$lib/chat/validation/readers";
import { parseQrMatrix, type PairingQrMatrix } from "./vault-handoff";

/** Wire trust scopes as the Rust side stores them. */
export const CONTACT_TRUST_KINDS = ["not_allowed", "once", "seven_days", "thirty_days", "until_revoked"] as const;
export type ContactTrustKind = (typeof CONTACT_TRUST_KINDS)[number];

export const CONTACT_STATES = ["active", "blocked"] as const;
export type ContactState = (typeof CONTACT_STATES)[number];

export const CONTACT_REQUEST_DIRECTIONS = ["sent", "received"] as const;
export type ContactRequestDirection = (typeof CONTACT_REQUEST_DIRECTIONS)[number];

export const CONTACT_REQUEST_STATES = ["pending", "accepted", "declined", "expired"] as const;
export type ContactRequestState = (typeof CONTACT_REQUEST_STATES)[number];

export const CONTACTS_ERROR_CODES = [
  "identity_unavailable",
  "key_unavailable",
  "profile_incomplete",
  "invalid_card",
  "card_revoked",
  "recipient_unreachable",
  "read_only",
  "revision_conflict",
  "failed",
] as const;
export type ContactsErrorCode = (typeof CONTACTS_ERROR_CODES)[number];

const MAX_COLOR = 31;

export interface LocalIdentity {
  publicKey: string;
  contactId: string;
  cardRevision: number;
  privateKeyAvailable: boolean;
}

export interface ContactCardData {
  text: string;
  qr: PairingQrMatrix;
  verificationCode: string;
  displayName: string;
  color: number;
  /** Empty when nobody can reach this person right now. */
  endpointHint: string;
}

export interface LocalContactCard {
  identity: LocalIdentity;
  /** Absent while this device holds no private key copy. */
  card: ContactCardData | null;
}

export interface ParsedContactCard {
  displayName: string;
  color: number;
  verificationCode: string;
  publicKey: string;
  contactId: string;
  hasEndpoint: boolean;
  isSelf: boolean;
  existingState: ContactState | null;
  pendingSent: boolean;
}

export interface Contact {
  id: string;
  publicKey: string;
  displayName: string;
  color: number;
  state: ContactState;
  inviteTrust: ContactTrustKind;
  inviteTrustExpiresAt: string | null;
  messageTrust: ContactTrustKind;
  messageTrustExpiresAt: string | null;
  acceptedAt: string | null;
  blockedAt: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
}

export interface ContactRequest {
  id: string;
  direction: ContactRequestDirection;
  publicKey: string;
  contactId: string;
  displayName: string;
  color: number;
  verificationCode: string;
  inviteTrust: ContactTrustKind;
  messageTrust: ContactTrustKind;
  state: ContactRequestState;
  expiresAt: string;
  lastAttemptAt: string | null;
  lastErrorCode: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
}

export interface ContactsSnapshot {
  identity: LocalIdentity | null;
  contacts: Contact[];
  requests: ContactRequest[];
}

export interface ContactTrustChoice {
  inviteTrust: ContactTrustKind;
  messageTrust: ContactTrustKind;
}

export interface ContactsRowRevision {
  id: string;
  expectedRevision: number;
}

/** A Contacts command failure with the stable code the UI branches on. */
export class ContactsError extends Error {
  readonly code: ContactsErrorCode;

  constructor(code: ContactsErrorCode, message: string) {
    super(message);
    this.name = "ContactsError";
    this.code = code;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Converts a rejected Contacts command into a typed error; unknown shapes become `failed`. */
export function normalizeContactsError(error: unknown): ContactsError {
  if (error instanceof ContactsError) return error;
  if (isRecord(error) && typeof error.code === "string" && typeof error.message === "string") {
    const code = (CONTACTS_ERROR_CODES as readonly string[]).includes(error.code)
      ? (error.code as ContactsErrorCode)
      : "failed";
    return new ContactsError(code, error.message);
  }
  if (error instanceof Error) return new ContactsError("failed", error.message);
  return new ContactsError("failed", typeof error === "string" ? error : "Contacts command failed");
}

function readColor(value: unknown, label: string): number {
  const color = readSafeInteger(value, label);
  if (color < 0 || color > MAX_COLOR) throw new Error(`${label} is outside the palette`);
  return color;
}

function readIdentity(value: unknown, label: string): LocalIdentity {
  const record = readRecord(value, label);
  return {
    publicKey: readString(record.publicKey, `${label}.publicKey`),
    contactId: readString(record.contactId, `${label}.contactId`),
    cardRevision: readSafeInteger(record.cardRevision, `${label}.cardRevision`),
    privateKeyAvailable: readBoolean(record.privateKeyAvailable, `${label}.privateKeyAvailable`),
  };
}

function readCard(value: unknown, label: string): ContactCardData {
  const record = readRecord(value, label);
  return {
    text: readString(record.text, `${label}.text`),
    qr: parseQrMatrix(record.qr),
    verificationCode: readString(record.verificationCode, `${label}.verificationCode`),
    displayName: readString(record.displayName, `${label}.displayName`),
    color: readColor(record.color, `${label}.color`),
    endpointHint: readString(record.endpointHint, `${label}.endpointHint`),
  };
}

export function parseLocalContactCard(value: unknown): LocalContactCard {
  const record = readRecord(value, "local card");
  return {
    identity: readIdentity(record.identity, "local card.identity"),
    card: readNullable(record.card, "local card.card", readCard),
  };
}

export function parseParsedContactCard(value: unknown): ParsedContactCard {
  const record = readRecord(value, "parsed card");
  return {
    displayName: readString(record.displayName, "parsed card.displayName"),
    color: readColor(record.color, "parsed card.color"),
    verificationCode: readString(record.verificationCode, "parsed card.verificationCode"),
    publicKey: readString(record.publicKey, "parsed card.publicKey"),
    contactId: readString(record.contactId, "parsed card.contactId"),
    hasEndpoint: readBoolean(record.hasEndpoint, "parsed card.hasEndpoint"),
    isSelf: readBoolean(record.isSelf, "parsed card.isSelf"),
    existingState: readNullable(record.existingState, "parsed card.existingState", (state, label) =>
      readEnum(state, CONTACT_STATES, label)),
    pendingSent: readBoolean(record.pendingSent, "parsed card.pendingSent"),
  };
}

function readContact(value: unknown, label: string): Contact {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    publicKey: readString(record.publicKey, `${label}.publicKey`),
    displayName: readString(record.displayName, `${label}.displayName`),
    color: readColor(record.color, `${label}.color`),
    state: readEnum(record.state, CONTACT_STATES, `${label}.state`),
    inviteTrust: readEnum(record.inviteTrust, CONTACT_TRUST_KINDS, `${label}.inviteTrust`),
    inviteTrustExpiresAt: readNullable(record.inviteTrustExpiresAt, `${label}.inviteTrustExpiresAt`, readUtcTimestamp),
    messageTrust: readEnum(record.messageTrust, CONTACT_TRUST_KINDS, `${label}.messageTrust`),
    messageTrustExpiresAt: readNullable(record.messageTrustExpiresAt, `${label}.messageTrustExpiresAt`, readUtcTimestamp),
    acceptedAt: readNullable(record.acceptedAt, `${label}.acceptedAt`, readUtcTimestamp),
    blockedAt: readNullable(record.blockedAt, `${label}.blockedAt`, readUtcTimestamp),
    revision: readSafeInteger(record.revision, `${label}.revision`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

function readRequest(value: unknown, label: string): ContactRequest {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    direction: readEnum(record.direction, CONTACT_REQUEST_DIRECTIONS, `${label}.direction`),
    publicKey: readString(record.publicKey, `${label}.publicKey`),
    contactId: readString(record.contactId, `${label}.contactId`),
    displayName: readString(record.displayName, `${label}.displayName`),
    color: readColor(record.color, `${label}.color`),
    verificationCode: readString(record.verificationCode, `${label}.verificationCode`),
    inviteTrust: readEnum(record.inviteTrust, CONTACT_TRUST_KINDS, `${label}.inviteTrust`),
    messageTrust: readEnum(record.messageTrust, CONTACT_TRUST_KINDS, `${label}.messageTrust`),
    state: readEnum(record.state, CONTACT_REQUEST_STATES, `${label}.state`),
    expiresAt: readUtcTimestamp(record.expiresAt, `${label}.expiresAt`),
    lastAttemptAt: readNullable(record.lastAttemptAt, `${label}.lastAttemptAt`, readUtcTimestamp),
    lastErrorCode: readNullable(record.lastErrorCode, `${label}.lastErrorCode`, readString),
    revision: readSafeInteger(record.revision, `${label}.revision`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseContactsSnapshot(value: unknown): ContactsSnapshot {
  const record = readRecord(value, "contacts snapshot");
  return {
    identity: readNullable(record.identity, "contacts snapshot.identity", readIdentity),
    contacts: readArray(record.contacts, "contacts snapshot.contacts", readContact),
    requests: readArray(record.requests, "contacts snapshot.requests", readRequest),
  };
}

async function invokeContacts<T>(command: string, parse: (value: unknown) => T, args?: Record<string, unknown>): Promise<T> {
  let value: unknown;
  try {
    value = await invoke<unknown>(command, args);
  } catch (error) {
    throw normalizeContactsError(error);
  }
  return parse(value);
}

/** Reads the local identity, contacts, and contact requests. */
export function listContacts(): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_list", parseContactsSnapshot);
}

/** Signs the current local card, creating the person identity on first use. */
export function loadLocalCard(): Promise<LocalContactCard> {
  return invokeContacts("contacts_local_card", parseLocalContactCard);
}

/** Rotates the card nonce so every card issued so far stops being accepted. */
export function regenerateLocalCard(): Promise<LocalContactCard> {
  return invokeContacts("contacts_regenerate_card", parseLocalContactCard);
}

/** Decodes and verifies pasted card text without writing anything. */
export function parseContactCard(text: string): Promise<ParsedContactCard> {
  return invokeContacts("contacts_parse_card", parseParsedContactCard, { text });
}

/** Decodes a bounded grayscale camera frame into contact card text. */
export function decodeContactCardQr(width: number, height: number, luma: Uint8Array): Promise<string> {
  return invokeContacts(
    "contacts_decode_card_qr",
    (value) => readString(value, "contact card QR"),
    { width, height, luma: Array.from(luma) },
  );
}

/** Delivers a contact request to the person named by the card. */
export function sendContactRequest(cardText: string, trust: ContactTrustChoice): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_send_request", parseContactsSnapshot, {
    request: { cardText, inviteTrust: trust.inviteTrust, messageTrust: trust.messageTrust },
  });
}

export function acceptContactRequest(row: ContactsRowRevision, trust: ContactTrustChoice): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_accept_request", parseContactsSnapshot, {
    request: { ...row, inviteTrust: trust.inviteTrust, messageTrust: trust.messageTrust },
  });
}

export function declineContactRequest(row: ContactsRowRevision): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_decline_request", parseContactsSnapshot, { request: row });
}

export function cancelContactRequest(row: ContactsRowRevision): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_cancel_request", parseContactsSnapshot, { request: row });
}

export function blockPerson(publicKey: string): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_block", parseContactsSnapshot, { request: { publicKey } });
}

export function unblockPerson(row: ContactsRowRevision): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_unblock", parseContactsSnapshot, { request: row });
}

export function removeContact(row: ContactsRowRevision): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_remove", parseContactsSnapshot, { request: row });
}

export function updateContactTrust(row: ContactsRowRevision, trust: ContactTrustChoice): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_update_trust", parseContactsSnapshot, {
    request: { ...row, inviteTrust: trust.inviteTrust, messageTrust: trust.messageTrust },
  });
}

/** Polls every pending sent request now instead of waiting for the background interval. */
export function syncContactRequests(): Promise<ContactsSnapshot> {
  return invokeContacts("contacts_sync_requests", parseContactsSnapshot);
}

/** Saves a PNG rendering of the card through the desktop save dialog. Resolves false when cancelled. */
export function saveCardImage(title: string, fileName: string, pngBase64: string): Promise<boolean> {
  return invokeContacts(
    "contacts_save_card_image",
    (value) => readBoolean(value, "save card image"),
    { title, fileName, pngBase64 },
  );
}
