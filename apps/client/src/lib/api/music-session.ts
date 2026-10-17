import { invoke, type Channel } from "@tauri-apps/api/core";
import { parseNativeMusicSnapshot, type NativeMusicCommand, type NativeMusicObservation, type NativeMusicStart } from "$lib/music/session/native-session";

/** Reads the one active native session, restoring only this device's portable checkpoint. */
export async function musicSessionSnapshot() { return parseNativeMusicSnapshot(await invoke<unknown>("music_session_snapshot")); }
/** Starts a canonical native queue from IDs or an explicitly selected temporary source. */
export async function musicSessionStart(request: NativeMusicStart) { return parseNativeMusicSnapshot(await invoke<unknown>("music_session_start", { request })); }
/** Retries must reuse the same command object and action ID until its outcome is known. */
export async function musicSessionCommand(request: NativeMusicCommand) { return parseNativeMusicSnapshot(await invoke<unknown>("music_session_command", { request })); }
/** Browser adapters report observations; native policy owns progression and persistence. */
export async function musicSessionObserve(subscriptionId: string, observation: NativeMusicObservation): Promise<void> { await invoke("music_session_observe", { subscriptionId, observation }); }
/** Renews browser-host availability without adding durable command receipts. */
export async function musicSessionHost(subscriptionId: string, available: boolean): Promise<void> { await invoke("music_session_host", { subscriptionId, available }); }
/** Registers one stream with at most one unacknowledged frame. */
export async function musicSessionSubscribe(subscriptionId: string, channel: Channel<unknown>): Promise<void> { await invoke("music_session_subscribe", { subscriptionId, channel }); }
/** Reads a retained frame without placing large snapshots in the channel payload cache. */
export async function musicSessionReadFrame(subscriptionId: string, sequence: number): Promise<unknown> { return invoke<unknown>("music_session_read_frame", { subscriptionId, sequence }); }
/** Acknowledges delivery after browser mechanisms consume the ordered effects. */
export async function musicSessionAcknowledge(subscriptionId: string, sequence: number): Promise<void> { await invoke("music_session_acknowledge", { subscriptionId, sequence }); }
/** Removes only the current WebView's matching stream. */
export async function musicSessionUnsubscribe(subscriptionId: string): Promise<void> { await invoke("music_session_unsubscribe", { subscriptionId }); }
