import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

const CLI_PACKAGE = "@tauri-apps/cli";
const REGISTRY_ORIGIN = "https://registry.npmjs.org";
/** Largest package tarball the signer setup downloads. */
const MAX_TARBALL_BYTES = 64 * 1024 * 1024;

/** Native Tauri CLI package for each signing runner platform. */
const NATIVE_CLI_PACKAGES = new Map([["linux-x64", "@tauri-apps/cli-linux-x64-gnu"]]);

/**
 * Reads the version and integrity pnpm locked for one package.
 *
 * @param {string} lockText Contents of `pnpm-lock.yaml`.
 * @param {string} name Package name.
 * @returns {{ version: string, integrity: string }} Locked version and SRI hash.
 */
export function lockedPackage(lockText, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\/]/gu, "\\$&");
  const pattern = new RegExp(
    `^  '${escaped}@([0-9A-Za-z.+-]+)':\\n    resolution: \\{integrity: (sha512-[A-Za-z0-9+/]+={0,2})\\}`,
    "gmu",
  );
  const matches = [...lockText.matchAll(pattern)];
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one locked ${name} package, found ${matches.length}`);
  }
  return { version: matches[0][1], integrity: matches[0][2] };
}

/**
 * Builds the registry tarball URL for a package version.
 *
 * @param {string} name Package name, scoped or not.
 * @param {string} version Exact version.
 * @returns {string} Tarball URL.
 */
export function tarballUrl(name, version) {
  const baseName = name.split("/").at(-1);
  return `${REGISTRY_ORIGIN}/${name}/-/${baseName}-${version}.tgz`;
}

/** Downloads one bounded tarball. */
async function download(url) {
  const response = await fetch(url, { redirect: "error" });
  if (!response.ok) throw new Error(`Download failed with HTTP ${response.status}: ${url}`);
  const declared = Number(response.headers.get("content-length") ?? "0");
  if (declared > MAX_TARBALL_BYTES) throw new Error(`Tarball exceeds ${MAX_TARBALL_BYTES} bytes: ${url}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length > MAX_TARBALL_BYTES) throw new Error(`Tarball exceeds ${MAX_TARBALL_BYTES} bytes: ${url}`);
  return bytes;
}

/**
 * Downloads one locked package tarball, checks it against the lockfile integrity, and unpacks it.
 * No package manager or package script runs.
 */
async function unpackLockedPackage(lockText, name, downloadDirectory, nodeModules) {
  const locked = lockedPackage(lockText, name);
  const bytes = await download(tarballUrl(name, locked.version));
  const digest = `sha512-${createHash("sha512").update(bytes).digest("base64")}`;
  if (digest !== locked.integrity) {
    throw new Error(`${name}@${locked.version} does not match the pnpm-lock.yaml integrity`);
  }
  const tarball = path.join(downloadDirectory, `${name.replace("/", "__")}.tgz`);
  writeFileSync(tarball, bytes, { mode: 0o600 });
  const target = path.join(nodeModules, ...name.split("/"));
  mkdirSync(target, { recursive: true });
  const result = spawnSync(
    "tar",
    ["-xzf", tarball, "-C", target, "--strip-components=1", "--no-same-owner", "--no-same-permissions"],
    { stdio: "inherit" },
  );
  if (result.status !== 0) throw new Error(`Could not unpack ${name}@${locked.version}`);
}

/**
 * Prepares the Tauri CLI version locked by the workspace without installing workspace dependencies.
 *
 * @param {string} repoRoot Repository root holding `pnpm-lock.yaml`.
 * @param {string} workDirectory Scratch directory outside the checkout; it is replaced.
 * @returns {Promise<string>} Path of the CLI entry script to run with Node.
 */
export async function preparePinnedTauriSigner(repoRoot, workDirectory) {
  const nativePackage = NATIVE_CLI_PACKAGES.get(`${process.platform}-${process.arch}`);
  if (!nativePackage) {
    throw new Error(`Updater signing is not supported on ${process.platform}-${process.arch}`);
  }
  const lockText = readFileSync(path.join(repoRoot, "pnpm-lock.yaml"), "utf8");
  rmSync(workDirectory, { recursive: true, force: true });
  const downloadDirectory = path.join(workDirectory, "downloads");
  const nodeModules = path.join(workDirectory, "node_modules");
  mkdirSync(downloadDirectory, { recursive: true });
  for (const name of [CLI_PACKAGE, nativePackage]) {
    await unpackLockedPackage(lockText, name, downloadDirectory, nodeModules);
  }
  return path.join(nodeModules, ...CLI_PACKAGE.split("/"), "tauri.js");
}
