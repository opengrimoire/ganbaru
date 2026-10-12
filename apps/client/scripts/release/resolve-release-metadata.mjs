#!/usr/bin/env node

import { appendFileSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const APP_PACKAGE_PATH = path.join(CLIENT_ROOT, "package.json");
const TAURI_CONFIG_PATH = path.join(CLIENT_ROOT, "src-tauri", "tauri.conf.json");
const CARGO_MANIFEST_PATH = path.join(CLIENT_ROOT, "src-tauri", "Cargo.toml");

/**
 * Reads JSON from disk.
 *
 * @param {string} filePath JSON file path.
 * @returns {unknown} Parsed JSON content.
 */
function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

/**
 * Extracts the package version from a Cargo manifest.
 *
 * @returns {string} Cargo package version.
 */
function readCargoVersion() {
  const manifest = readFileSync(CARGO_MANIFEST_PATH, "utf8");
  let inPackageSection = false;

  for (const line of manifest.split(/\r?\n/u)) {
    const trimmed = line.trim();
    if (trimmed === "[package]") {
      inPackageSection = true;
      continue;
    }

    if (inPackageSection && trimmed.startsWith("[")) {
      break;
    }

    if (inPackageSection) {
      const versionMatch = trimmed.match(/^version\s*=\s*"([^"]+)"$/u);
      if (versionMatch) {
        return versionMatch[1];
      }
    }
  }

  throw new Error("Could not read Cargo package version");
}

/**
 * Validates a parsed package object with a string version field.
 *
 * @param {unknown} value Parsed JSON object.
 * @param {string} label File label used in errors.
 * @returns {string} Version value.
 */
function readVersion(value, label) {
  if (
    !value ||
    typeof value !== "object" ||
    !("version" in value) ||
    typeof value.version !== "string" ||
    !value.version.trim()
  ) {
    throw new Error(`${label} must contain a string version`);
  }
  return value.version.trim();
}

/**
 * Writes a GitHub Actions output value.
 *
 * @param {string} name Output name.
 * @param {string} value Output value.
 */
function writeOutput(name, value) {
  const outputPath = process.env.GITHUB_OUTPUT;
  if (outputPath) {
    appendFileSync(outputPath, `${name}=${value}\n`, "utf8");
    return;
  }

  console.log(`${name}=${value}`);
}

const appVersion = readVersion(readJson(APP_PACKAGE_PATH), "apps/client/package.json");
const tauriVersion = readVersion(readJson(TAURI_CONFIG_PATH), "tauri.conf.json");
const cargoVersion = readCargoVersion();

if (appVersion !== tauriVersion || appVersion !== cargoVersion) {
  throw new Error(
    `Release versions must match, package ${appVersion}, Tauri ${tauriVersion}, Cargo ${cargoVersion}`,
  );
}

const tagName = `app-v${appVersion}`;
const releaseName = `Ganbaru AI v${appVersion}`;
const refType = process.env.GITHUB_REF_TYPE;
const refName = process.env.GITHUB_REF_NAME;

if (refType !== "tag") {
  throw new Error("Release builds must run from an explicit app-v* tag");
}

if (refName !== tagName) {
  throw new Error(`Tag ${refName ?? ""} does not match app version ${tagName}`);
}

writeOutput("version", appVersion);
writeOutput("tag_name", tagName);
writeOutput("release_name", releaseName);

console.log(`Resolved ${releaseName} from ${path.relative(REPO_ROOT, APP_PACKAGE_PATH)}`);
