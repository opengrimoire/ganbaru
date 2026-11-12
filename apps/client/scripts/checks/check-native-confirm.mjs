#!/usr/bin/env node

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const SOURCE_DIR = "src";

/** Native confirmation calls, which bypass the shared confirmation service. */
const NATIVE_CONFIRM_CALL = /\b(?:window|globalThis)\s*\.\s*confirm\s*\(/u;
const CHECKED_EXTENSIONS = new Set([".ts", ".js", ".mjs", ".svelte"]);
const TEST_FILE = /\.test\.[^/]+$/u;

/**
 * Whether a source path is checked: a frontend source file that is not a test.
 *
 * @param {string} relative Path relative to the client package, with `/` separators.
 * @returns {boolean} True when the file must not call the native confirmation.
 */
export function isCheckedSource(relative) {
  return CHECKED_EXTENSIONS.has(path.posix.extname(relative)) && !TEST_FILE.test(relative);
}

/**
 * Reports native `window.confirm` calls in checked source files.
 *
 * @param {{ path: string, text: string }[]} files Frontend sources with client-relative paths.
 * @returns {string[]} Violations as `path:line` descriptions.
 */
export function nativeConfirmViolations(files) {
  const violations = [];
  for (const { path: file, text } of files) {
    if (!isCheckedSource(file)) continue;
    text.split("\n").forEach((line, index) => {
      if (NATIVE_CONFIRM_CALL.test(line)) violations.push(`${file}:${index + 1} calls the native confirmation`);
    });
  }
  return violations;
}

/**
 * Lists every file under the client source directory, including untracked ones.
 *
 * @returns {string[]} Client-relative paths with `/` separators.
 */
function sourceFiles() {
  return readdirSync(path.join(CLIENT_ROOT, SOURCE_DIR), { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => path.relative(CLIENT_ROOT, path.join(entry.parentPath, entry.name)).split(path.sep).join("/"));
}

/** Runs the check against the client sources. */
function main() {
  const files = sourceFiles()
    .filter(isCheckedSource)
    .map((relative) => ({ path: relative, text: readFileSync(path.join(CLIENT_ROOT, relative), "utf8") }));
  const violations = nativeConfirmViolations(files);
  if (violations.length > 0) {
    console.error("Use requestConfirmation from $lib/components/ui/confirmation.svelte instead of window.confirm:");
    for (const violation of violations) console.error(`  ${violation}`);
    process.exit(1);
  }
  console.log(`${files.length} frontend source files use the shared confirmation service.`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
