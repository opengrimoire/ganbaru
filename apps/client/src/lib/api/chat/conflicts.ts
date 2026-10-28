/**
 * Bridge for Chat sync conflicts: participant names, teammate roles and instructions, channel names and topics, and
 * message edits.
 */

import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";
import {
  readArray,
  readBoolean,
  readEnum,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
} from "$lib/utils/readers";

/** Chat row kinds whose edits can conflict. */
export const CHAT_CONFLICT_TARGETS = ["participant", "teammate", "channel", "message"] as const;
export type ChatConflictTarget = (typeof CHAT_CONFLICT_TARGETS)[number];

/** One concurrent version of a conflicted Chat row. */
export interface ChatConflictVersion {
  /** Opaque id for choosing this version. */
  version: string;
  device: SyncDeviceRef;
  editedAtMs: number;
  /** Whether every device shows this version now. */
  displayed: boolean;
  /** A name, a role, or message Markdown. */
  text: string;
  /** A teammate's instructions or a channel's topic, or null when the row kind has none. */
  detail: string | null;
}

/** The versions of one conflicted Chat row, newest first. */
export interface ChatConflict {
  target: ChatConflictTarget;
  id: string;
  /** Where the row is, for a message its channel name, or null when the row names itself. */
  context: string | null;
  versions: ChatConflictVersion[];
}

/** Keep the version every device shows, or use another one. */
export type ChatConflictChoice = { kind: "displayed" } | { kind: "version"; version: string };

export interface ChatConflictResolution {
  target: ChatConflictTarget;
  id: string;
  choice: ChatConflictChoice;
}

function mapVersion(value: unknown, label: string): ChatConflictVersion {
  const row = readRecord(value, label);
  return {
    version: readString(row.version, `${label}.version`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs: readNonNegativeSafeInteger(row.editedAtMs, `${label}.editedAtMs`),
    displayed: readBoolean(row.displayed, `${label}.displayed`),
    text: readString(row.text, `${label}.text`),
    detail: readNullable(row.detail, `${label}.detail`, readString),
  };
}

function mapConflict(value: unknown, label: string): ChatConflict {
  const row = readRecord(value, label);
  return {
    target: readEnum(row.target, CHAT_CONFLICT_TARGETS, `${label}.target`),
    id: readString(row.id, `${label}.id`),
    context: readNullable(row.context, `${label}.context`, readString),
    versions: readArray(row.versions, `${label}.versions`, mapVersion),
  };
}

/**
 * Validates the Chat conflicts from the native side.
 *
 * @param value Untrusted command response.
 * @returns The conflicts, one per row.
 * @throws When the response does not match the conflict shape.
 */
export function mapChatConflicts(value: unknown): ChatConflict[] {
  return readArray(value, "chat conflicts", mapConflict);
}

/** Reads the open Chat conflicts of the active vault. */
export async function loadChatConflicts(): Promise<ChatConflict[]> {
  const dbUrl = await ensureDbUrl();
  return mapChatConflicts(await invoke<unknown>("chat_conflicts", { dbUrl }));
}

/**
 * Resolves a Chat conflict on every linked device. Rejects with a `not_found` Chat error when the conflict or version
 * is gone.
 */
export async function resolveChatConflict(resolution: ChatConflictResolution): Promise<void> {
  const dbUrl = await ensureDbUrl();
  await invoke<unknown>("chat_resolve_conflict", { dbUrl, resolution });
}
