/**
 * Bridge for Chat message provenance: the devices that sealed a message, as the sync log records them.
 */

import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";
import { readArray, readRecord } from "$lib/utils/readers";

/** The devices that sealed a Chat message. */
export interface ChatMessageProvenance {
  /** Devices whose writers created the message, empty when sync holds no record of it. */
  devices: SyncDeviceRef[];
}

/**
 * Validates message provenance from the native side.
 *
 * @param value Untrusted command response.
 * @returns The provenance.
 * @throws When the response does not match the provenance shape.
 */
export function mapChatMessageProvenance(value: unknown): ChatMessageProvenance {
  const row = readRecord(value, "chat message provenance");
  return { devices: readArray(row.devices, "chat message provenance.devices", mapSyncDeviceRef) };
}

/**
 * Reads the devices that sealed an organizational message.
 *
 * @param messageItemId The message's conversation item id.
 */
export async function loadChatMessageProvenance(messageItemId: string): Promise<ChatMessageProvenance> {
  const dbUrl = await ensureDbUrl();
  return mapChatMessageProvenance(await invoke<unknown>("chat_message_provenance", { dbUrl, messageItemId }));
}
