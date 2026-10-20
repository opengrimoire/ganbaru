#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Checksum manifest published with every release and signed inside the signing job. */
export const CHECKSUM_MANIFEST = "SHA256SUMS";
/** Detached ASCII-armored signature of the checksum manifest. */
export const CHECKSUM_SIGNATURE = `${CHECKSUM_MANIFEST}.asc`;
/** Environment variable holding the package repository public key that verifies release manifests. */
export const PUBLIC_KEY_ENV = "GANBARU_PACKAGE_REPO_PUBLIC_KEY";

const SHA256_HEX = /^[0-9a-f]{64}$/u;

/**
 * Hashes one file with SHA-256.
 *
 * @param {string} filePath File path.
 * @returns {string} Lowercase hex digest.
 */
export function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

/**
 * Formats entries in `sha256sum` text mode, sorted by name.
 *
 * @param {Map<string, string>} entries Relative path to hex digest.
 * @returns {string} Manifest text.
 */
export function formatChecksumManifest(entries) {
  return [...entries]
    .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
    .map(([name, digest]) => `${digest}  ${name}\n`)
    .join("");
}

/**
 * Parses `sha256sum` output. Duplicate names, unsafe paths, and malformed lines are rejected so a
 * manifest can never vouch for two different contents under one name.
 *
 * @param {string} text Manifest text.
 * @returns {Map<string, string>} Relative path to hex digest.
 */
export function parseChecksumManifest(text) {
  const entries = new Map();
  for (const line of text.split("\n")) {
    if (!line) continue;
    const match = /^([0-9a-f]{64}) [ *](.+)$/u.exec(line);
    if (!match) throw new Error(`Malformed checksum line: ${line}`);
    const name = match[2].startsWith("./") ? match[2].slice(2) : match[2];
    if (path.posix.isAbsolute(name) || name.split("/").includes("..") || name.includes("\\")) {
      throw new Error(`Checksum entry escapes its directory: ${name}`);
    }
    if (entries.has(name)) throw new Error(`Duplicate checksum entry: ${name}`);
    entries.set(name, match[1]);
  }
  return entries;
}

/**
 * Lists every file the manifest does not vouch for with the same digest.
 *
 * @param {Map<string, string>} manifest Signed relative path to digest.
 * @param {Map<string, string>} observed Present relative path to digest.
 * @returns {string[]} Human readable violations, empty when every present file is vouched for.
 */
export function checksumViolations(manifest, observed) {
  const violations = [];
  for (const [name, digest] of observed) {
    if (!SHA256_HEX.test(digest)) {
      violations.push(`${name}: invalid digest`);
    } else if (!manifest.has(name)) {
      violations.push(`${name}: not listed in the signed checksum manifest`);
    } else if (manifest.get(name) !== digest) {
      violations.push(`${name}: digest differs from the signed checksum manifest`);
    }
  }
  return violations.sort();
}

/**
 * Splits present files into those a signed manifest vouches for with the same digest and the rest.
 *
 * @param {Map<string, string>} manifest Signed relative path to digest.
 * @param {Map<string, string>} observed Present relative path to digest.
 * @returns {{ keep: string[], drop: string[] }} Sorted relative paths.
 */
export function planManifestPrune(manifest, observed) {
  const keep = [];
  const drop = [];
  for (const [name, digest] of observed) {
    (SHA256_HEX.test(digest) && manifest.get(name) === digest ? keep : drop).push(name);
  }
  return { keep: keep.sort(), drop: drop.sort() };
}

/**
 * Hashes the regular files directly inside a directory. Links and nested directories are errors
 * because a published release directory never contains them.
 *
 * @param {string} directory Directory path.
 * @param {Set<string>} excluded File names to skip.
 * @returns {Map<string, string>} File name to digest.
 */
export function hashDirectoryFiles(directory, excluded) {
  const entries = new Map();
  for (const name of readdirSync(directory).sort()) {
    if (excluded.has(name)) continue;
    const filePath = path.join(directory, name);
    if (!lstatSync(filePath).isFile()) throw new Error(`${name} is not a regular file`);
    entries.set(name, sha256File(filePath));
  }
  return entries;
}

