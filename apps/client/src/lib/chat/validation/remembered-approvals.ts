import {
  CHAT_REMEMBERED_APPROVAL_KINDS,
  type ChatChannelRememberedApprovalRead,
  type ChatRememberableApprovalRead,
} from "../contracts";
import { readArray, readEnum, readIdentifier, readRecord, readString, readUtcTimestamp } from "./readers";

/** Longest remembered approval subject Ganbaru records, in UTF-8 bytes. */
const MAX_SUBJECT_KEY_BYTES = 2_048;
const CONTROL_CHARACTER_PATTERN = /\p{Cc}/u;

function readSubjectKey(value: unknown, label: string): string {
  const key = readString(value, label);
  if (!key.trim() || new TextEncoder().encode(key).byteLength > MAX_SUBJECT_KEY_BYTES || CONTROL_CHARACTER_PATTERN.test(key)) {
    throw new Error(`${label} must be a non-empty line of at most ${MAX_SUBJECT_KEY_BYTES} bytes`);
  }
  return key;
}

/** Reads the rule a request offers to remember. */
export function parseChatRememberableApproval(
  value: unknown,
  label = "Chat rememberable approval",
): ChatRememberableApprovalRead {
  const record = readRecord(value, label);
  return {
    kind: readEnum(record.kind, CHAT_REMEMBERED_APPROVAL_KINDS, `${label}.kind`),
    key: readSubjectKey(record.key, `${label}.key`),
  };
}

/**
 * Reads a channel's remembered approvals. Approvals replicate from other devices, so each subject is
 * checked against the shape Ganbaru writes before it is shown.
 */
export function parseChatRememberedApprovals(
  value: unknown,
  label = "Chat remembered approvals",
): ChatChannelRememberedApprovalRead[] {
  return readArray(value, label, (entry, path) => {
    const record = readRecord(entry, path);
    return {
      approvalId: readIdentifier(record.approvalId, `${path}.approvalId`),
      channelId: readIdentifier(record.channelId, `${path}.channelId`),
      agentId: readIdentifier(record.agentId, `${path}.agentId`),
      ...parseChatRememberableApproval(record, path),
      approvedAt: readUtcTimestamp(record.approvedAt, `${path}.approvedAt`),
    };
  });
}
