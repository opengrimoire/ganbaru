#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..", "..", "..", "..");
const INVENTORY_DOC = "docs/data/security/unsafe-rust.md";

/** Lint attributes that would permit unsafe code below the workspace deny. */
const UNSAFE_LINT_RELAXATION = /\b(?:allow|expect|warn)\s*\(\s*(?:[\w:]+\s*,\s*)*unsafe_code\b/u;
const OWNED_MODULE_LINE = /^\*\*Owned modules?:\*\*/u;
const BACKTICKED = /`([^`]+)`/gu;

/**
 * Reads the owned module paths from the unsafe inventory. A path ending in `/` owns its directory.
 *
 * @param {string} markdown Inventory document.
 * @returns {string[]} Repository-relative files and directory prefixes.
 */
export function inventoryPaths(markdown) {
  const paths = [];
  for (const line of markdown.split("\n")) {
    if (!OWNED_MODULE_LINE.test(line)) continue;
    for (const [, value] of line.matchAll(BACKTICKED)) {
      if (value.includes("/") && (value.endsWith(".rs") || value.endsWith("/"))) paths.push(value);
    }
  }
  return paths;
}

/**
 * Reports unsafe lint relaxations in Rust files the inventory does not own.
 *
 * @param {{ path: string, text: string }[]} files Tracked Rust sources.
 * @param {string[]} owned Inventory paths.
 * @returns {string[]} Violations as `path:line` descriptions.
 */
export function unownedUnsafeAllowViolations(files, owned) {
  const isOwned = (file) => owned.some((entry) => (entry.endsWith("/") ? file.startsWith(entry) : file === entry));
  const violations = [];
  for (const { path: file, text } of files) {
    if (isOwned(file)) continue;
    text.split("\n").forEach((line, index) => {
      if (UNSAFE_LINT_RELAXATION.test(line)) violations.push(`${file}:${index + 1} relaxes unsafe_code outside the inventory`);
    });
  }
  return violations;
}

/**
 * Reports manifests that do not inherit the workspace `unsafe_code = "deny"` lint.
 *
 * @param {string} workspaceManifest Root `Cargo.toml`.
 * @param {{ path: string, text: string }[]} memberManifests Member `Cargo.toml` files.
 * @returns {string[]} Violations.
 */
export function workspaceLintViolations(workspaceManifest, memberManifests) {
  const violations = [];
  const rustLints = workspaceManifest.match(/^\[workspace\.lints\.rust\]\n((?:(?!\[).*\n?)*)/mu)?.[1] ?? "";
  if (!/^unsafe_code\s*=\s*"deny"\s*$/mu.test(rustLints)) {
    violations.push('Cargo.toml must set unsafe_code = "deny" under [workspace.lints.rust]');
  }
  for (const { path: manifest, text } of memberManifests) {
    const lints = text.match(/^\[lints\]\n((?:(?!\[).*\n?)*)/mu)?.[1];
    if (lints === undefined || !/^workspace\s*=\s*true\s*$/mu.test(lints) || /^\[lints\./mu.test(text)) {
      violations.push(`${manifest} must inherit workspace lints with only [lints] workspace = true`);
    }
  }
  return violations;
}

/**
 * Reports inventory paths that no longer exist, so the inventory cannot silently own a moved module.
 *
 * @param {string[]} owned Inventory paths.
 * @param {(relative: string) => boolean} exists Repository path probe.
 * @returns {string[]} Violations.
 */
export function staleInventoryViolations(owned, exists) {
  return owned.filter((entry) => !exists(entry)).map((entry) => `${INVENTORY_DOC} names missing path ${entry}`);
}

/**
 * Lists tracked files matching the given pathspecs.
 *
 * @param {string[]} pathspecs Git pathspecs.
 * @returns {string[]} Repository-relative paths.
 */
function trackedFiles(pathspecs) {
  const result = spawnSync("git", ["ls-files", "-z", "--", ...pathspecs], { cwd: REPO_ROOT, encoding: "utf8" });
  if (result.status !== 0) throw new Error(`git ls-files failed: ${result.stderr || result.error?.message}`);
  return result.stdout.split("\0").filter(Boolean);
}

/** Runs the check against the repository. */
function main() {
  const read = (relative) => ({ path: relative, text: readFileSync(path.join(REPO_ROOT, relative), "utf8") });
  const owned = inventoryPaths(read(INVENTORY_DOC).text);
  const sources = trackedFiles(["*.rs"]).map(read);
  const manifests = trackedFiles(["Cargo.toml", "*/Cargo.toml"]);
  const violations = [
    ...unownedUnsafeAllowViolations(sources, owned),
    ...workspaceLintViolations(
      read("Cargo.toml").text,
      manifests.filter((manifest) => manifest !== "Cargo.toml").map(read),
    ),
    ...staleInventoryViolations(owned, (relative) => existsSync(path.join(REPO_ROOT, relative))),
  ];
  if (violations.length > 0) {
    console.error(`Unsafe Rust must stay denied outside the modules owned in ${INVENTORY_DOC}:`);
    for (const violation of violations) console.error(`  ${violation}`);
    process.exit(1);
  }
  console.log(`${sources.length} Rust files deny unsafe code outside ${owned.length} inventory entries.`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