/**
 * Runs a command and fails with its name when it exits unsuccessfully.
 *
 * @param {string} command Command name.
 * @param {string[]} args Arguments.
 * @param {{ input?: string }} [options] Standard input.
 */
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    input: options.input,
    stdio: [options.input === undefined ? "ignore" : "pipe", "inherit", "inherit"],
  });
  if (result.error) throw new Error(`Could not start ${command}: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} failed`);
}

/**
 * Verifies a detached OpenPGP signature against exactly one armored public key. `gpgv` reads a
 * private keyring built from that key, so no other key on the runner can satisfy the check.
 *
 * @param {string} armoredPublicKey ASCII-armored public key.
 * @param {string} dataPath Signed file.
 * @param {string} signaturePath Detached signature.
 */
export function verifyDetachedSignature(armoredPublicKey, dataPath, signaturePath) {
  if (!armoredPublicKey.includes("BEGIN PGP PUBLIC KEY BLOCK")) {
    throw new Error(`${PUBLIC_KEY_ENV} must be an ASCII-armored public key`);
  }
  const home = mkdtempSync(path.join(tmpdir(), "ganbaru-verify-"));
  try {
    const keyring = path.join(home, "trusted.gpg");
    run("gpg", ["--batch", "--quiet", "--homedir", home, "--dearmor", "--output", keyring], {
      input: armoredPublicKey,
    });
    try {
      run("gpgv", ["--homedir", home, "--keyring", keyring, signaturePath, dataPath]);
    } catch {
      throw new Error(`${path.basename(signaturePath)} is not a valid signature by the package repository key`);
    }
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
}

/**
 * Reads the public key that verifies release and repository manifests.
 *
 * @returns {string} ASCII-armored public key.
 */
export function packageRepoPublicKey() {
  const key = process.env[PUBLIC_KEY_ENV]?.trim();
  if (!key) throw new Error(`${PUBLIC_KEY_ENV} must be set`);
  if (!key.includes("BEGIN PGP PUBLIC KEY BLOCK")) {
    throw new Error(`${PUBLIC_KEY_ENV} must be an ASCII-armored public key`);
  }
  return `${key}\n`;
}

/**
 * Writes the checksum manifest over every other file in a release asset directory.
 *
 * @param {string} directory Release asset directory.
 */
function writeManifest(directory) {
  const entries = hashDirectoryFiles(directory, new Set([CHECKSUM_MANIFEST, CHECKSUM_SIGNATURE]));
  if (!entries.size) throw new Error(`No release assets were found in ${directory}`);
  writeFileSync(path.join(directory, CHECKSUM_MANIFEST), formatChecksumManifest(entries), "utf8");
  console.log(`Wrote ${CHECKSUM_MANIFEST} for ${entries.size} release assets`);
}

/**
 * Verifies the signed manifest, then requires every downloaded asset to match it.
 *
 * @param {string} directory Directory holding downloaded assets, the manifest, and its signature.
 */
function verifyManifest(directory) {
  const manifestPath = path.join(directory, CHECKSUM_MANIFEST);
  const signaturePath = path.join(directory, CHECKSUM_SIGNATURE);
  verifyDetachedSignature(packageRepoPublicKey(), manifestPath, signaturePath);
  const observed = hashDirectoryFiles(directory, new Set([CHECKSUM_MANIFEST, CHECKSUM_SIGNATURE]));
  if (!observed.size) throw new Error(`No release assets were downloaded into ${directory}`);
  const violations = checksumViolations(parseChecksumManifest(readFileSync(manifestPath, "utf8")), observed);
  if (violations.length) {
    throw new Error(
      `Release assets do not match the signed ${CHECKSUM_MANIFEST}; the draft release was changed after signing. Rerun the tag workflow to rebuild it.\n  ${violations.join("\n  ")}`,
    );
  }
  console.log(`Verified ${observed.size} release assets against the signed ${CHECKSUM_MANIFEST}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, directory] = process.argv.slice(2);
  if (!directory || (command !== "write" && command !== "verify")) {
    console.error("Usage: node release-checksums.mjs <write | verify> <directory>");
    process.exit(2);
  }
  try {
    if (command === "write") writeManifest(path.resolve(directory));
    else verifyManifest(path.resolve(directory));
  } catch (caught) {
    console.error(caught instanceof Error ? caught.message : String(caught));
    process.exit(1);
  }
}
