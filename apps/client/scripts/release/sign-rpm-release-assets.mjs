#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { packageRepoPublicKey } from "./release-checksums.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");

/**
 * @typedef {object} RpmSigningKey
 * @property {string} keyId Fingerprint of the imported package repository secret key.
 * @property {string} gnupgHome GnuPG home that holds the imported key.
 * @property {string} passphraseFile File that holds the key passphrase.
 */

/**
 * Builds the `rpmsign` arguments that add a header signature by the package repository key
 * without a pinentry prompt.
 *
 * @param {RpmSigningKey} key Imported signing key.
 * @param {string} rpmPath Package to sign in place.
 * @returns {string[]} Arguments for `rpmsign`.
 */
export function rpmSignArguments(key, rpmPath) {
  return [
    "--addsign",
    "--define",
    `_gpg_name ${key.keyId}`,
    "--define",
    `_gpg_path ${key.gnupgHome}`,
    "--define",
    `_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-file ${key.passphraseFile}`,
    rpmPath,
  ];
}

/**
 * Reports whether `rpmkeys --checksig` output confirms a valid signature. Unsigned packages print
 * only `digests OK`, so the word `signatures` must be present and reported OK.
 *
 * @param {string} output Standard output of `rpmkeys --checksig <rpmPath>`.
 * @param {string} rpmPath Checked package path as passed to `rpmkeys`.
 * @returns {boolean} Whether the package carries a signature the keyring accepts.
 */
export function checksigConfirmsSignature(output, rpmPath) {
  return output.trim() === `${rpmPath}: digests signatures OK`;
}

/**
 * Runs a command and returns its standard output.
 *
 * @param {string} command Command name.
 * @param {string[]} args Arguments.
 * @returns {string} Captured standard output.
 */
function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
  if (result.error) throw new Error(`Could not start ${command}: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} failed`);
  return result.stdout;
}

/**
 * Reads a required environment variable without exposing its value.
 *
 * @param {string} name Environment variable name.
 * @returns {string} Trimmed value.
 */
function readRequiredEnv(name) {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} must be set`);
  return value;
}

/**
 * Lists the release RPMs.
 *
 * @returns {string[]} Absolute package paths.
 */
function rpmAssets() {
  return readdirSync(ASSET_DIR)
    .filter((name) => name.endsWith(".rpm"))
    .map((name) => path.join(ASSET_DIR, name))
    .filter((filePath) => statSync(filePath).isFile())
    .sort();
}

/**
 * Signs every release RPM, then checks each signature with an rpm keyring that trusts only the
 * published package repository public key, so a key mismatch fails the release instead of
 * shipping packages that `gpgcheck=1` would refuse.
 */
function signRpmAssets() {
  const key = {
    keyId: readRequiredEnv("GANBARU_PACKAGE_REPO_GPG_KEY_ID"),
    gnupgHome: readRequiredEnv("GNUPGHOME"),
    passphraseFile: readRequiredEnv("GANBARU_PACKAGE_REPO_GPG_PASSPHRASE_FILE"),
  };
  const assets = rpmAssets();
  if (!assets.length) throw new Error("No RPM release assets were found to sign");

  const verifyHome = mkdtempSync(path.join(tmpdir(), "ganbaru-rpm-verify-"));
  try {
    const keyFile = path.join(verifyHome, "package-repo.asc");
    const database = path.join(verifyHome, "rpmdb");
    writeFileSync(keyFile, packageRepoPublicKey(), "utf8");
    mkdirSync(database);
    run("rpmdb", ["--dbpath", database, "--initdb"]);
    run("rpmkeys", ["--dbpath", database, "--import", keyFile]);

    for (const asset of assets) {
      run("rpmsign", rpmSignArguments(key, asset));
      const output = run("rpmkeys", ["--dbpath", database, "--checksig", asset]);
      if (!checksigConfirmsSignature(output, asset)) {
        throw new Error(`${path.basename(asset)} is not signed by the package repository key`);
      }
      console.log(`Signed ${path.basename(asset)}`);
    }
  } finally {
    rmSync(verifyHome, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    signRpmAssets();
  } catch (caught) {
    console.error(caught instanceof Error ? caught.message : String(caught));
    process.exit(1);
  }
}
