/**
 * Frontend bridge to the active vault's root `config.json`.
 *
 * Reads the file once at boot, keeps an in-memory cache, and flushes bounded
 * key-level patches through a debounced write so a burst of edits (dragging a
 * color picker, typing in a name field) coalesces into one IO. Rust applies
 * those patches to the latest file under the same lock used by native Chat
 * settings, so neither side can replace the other's newer branches.
 *
 * Consumers go through `getConfigKey` / `setConfigKey` with dotted keys
 * (e.g. `"theme.activeId"`, `"preferences.fontScale"`). Dotted keys map to
 * nested objects on disk so the JSON stays human-readable and orthogonal
 * concerns live under their own branches.
 *
 * The vault path itself is owned by the Rust side (see
 * `src-tauri/app/src/vault.rs`). This module only reads cached values and
 * requests patches, and never talks to the filesystem directly.
 */

import { invoke } from "@tauri-apps/api/core";

type JsonObject = Record<string, unknown>;

const WRITE_DEBOUNCE_MS = 250;

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
  try {
    const request = invoke<void>("vault_patch_config", { patches });
    writeInflight = request;
    await request;
  } catch (err) {
    for (const patch of patches) {
      const key = patch.path.join(".");
      if (!pendingPatches.has(key)) pendingPatches.set(key, patch);
    }
    console.error("vault_patch_config failed", err);
  } finally {
    writeInflight = null;
  }
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
 * Resolve once any pending debounced write has flushed. Useful in tests
 * and in shutdown handlers; product code can rely on the debounced write.
 */
export async function flushConfig(): Promise<void> {
  if (writeTimer !== null) {
    clearTimeout(writeTimer);
    writeTimer = null;
  }
  await flushPendingPatches();
  if (pendingPatches.size > 0) await flushPendingPatches();
}
