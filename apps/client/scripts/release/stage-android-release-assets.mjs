#!/usr/bin/env node

import { copyFileSync, mkdirSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ANDROID_OUTPUT_ROOT = path.join(
  CLIENT_ROOT,
  "src-tauri",
  "gen",
  "android",
  "app",
  "build",
  "outputs",
);
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const APP_PACKAGE_PATH = path.join(CLIENT_ROOT, "package.json");

/** Recursively list regular files without following directory entries as links. */
function listFiles(directory) {
  if (!statSync(directory, { throwIfNoEntry: false })?.isDirectory()) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) return listFiles(entryPath);
    return entry.isFile() ? [entryPath] : [];
  });
}

/** Require one universal release artifact and reject ambiguous stale outputs. */
function requireArtifact(extension) {
  const matches = listFiles(ANDROID_OUTPUT_ROOT).filter((filePath) => {
    const normalized = filePath.split(path.sep).join("/");
    return normalized.endsWith(extension)
      && normalized.toLowerCase().includes("release")
      && path.basename(filePath).includes("universal");
  });
  if (matches.length !== 1) {
    throw new Error(
      `Expected exactly one universal Android release ${extension} artifact, found ${matches.length}`,
    );
  }
  return matches[0];
}

const packageJson = JSON.parse(readFileSync(APP_PACKAGE_PATH, "utf8"));
if (typeof packageJson.version !== "string" || !/^[0-9A-Za-z.+-]+$/u.test(packageJson.version)) {
  throw new Error("apps/client/package.json contains an invalid release version");
}

mkdirSync(ASSET_DIR, { recursive: true });
for (const [extension, targetName] of [
  [".apk", `Ganbaru_AI_${packageJson.version}_android_universal.apk`],
  [".aab", `Ganbaru_AI_${packageJson.version}_android_universal.aab`],
]) {
  const source = requireArtifact(extension);
  const target = path.join(ASSET_DIR, targetName);
  if (statSync(target, { throwIfNoEntry: false })) {
    throw new Error(`Android release asset already exists: ${targetName}`);
  }
  copyFileSync(source, target);
  console.log(`Staged ${targetName}`);
}
