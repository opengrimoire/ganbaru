#!/usr/bin/env node

import { copyFileSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const BUNDLE_ROOT = path.join(REPO_ROOT, "target", "release", "bundle");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const BUNDLE_OUTPUTS = [
  { directory: "appimage", extension: ".AppImage" },
  { directory: "deb", extension: ".deb" },
  { directory: "rpm", extension: ".rpm" },
  { directory: "nsis", extension: ".exe" },
  { directory: "msi", extension: ".msi" },
];

/**
 * Lists release assets from known Tauri bundle output directories.
 *
 * @returns {string[]} Absolute file paths.
 */
function listBundleAssets() {
  return BUNDLE_OUTPUTS.flatMap(({ directory, extension }) => {
    const outputDir = path.join(BUNDLE_ROOT, directory);
    if (!statSync(outputDir, { throwIfNoEntry: false })?.isDirectory()) {
      return [];
    }

    return readdirSync(outputDir)
      .map((entry) => path.join(outputDir, entry))
      .filter((entryPath) => statSync(entryPath).isFile())
      .filter((entryPath) => path.extname(entryPath) === extension);
  });
}

if (!statSync(BUNDLE_ROOT, { throwIfNoEntry: false })?.isDirectory()) {
  throw new Error(`Bundle output does not exist at ${BUNDLE_ROOT}`);
}

rmSync(ASSET_DIR, { recursive: true, force: true });
mkdirSync(ASSET_DIR, { recursive: true });

const assets = listBundleAssets();
const copiedNames = new Set();

for (const asset of assets) {
  const name = path.basename(asset);
  if (copiedNames.has(name)) {
    throw new Error(`Duplicate release asset name ${name}`);
  }
  copiedNames.add(name);
  copyFileSync(asset, path.join(ASSET_DIR, name));
}

const runnerOs = process.env.RUNNER_OS;
const names = [...copiedNames].sort();

if (runnerOs === "Linux" && !names.some((name) => name.endsWith(".AppImage"))) {
  throw new Error("Linux release assets must include an AppImage");
}

if (runnerOs === "Windows" && !names.some((name) => name.endsWith(".exe") || name.endsWith(".msi"))) {
  throw new Error("Windows release assets must include an installer");
}

if (!names.length) {
  throw new Error("No release assets were staged");
}

for (const name of names) {
  console.log(`Staged ${name}`);
}
