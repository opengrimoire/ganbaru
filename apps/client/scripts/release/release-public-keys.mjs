import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));

/** Directory of the committed release public keys, so a key change is a reviewed source change. */
export const RELEASE_KEYS_DIR = path.resolve(SCRIPT_DIR, "..", "..", "src-tauri", "release-keys");
/** Tauri updater public key, in the base64 form the updater configuration expects. */
export const UPDATER_PUBLIC_KEY_FILE = path.join(RELEASE_KEYS_DIR, "tauri-updater.pub");
/** ASCII-armored package repository public key. */
export const PACKAGE_REPO_PUBLIC_KEY_FILE = path.join(RELEASE_KEYS_DIR, "ganbaru-package-repo.asc");
/** Optional repository variable that must match the committed updater key when set. */
export const UPDATER_PUBLIC_KEY_ENV = "TAURI_UPDATER_PUBLIC_KEY";
/** Optional repository variable that must match the committed package repository key when set. */
export const PACKAGE_REPO_PUBLIC_KEY_ENV = "GANBARU_PACKAGE_REPO_PUBLIC_KEY";

const MINISIGN_PUBLIC_KEY_COMMENT = "untrusted comment: minisign public key";
const PGP_PUBLIC_KEY_BEGIN = "-----BEGIN PGP PUBLIC KEY BLOCK-----";
const PGP_PUBLIC_KEY_END = "-----END PGP PUBLIC KEY BLOCK-----";

/**
 * Normalizes key text for comparison, ignoring line ending style and surrounding whitespace.
 *
 * @param {string} text Key text.
 * @returns {string} Normalized key text.
 */
function normalizeKey(text) {
  return text.replace(/\r\n/gu, "\n").trim();
}

/**
 * Reads a committed key file.
 *
 * @param {string} filePath Key file.
 * @returns {string} Normalized key text.
 */
function readCommittedKey(filePath) {
  try {
    return normalizeKey(readFileSync(filePath, "utf8"));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`read committed release key ${path.basename(filePath)}: ${message}`);
  }
}

/**
 * Rejects a configured key copy that differs from the committed key. An unset copy is accepted.
 *
 * @param {string} committed Normalized committed key.
 * @param {string | undefined} configured Configured copy, usually a repository variable.
 * @param {string} envName Variable name for the error message.
 */
export function ensureMatchingKey(committed, configured, envName) {
  if (configured === undefined || !configured.trim()) return;
  if (normalizeKey(configured) !== committed) {
    throw new Error(
      `${envName} differs from the committed key in src-tauri/release-keys; change both through review`,
    );
  }
}

/**
 * Reads the Tauri updater public key that installed apps use to verify updates.
 *
 * @param {NodeJS.ProcessEnv} [env] Environment that may carry a copy to compare.
 * @param {string} [filePath] Committed key file.
 * @returns {string} Base64 updater public key.
 */
export function updaterPublicKey(env = process.env, filePath = UPDATER_PUBLIC_KEY_FILE) {
  const key = readCommittedKey(filePath);
  const decoded = /^[A-Za-z0-9+/]+={0,2}$/u.test(key) ? Buffer.from(key, "base64").toString("utf8") : "";
  const [comment, encodedKey] = decoded.split("\n");
  if (
    !comment?.startsWith(MINISIGN_PUBLIC_KEY_COMMENT) ||
    Buffer.from(encodedKey ?? "", "base64").length !== 42
  ) {
    throw new Error(`${path.basename(filePath)} must be a base64 minisign public key`);
  }
  ensureMatchingKey(key, env[UPDATER_PUBLIC_KEY_ENV], UPDATER_PUBLIC_KEY_ENV);
  return key;
}

/**
 * Reads the package repository public key bundled into Linux packages and used to verify release manifests.
 *
 * @param {NodeJS.ProcessEnv} [env] Environment that may carry a copy to compare.
 * @param {string} [filePath] Committed key file.
 * @returns {string} ASCII-armored public key ending in a newline.
 */
export function packageRepoPublicKey(env = process.env, filePath = PACKAGE_REPO_PUBLIC_KEY_FILE) {
  const key = readCommittedKey(filePath);
  if (!key.startsWith(PGP_PUBLIC_KEY_BEGIN) || !key.endsWith(PGP_PUBLIC_KEY_END)) {
    throw new Error(`${path.basename(filePath)} must be an ASCII-armored public key`);
  }
  ensureMatchingKey(key, env[PACKAGE_REPO_PUBLIC_KEY_ENV], PACKAGE_REPO_PUBLIC_KEY_ENV);
  return `${key}\n`;
}
