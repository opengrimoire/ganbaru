#!/usr/bin/env node

import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const APP_PACKAGE_PATH = path.join(CLIENT_ROOT, "package.json");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const GITHUB_REPOSITORY_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u;
const DEFAULT_RELEASE_NOTES = "See the release notes and attached assets on GitHub.";

/**
 * Reads a required environment variable without exposing its value.
 *
 * @param {string} name Environment variable name.
 * @returns {string} Trimmed environment variable value.
 */
function readRequiredEnv(name) {
  const value = process.env[name]?.trim();
  if (!value) {
    throw new Error(`${name} must be set`);
  }
  return value;
}

/**
 * Reads the app package version.
 *
 * @returns {string} App version.
 */
function appVersion() {
  const packageJson = JSON.parse(readFileSync(APP_PACKAGE_PATH, "utf8"));
  if (typeof packageJson.version !== "string" || !packageJson.version.trim()) {
    throw new Error("apps/client/package.json must contain a string version");
  }
  return packageJson.version.trim();
}

/**
 * Creates a GitHub Releases asset URL.
 *
 * @param {string} repository Repository in owner/name form.
 * @param {string} tag Release tag.
 * @param {string} assetName Asset file name.
 * @returns {string} Download URL.
 */
function releaseAssetUrl(repository, tag, assetName) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(assetName)}`;
}

/**
 * Reads a signature beside an updater asset.
 *
 * @param {string} assetName Asset file name.
 * @returns {string} Signature content.
 */
function readSignature(assetName) {
  return readFileSync(path.join(ASSET_DIR, `${assetName}.sig`), "utf8").trim();
}

/**
 * Reads generated release notes from the workflow, falling back to the default notes.
 *
 * @returns {string} Updater release notes.
 */
function releaseNotes() {
  const notesPath = process.env.GANBARU_AI_RELEASE_NOTES_PATH?.trim();
  if (!notesPath) return DEFAULT_RELEASE_NOTES;

  const notes = readFileSync(notesPath, "utf8").trim();
  return notes || DEFAULT_RELEASE_NOTES;
}

/**
 * Compares Windows updater assets so `.exe` installers sort before other types.
 *
 * @param {string} left First file name.
 * @param {string} right Second file name.
 * @returns {number} Sort result.
 */
function compareWindowsAsset(left, right) {
  const leftScore = left.endsWith(".exe") ? 0 : 1;
  const rightScore = right.endsWith(".exe") ? 0 : 1;
  if (leftScore !== rightScore) {
    return leftScore - rightScore;
  }
  return left.localeCompare(right);
}

const repository = readRequiredEnv("GANBARU_AI_RELEASE_REPOSITORY");
if (!GITHUB_REPOSITORY_PATTERN.test(repository)) {
  throw new Error("GANBARU_AI_RELEASE_REPOSITORY must be an owner/name repository");
}

const tag = readRequiredEnv("GANBARU_AI_RELEASE_TAG");
const files = readdirSync(ASSET_DIR).sort();
const signedAssets = files.filter((name) => files.includes(`${name}.sig`));
const linuxAsset = signedAssets.find((name) => name.endsWith(".AppImage"));
const windowsAsset = signedAssets
  .filter((name) => name.endsWith(".exe") || name.endsWith(".msi"))
  .sort(compareWindowsAsset)[0];

if (!linuxAsset) {
  throw new Error("latest.json requires a signed Linux AppImage");
}

if (!windowsAsset) {
  throw new Error("latest.json requires a signed Windows installer");
}

const latest = {
  version: appVersion(),
  notes: releaseNotes(),
  pub_date: new Date().toISOString(),
  platforms: {
    "linux-x86_64": {
      signature: readSignature(linuxAsset),
      url: releaseAssetUrl(repository, tag, linuxAsset),
    },
    "windows-x86_64": {
      signature: readSignature(windowsAsset),
      url: releaseAssetUrl(repository, tag, windowsAsset),
    },
  },
};

writeFileSync(path.join(ASSET_DIR, "latest.json"), `${JSON.stringify(latest, null, 2)}\n`, "utf8");
console.log(`Wrote latest.json for ${tag}`);
