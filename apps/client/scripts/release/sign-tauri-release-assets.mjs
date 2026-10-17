#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { readdirSync, rmSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const UPDATE_EXTENSIONS = new Set([".AppImage", ".exe", ".msi"]);

/**
 * Lists files that can be installed by the Tauri updater.
 *
 * @returns {string[]} Absolute file paths.
 */
function updaterAssets() {
  return readdirSync(ASSET_DIR)
    .map((name) => path.join(ASSET_DIR, name))
    .filter((filePath) => statSync(filePath).isFile())
    .filter((filePath) => UPDATE_EXTENSIONS.has(path.extname(filePath)))
    .sort();
}

const signingKey = process.env.TAURI_SIGNING_PRIVATE_KEY?.trim();
if (!signingKey) {
  throw new Error("TAURI_SIGNING_PRIVATE_KEY must be set");
}

const assets = updaterAssets();
if (!assets.length) {
  throw new Error("No updater assets were found to sign");
}

for (const asset of assets) {
  const signaturePath = `${asset}.sig`;
  rmSync(signaturePath, { force: true });

  const result = spawnSync(
    "pnpm",
    ["--dir", "apps/client", "exec", "tauri", "signer", "sign", asset],
    {
      cwd: REPO_ROOT,
      env: process.env,
      stdio: "inherit",
    },
  );

  if (result.status !== 0) {
    throw new Error(`Failed to sign ${path.basename(asset)}`);
  }

  if (!statSync(signaturePath, { throwIfNoEntry: false })?.isFile()) {
    throw new Error(`Signature was not written for ${path.basename(asset)}`);
  }

  console.log(`Signed ${path.basename(asset)}`);
}
