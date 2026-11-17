import {
  CHAT_PUSH_COMMIT_MAX_LISTED_PATHS,
  CHAT_PUSH_REQUEST_STATES,
  type ChatPushApproval,
  type ChatPushRequest,
  type ChatPushRequestCommit,
  type ChatPushRequestStateRead,
} from "../contracts";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
  readUtcTimestamp,
} from "./readers";

/** Longest push URL Ganbaru records, in UTF-8 bytes. */
const MAX_PUSH_URL_BYTES = 2_048;
const OBJECT_ID_PATTERN = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const DIGEST_PATTERN = /^[0-9a-f]{64}$/;
const UNSAFE_URL_CHARACTER_PATTERN = /[\p{Cc}\s]/u;
/** Longest commit message Ganbaru records, in characters. */
const MAX_COMMIT_MESSAGE_CHARS = 5_000;
/** Longest listed path or author Ganbaru records, in UTF-8 bytes. */
const MAX_COMMIT_FIELD_BYTES = 1_024;
const CONTROL_CHARACTER_PATTERN = /\p{Cc}/u;
const MESSAGE_CONTROL_CHARACTER_PATTERN = /[^\P{Cc}\n\t]/u;

/**
 * Reads the push target of an agent's push request. Requests replicate from other devices, so every
 * field is checked against the shape Ganbaru writes, and a push URL that carries a password is refused.
 */
export function parseChatPushRequest(value: unknown, label = "Chat push request"): ChatPushRequest {
  const record = readRecord(value, label);
  return {
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    executionEnvironmentId: readNullable(record.executionEnvironmentId, `${label}.executionEnvironmentId`, readIdentifier),
    localBranch: readIdentifier(record.localBranch, `${label}.localBranch`),
    remote: readIdentifier(record.remote, `${label}.remote`),
    destinationBranch: readIdentifier(record.destinationBranch, `${label}.destinationBranch`),
    pushUrl: readPushUrl(record.pushUrl, `${label}.pushUrl`),
    objectId: readPattern(record.objectId, OBJECT_ID_PATTERN, `${label}.objectId`),
    gitConfigDigest: readPattern(record.gitConfigDigest, DIGEST_PATTERN, `${label}.gitConfigDigest`),
    ...(record.commit === undefined ? {} : { commit: parseChatPushRequestCommit(record.commit, `${label}.commit`) }),
  };
}

/** Reads where each named push request stands. */
export function parseChatPushRequestStates(
  value: unknown,
  label = "Chat push request states",
): ChatPushRequestStateRead[] {
  return readArray(value, label, (entry, path) => {
    const record = readRecord(entry, path);
    return {
      requestItemId: readIdentifier(record.requestItemId, `${path}.requestItemId`),
      state: readEnum(record.state, CHAT_PUSH_REQUEST_STATES, `${path}.state`),
    };
  });
}

/**
 * Reads an agent's standing push approvals. Approvals replicate from other devices, so each push URL is
 * checked like a push request's.
 */
export function parseChatPushApprovals(value: unknown, label = "Chat push approvals"): ChatPushApproval[] {
  return readArray(value, label, (entry, path) => {
    const record = readRecord(entry, path);
    return {
      approvalId: readIdentifier(record.approvalId, `${path}.approvalId`),
      agentId: readIdentifier(record.agentId, `${path}.agentId`),
      workingFolderId: readIdentifier(record.workingFolderId, `${path}.workingFolderId`),
      workingFolderName: readString(record.workingFolderName, `${path}.workingFolderName`),
      pushUrl: readPushUrl(record.pushUrl, `${path}.pushUrl`),
      destinationBranch: readIdentifier(record.destinationBranch, `${path}.destinationBranch`),
      approvedAt: readUtcTimestamp(record.approvedAt, `${path}.approvedAt`),
    };
  });
}

function readPattern(value: unknown, pattern: RegExp, label: string): string {
  const text = readString(value, label);
  if (!pattern.test(text)) throw new Error(`${label} is malformed`);
  return text;
}

/** A bounded, printable URL whose user information, when present, names no password. */
function readPushUrl(value: unknown, label: string): string {
  const url = readString(value, label);
  if (!url || new TextEncoder().encode(url).byteLength > MAX_PUSH_URL_BYTES || UNSAFE_URL_CHARACTER_PATTERN.test(url)) {
    throw new Error(`${label} is malformed`);
  }
  const schemeEnd = url.indexOf("://");
  if (schemeEnd === -1) return url;
  const afterScheme = url.slice(schemeEnd + 3);
  const authorityEnd = afterScheme.search(/[/?#]/);
  const authority = authorityEnd === -1 ? afterScheme : afterScheme.slice(0, authorityEnd);
  const userInfoEnd = authority.lastIndexOf("@");
  if (userInfoEnd !== -1 && authority.slice(0, userInfoEnd).includes(":")) {
    throw new Error(`${label} must not carry a password`);
  }
  return url;
}

/** Reads the commit a push request asks Ganbaru to make, with the bounds Rust writes it within. */
function parseChatPushRequestCommit(value: unknown, label: string): ChatPushRequestCommit {
  const record = readRecord(value, label);
  const message = readString(record.message, `${label}.message`);
  if (
    !message
    || message !== message.trim()
    || [...message].length > MAX_COMMIT_MESSAGE_CHARS
    || MESSAGE_CONTROL_CHARACTER_PATTERN.test(message)
  ) {
    throw new Error(`${label}.message is malformed`);
  }
  const changedPaths = readArray(record.changedPaths, `${label}.changedPaths`, readCommitField);
  const changedPathCount = readNonNegativeSafeInteger(record.changedPathCount, `${label}.changedPathCount`);
  if (changedPaths.length > CHAT_PUSH_COMMIT_MAX_LISTED_PATHS || changedPathCount < Math.max(changedPaths.length, 1)) {
    throw new Error(`${label}.changedPaths is malformed`);
  }
  return {
    message,
    treeId: readPattern(record.treeId, OBJECT_ID_PATTERN, `${label}.treeId`),
    author: readCommitField(record.author, `${label}.author`),
    signed: readBoolean(record.signed, `${label}.signed`),
    changedPaths,
    changedPathCount,
  };
}

/** A bounded, nonempty text without control characters. */
function readCommitField(value: unknown, label: string): string {
  const text = readString(value, label);
  if (!text || new TextEncoder().encode(text).byteLength > MAX_COMMIT_FIELD_BYTES || CONTROL_CHARACTER_PATTERN.test(text)) {
    throw new Error(`${label} is malformed`);
  }
  return text;
}
