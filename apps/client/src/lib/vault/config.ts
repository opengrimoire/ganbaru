/**
 * Frontend bridge to the active vault's configuration.
 *
 * Rust composes one nested view from the portable preferences in the vault
 * database, this device's preference file, and the `distractions` and `chat`
 * branches of `config.json`. This module reads that view once at boot, keeps
 * an in-memory cache, and flushes bounded key-level patches through a
 * debounced write so a burst of edits (dragging a color picker, typing in a
 * name field) coalesces into one IO. Rust routes each patch to the store that
 * owns its key and refuses keys outside the preference registry. A failed
 * write restores the saved values of its keys and tells subscribers.
 *
 * Consumers go through `getConfigKey` / `setConfigKey` with dotted keys
 * (e.g. `"theme.activeId"`, `"preferences.fontScale"`), which name nested
 * objects of the view.
 *
 * The vault path itself is owned by the Rust side (see
 * `src-tauri/app/src/vault.rs`). This module only reads cached values and
 * requests patches, and never talks to the filesystem directly.
 */

import { invoke } from "@tauri-apps/api/core";
import { listenSyncApplied } from "$lib/api/sync";
import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

type JsonObject = Record<string, unknown>;

const WRITE_DEBOUNCE_MS = 250;
/** Replicated table of the portable preferences, as sync names it in applied notices. */
export const PREFERENCES_SYNC_TABLE = "vault_preferences";

let cache: JsonObject = {};
let loadPromise: Promise<void> | null = null;
let writeTimer: ReturnType<typeof setTimeout> | null = null;
let writeInflight: Promise<void> | null = null;
interface ConfigPatch {
  path: string[];
  remove: boolean;
  value: unknown;
}
const pendingPatches = new Map<string, ConfigPatch>();
/** Keys of the write in flight, still unconfirmed on disk. */
const inflightKeys = new Set<string>();
const configChanges = createVaultChangeChannel("vault-config-changed");
const changeListeners = new Set<(keys: ReadonlySet<string>) => void>();
let windowSyncStarted = false;

function isPlainObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function splitKey(dotted: string): string[] {
  return dotted.split(".").filter((part) => part.length > 0);
}

function readPath(root: JsonObject, parts: readonly string[]): unknown {
  let current: unknown = root;
  for (const part of parts) {
    if (!isPlainObject(current)) return undefined;
    if (!Object.hasOwn(current, part)) return undefined;
    current = (current as JsonObject)[part];
  }
  return current;
}

function writePath(root: JsonObject, parts: readonly string[], value: unknown): void {
  if (parts.length === 0) return;
  let current = root;
  for (let i = 0; i < parts.length - 1; i++) {
    const part = parts[i];
    const next = current[part];
    if (!isPlainObject(next)) {
      const fresh: JsonObject = {};
      current[part] = fresh;
      current = fresh;
    } else {
      current = next;
    }
  }
  current[parts[parts.length - 1]] = value;
}

function deletePath(root: JsonObject, parts: readonly string[]): void {
  if (parts.length === 0) return;
  let current = root;
  for (let i = 0; i < parts.length - 1; i++) {
    const part = parts[i];
    const next = current[part];
    if (!isPlainObject(next)) return;
    current = next;
  }
  delete current[parts[parts.length - 1]];
}

