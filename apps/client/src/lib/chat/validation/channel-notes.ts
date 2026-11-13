import {
  CHAT_MAX_CHANNEL_NOTE_CHARS,
  CHAT_NOTE_PROPOSAL_STATES,
  type ChatChannelNoteRead,
  type ChatNoteProposalStateRead,
} from "../contracts";
import {
  readArray,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
  readUtcTimestamp,
} from "./readers";

const CHANNEL_NOTE_FIELDS: ReadonlySet<string> = new Set([
  "id",
  "channelId",
  "body",
  "sourceItemId",
  "proposedBy",
  "editedBy",
  "revision",
  "createdAt",
  "updatedAt",
]);

/** Refuses a record that carries a field outside `known`, so a changed backend shape fails loudly. */
function rejectUnknownFields(record: Record<string, unknown>, known: ReadonlySet<string>, label: string): void {
  for (const key of Object.keys(record)) {
    if (!known.has(key)) throw new Error(`${label}.${key} is not a known field`);
  }
}

/** Reads note text that is neither blank nor longer than the note bound, in characters. */
function readNoteBody(value: unknown, label: string): string {
  const body = readString(value, label);
  if (!body.trim()) throw new Error(`${label} must not be empty`);
  if ([...body].length > CHAT_MAX_CHANNEL_NOTE_CHARS) {
    throw new Error(`${label} holds at most ${CHAT_MAX_CHANNEL_NOTE_CHARS} characters`);
  }
  return body;
}

/** Parses one live channel note, refusing unknown fields and text beyond the note bound. */
export function parseChatChannelNote(value: unknown, label = "Chat channel note"): ChatChannelNoteRead {
  const record = readRecord(value, label);
  rejectUnknownFields(record, CHANNEL_NOTE_FIELDS, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    body: readNoteBody(record.body, `${label}.body`),
    sourceItemId: readNullable(record.sourceItemId, `${label}.sourceItemId`, readIdentifier),
    proposedBy: readNullable(record.proposedBy, `${label}.proposedBy`, readIdentifier),
    editedBy: readNullable(record.editedBy, `${label}.editedBy`, readIdentifier),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

/** Parses the live notes of one channel. */
export function parseChatChannelNotes(value: unknown, label = "Chat channel notes"): ChatChannelNoteRead[] {
  return readArray(value, label, (entry, path) => parseChatChannelNote(entry, path));
}

/** Parses the states of the note proposals one read named. */
export function parseChatNoteProposalStates(
  value: unknown,
  label = "Chat note proposal states",
): ChatNoteProposalStateRead[] {
  return readArray(value, label, (entry, path) => {
    const record = readRecord(entry, path);
    return {
      proposalItemId: readIdentifier(record.proposalItemId, `${path}.proposalItemId`),
      state: readEnum(record.state, CHAT_NOTE_PROPOSAL_STATES, `${path}.state`),
    };
  });
}