/** Parse the config file, or return null when it is not a JSON object. */
function parseConfig(raw: string): JsonObject | null {
  try {
    const parsed: unknown = JSON.parse(raw);
    return isPlainObject(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

async function fetchFromDisk(): Promise<JsonObject> {
  const raw = await invoke<string>("vault_read_config");
  try {
    const parsed = JSON.parse(raw);
    return isPlainObject(parsed) ? parsed : {};
  } catch (err) {
    console.error("vault_read_config failed", err);
    return {};
  }
}

async function flushPendingPatches(): Promise<void> {
  if (writeInflight) await writeInflight;
  if (pendingPatches.size === 0) return;
  const patches = [...pendingPatches.values()];
  pendingPatches.clear();
  const keys = patches.map((patch) => patch.path.join("."));
  for (const key of keys) inflightKeys.add(key);
  try {
    const request = invoke<void>("vault_patch_config", { patches });
    writeInflight = request;
    await request;
    configChanges.publish(keys);
  } catch (err) {
    console.error("vault_patch_config failed", keys, err);
    await restoreFailedKeys(keys).catch((error: unknown) => {
      console.error("Config restore after a failed write failed", keys, error);
    });
  } finally {
    writeInflight = null;
    for (const key of keys) inflightKeys.delete(key);
  }
}

/**
 * Put the saved values back into the cache for keys whose write failed, unless a newer local edit is queued for
 * them, then tell subscribers which keys changed. Nothing is published to other windows, since nothing was saved.
 */
async function restoreFailedKeys(keys: readonly string[]): Promise<void> {
  const fresh = parseConfig(await invoke<string>("vault_read_config"));
  if (fresh === null) throw new Error("vault configuration is not a JSON object");
  const restored = new Set<string>();
  for (const key of keys) {
    if ([...pendingPatches.keys()].some((pending) => keysOverlap(key, pending))) continue;
    const parts = splitKey(key);
    const value = readPath(fresh, parts);
    if (value === undefined) deletePath(cache, parts);
    else writePath(cache, parts, value);
    restored.add(key);
  }
  if (restored.size === 0) return;
  for (const listener of changeListeners) listener(restored);
}

function scheduleFlush(): void {
  if (writeTimer !== null) clearTimeout(writeTimer);
  writeTimer = setTimeout(() => {
    writeTimer = null;
    void flushPendingPatches();
  }, WRITE_DEBOUNCE_MS);
}

/**
 * Trigger the config load. Idempotent: subsequent calls return the same
 * promise. Must complete before any consumer calls `getConfigKey`.
 */
export function ensureConfigLoaded(): Promise<void> {
  if (loadPromise) return loadPromise;
  loadPromise = (async () => {
    cache = await fetchFromDisk();
  })();
  return loadPromise;
}

/**
 * Read a dotted-path config value. Returns `fallback` only when the key is
 * missing. The stored value is not checked against `T`, so callers must
 * validate its shape.
 */
export function getConfigKey<T>(key: string, fallback: T): T {
  const parts = splitKey(key);
  const found = readPath(cache, parts);
  return found === undefined ? fallback : (found as T);
}

/**
 * Update a dotted-path config value and schedule a debounced disk write.
 * Pass `undefined` to remove the key.
 */
export function setConfigKey(key: string, value: unknown): void {
  const parts = splitKey(key);
  if (parts.length === 0) return;
  if (value === undefined) {
    deletePath(cache, parts);
  } else {
    writePath(cache, parts, value);
  }
  pendingPatches.set(key, {
    path: parts,
    remove: value === undefined,
    value: value ?? null,
  });
  scheduleFlush();
}

/**
 * Resolve once any pending debounced write has flushed. Use it before work
 * that native code checks against the saved config, such as deleting a
 * replaced profile image, and in shutdown handlers and tests.
 */
export async function flushConfig(): Promise<void> {
  if (writeTimer !== null) {
    clearTimeout(writeTimer);
    writeTimer = null;
  }
  await flushPendingPatches();
  if (pendingPatches.size > 0) await flushPendingPatches();
}

/** Whether two dotted keys name the same branch or one contains the other. */
function keysOverlap(left: string, right: string): boolean {
  return left === right || left.startsWith(`${right}.`) || right.startsWith(`${left}.`);
}

function hasLocalEdit(key: string): boolean {
  for (const pending of [...pendingPatches.keys(), ...inflightKeys]) {
    if (keysOverlap(key, pending)) return true;
  }
  return false;
}

/**
 * Reread the keys another window saved and copy them into this window's cache, then tell subscribers which keys
 * changed. Keys this window is still writing keep the local value, since that write lands after the other one.
 * Receiving never writes or announces anything, so windows do not echo each other.
 */
export async function refreshConfigKeysFromDisk(keys: Iterable<string>): Promise<void> {
  const fresh = parseConfig(await invoke<string>("vault_read_config"));
  if (fresh === null) throw new Error("vault configuration is not a JSON object");
  applyFreshKeys(fresh, keys);
}

/**
 * Reread the whole view after sync applied preference rows from a linked device, copy every key whose value differs
 * from the cache into it, and tell subscribers which keys changed. Keys this window is still writing keep the local
 * value, which reaches the other devices once it is saved.
 */
export async function refreshConfigFromSync(): Promise<void> {
  const fresh = parseConfig(await invoke<string>("vault_read_config"));
  if (fresh === null) throw new Error("vault configuration is not a JSON object");
  applyFreshKeys(fresh, changedConfigKeys(cache, fresh));
}

/**
 * Dotted keys whose values differ between two views. Objects present on both sides are compared member by member, so
 * a change deep in a branch names only its own key; any other value is compared whole.
 */
export function changedConfigKeys(previous: JsonObject, next: JsonObject, prefix = ""): string[] {
  const changed: string[] = [];
  for (const name of new Set([...Object.keys(previous), ...Object.keys(next)])) {
    const key = prefix ? `${prefix}.${name}` : name;
    const before = Object.hasOwn(previous, name) ? previous[name] : undefined;
    const after = Object.hasOwn(next, name) ? next[name] : undefined;
    if (isPlainObject(before) && isPlainObject(after)) changed.push(...changedConfigKeys(before, after, key));
    else if (JSON.stringify(before) !== JSON.stringify(after)) changed.push(key);
  }
  return changed;
}

/** Copy `keys` from a fresh view into the cache, except keys with local edits, and tell subscribers. */
function applyFreshKeys(fresh: JsonObject, keys: Iterable<string>): void {
  const applied = new Set<string>();
  for (const key of new Set(keys)) {
    const parts = splitKey(key);
    if (parts.length === 0 || hasLocalEdit(parts.join("."))) continue;
    const value = readPath(fresh, parts);
    if (value === undefined) deletePath(cache, parts);
    else writePath(cache, parts, value);
    applied.add(parts.join("."));
  }
  if (applied.size === 0) return;
  for (const listener of changeListeners) listener(applied);
}

/**
 * Subscribe to config keys that another window changed, after this window's cache holds the new values. Returns a
 * function that unsubscribes.
 */
export function onConfigChangedElsewhere(listener: (keys: ReadonlySet<string>) => void): () => void {
  changeListeners.add(listener);
  return () => changeListeners.delete(listener);
}

/** Whether a changed key touches the given dotted key, including its parent or child branches. */
export function configKeyChanged(keys: ReadonlySet<string>, key: string): boolean {
  for (const changed of keys) {
    if (keysOverlap(changed, key)) return true;
  }
  return false;
}

/**
 * Start receiving config changes from other windows of the vault and preference rows that sync applied. Every window
 * receives the sync notice itself, so nothing is relayed between windows. Idempotent; call once the config is loaded.
 */
export function startConfigWindowSync(): void {
  if (windowSyncStarted) return;
  windowSyncStarted = true;
  const stopWindows = configChanges.listen((keys) => {
    void refreshConfigKeysFromDisk(keys).catch((error: unknown) => {
      console.error("Config refresh from another window failed", error);
    });
  });
  const stopReplicated = listenSyncApplied((tables) => {
    if (!tables.includes(PREFERENCES_SYNC_TABLE)) return;
    void refreshConfigFromSync().catch((error: unknown) => {
      console.error("Config refresh after sync failed", error);
    });
  });
  void Promise.all([stopWindows, stopReplicated]).catch((error: unknown) => {
    windowSyncStarted = false;
    void stopWindows.then((stop) => stop(), () => undefined);
    void stopReplicated.then((stop) => stop(), () => undefined);
    console.error("Config could not listen for other windows or sync", error);
  });
}
