#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  formatChecksumManifest,
  packageRepoPublicKey,
  parseChecksumManifest,
  planManifestPrune,
  sha256File,
  verifyDetachedSignature,
} from "./release-checksums.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const PAGES_DIR = process.env.GANBARU_PACKAGE_PAGES_DIR?.trim()
  ? path.resolve(process.env.GANBARU_PACKAGE_PAGES_DIR)
  : path.join(REPO_ROOT, "dist", "pages");
const PACKAGES_DIR = path.join(PAGES_DIR, "packages");
const APT_ROOT = path.join(PACKAGES_DIR, "apt");
const RPM_ROOT = path.join(PACKAGES_DIR, "rpm");
const PUBLIC_KEY_FILE = path.join(PACKAGES_DIR, "ganbaru-package-repo.asc");
const APT_RELEASE_CONFIG = path.join(REPO_ROOT, "dist", "apt-release.conf");
const APT_POOL = path.join(APT_ROOT, "pool");
const RPM_METADATA = path.join(RPM_ROOT, "repodata");
/** Signed list of every package file the repository publishes, relative to `packages/`. */
const POOL_MANIFEST = path.join(PACKAGES_DIR, "pool-SHA256SUMS");
const POOL_SIGNATURE = `${POOL_MANIFEST}.asc`;
/**
 * Days apt accepts a signed `Release` before treating the repository as frozen. The refresh
 * workflow re-signs it monthly, so two missed refreshes still leave clients a valid `Release`.
 */
export const APT_RELEASE_VALID_DAYS = 90;
const SECONDS_PER_DAY = 24 * 60 * 60;
/** Command line flag that re-signs the existing apt metadata without adding a release. */
const REFRESH_FLAG = "--refresh";

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
 * Lists release assets with a given extension.
 *
 * @param {string} extension File extension.
 * @returns {string[]} Absolute asset paths.
 */
function releaseAssets(extension) {
  return readdirSync(ASSET_DIR)
    .filter((name) => name.endsWith(extension))
    .map((name) => path.join(ASSET_DIR, name))
    .filter((filePath) => statSync(filePath).isFile())
    .sort();
}

/**
 * Runs a command and returns captured standard output.
 *
 * @param {string} command Command name.
 * @param {string[]} args Command arguments.
 * @param {string} cwd Working directory.
 * @returns {string} Captured standard output.
 */
function commandOutput(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (result.status !== 0) {
    throw new Error(`${command} failed`);
  }
  return result.stdout;
}

/**
 * Runs a command with inherited output.
 *
 * @param {string} command Command name.
 * @param {string[]} args Command arguments.
 * @param {string} cwd Working directory.
 */
function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} failed`);
  }
}

/**
 * Signs a repository metadata file with the imported package repo key.
 *
 * @param {string[]} args Additional gpg arguments.
 */
function gpgSign(args) {
  run(
    "gpg",
    [
      "--batch",
      "--yes",
      "--pinentry-mode",
      "loopback",
      "--passphrase-file",
      readRequiredEnv("GANBARU_PACKAGE_REPO_GPG_PASSPHRASE_FILE"),
      "--local-user",
      readRequiredEnv("GANBARU_PACKAGE_REPO_GPG_KEY_ID"),
      ...args,
    ],
    REPO_ROOT,
  );
}

/**
 * Copies one release asset into a repository subdirectory.
 *
 * @param {string} assetPath Release asset path.
 * @param {string} directory Destination directory.
 */
function copyPackage(assetPath, directory) {
  mkdirSync(directory, { recursive: true });
  const destination = path.join(directory, path.basename(assetPath));
  copyFileSync(assetPath, destination);
  console.log(`Copied ${path.basename(assetPath)} into ${path.relative(PAGES_DIR, directory)}`);
}

/**
 * Hashes every file under the trees the metadata generators scan, keyed by path relative to
 * `packages/`. Entries that are not regular files get an empty digest so the prune drops them.
 *
 * @returns {Map<string, string>} Relative path to digest.
 */
function poolFiles() {
  const files = new Map();
  const walk = (directory) => {
    if (!existsSync(directory)) return;
    for (const name of readdirSync(directory).sort()) {
      const entry = path.join(directory, name);
      if (entry === RPM_METADATA) continue;
      const stats = lstatSync(entry);
      if (stats.isDirectory()) {
        walk(entry);
      } else {
        const relative = path.relative(PACKAGES_DIR, entry).split(path.sep).join("/");
        files.set(relative, stats.isFile() ? sha256File(entry) : "");
      }
    }
  };
  walk(APT_POOL);
  walk(RPM_ROOT);
  return files;
}

/**
 * Reads the pool manifest the previous run signed. A missing manifest and signature is the first
 * publication, which keeps no earlier package; a manifest without a valid signature means the
 * Pages branch was changed outside this job, so publishing stops for a maintainer to inspect it.
 *
 * @returns {Map<string, string>} Relative path to digest of every package the key vouched for.
 */
function verifiedPoolManifest() {
  const hasManifest = existsSync(POOL_MANIFEST);
  const hasSignature = existsSync(POOL_SIGNATURE);
  if (!hasManifest && !hasSignature) {
    console.log("No signed pool manifest exists yet; earlier packages are not carried forward");
    return new Map();
  }
  if (!hasManifest || !hasSignature) {
    throw new Error("The package pool manifest or its signature is missing; inspect the gh-pages branch");
  }
  verifyDetachedSignature(packageRepoPublicKey(), POOL_MANIFEST, POOL_SIGNATURE);
  return parseChecksumManifest(readFileSync(POOL_MANIFEST, "utf8"));
}

/**
 * Removes every pool file the previous signed manifest does not vouch for, so the repository key
 * never signs metadata for a package that only appeared on the Pages branch.
 */
function pruneUnverifiedPool() {
  const { keep, drop } = planManifestPrune(verifiedPoolManifest(), poolFiles());
  for (const relative of drop) {
    rmSync(path.join(PACKAGES_DIR, relative), { force: true });
    console.log(`Removed unverified pool file ${relative}`);
  }
  console.log(`Kept ${keep.length} verified pool files`);
}

/** Writes and signs the manifest of every package file the repository now publishes. */
function writePoolManifest() {
  writeFileSync(POOL_MANIFEST, formatChecksumManifest(poolFiles()), "utf8");
  rmSync(POOL_SIGNATURE, { force: true });
  gpgSign(["--detach-sign", "--armor", "-o", POOL_SIGNATURE, POOL_MANIFEST]);
}

/**
 * Builds the `apt-ftparchive` configuration for the signed `Release` file. `ValidTime` makes it
 * emit `Valid-Until`, which apt enforces so a replayed old `Release` stops being accepted.
 *
 * @param {number} validDays Days the signed `Release` stays valid.
 * @returns {string} Configuration text.
 */
export function aptReleaseConfig(validDays) {
  if (!Number.isInteger(validDays) || validDays <= 0) {
    throw new Error("The apt Release validity must be a positive number of days");
  }
  return [
    'APT::FTPArchive::Release::Origin "Ganbaru";',
    'APT::FTPArchive::Release::Label "Ganbaru";',
    'APT::FTPArchive::Release::Suite "stable";',
    'APT::FTPArchive::Release::Codename "stable";',
    'APT::FTPArchive::Release::Architectures "amd64";',
    'APT::FTPArchive::Release::Components "main";',
    `APT::FTPArchive::Release::ValidTime "${validDays * SECONDS_PER_DAY}";`,
    "",
  ].join("\n");
}

/**
 * Regenerates and signs apt repository metadata from the pool.
 */
function buildAptRepo() {
  const binaryDir = path.join(APT_ROOT, "dists", "stable", "main", "binary-amd64");
  // Release hashes every file under dists/stable, so the tree is rebuilt from the pool alone.
  rmSync(path.join(APT_ROOT, "dists"), { recursive: true, force: true });
  mkdirSync(binaryDir, { recursive: true });

  const packages = commandOutput("dpkg-scanpackages", ["--arch", "amd64", "pool"], APT_ROOT);
  writeFileSync(path.join(binaryDir, "Packages"), packages, "utf8");
  run("gzip", ["-kf", "Packages"], binaryDir);

  writeFileSync(APT_RELEASE_CONFIG, aptReleaseConfig(APT_RELEASE_VALID_DAYS), "utf8");

  const releasePath = path.join(APT_ROOT, "dists", "stable", "Release");
  const release = commandOutput(
    "apt-ftparchive",
    ["-c", APT_RELEASE_CONFIG, "release", "dists/stable"],
    APT_ROOT,
  );
  writeFileSync(releasePath, release, "utf8");

  rmSync(path.join(APT_ROOT, "dists", "stable", "Release.gpg"), { force: true });
  rmSync(path.join(APT_ROOT, "dists", "stable", "InRelease"), { force: true });
  gpgSign(["--detach-sign", "--armor", "-o", `${releasePath}.gpg`, releasePath]);
  gpgSign(["--clearsign", "-o", path.join(APT_ROOT, "dists", "stable", "InRelease"), releasePath]);
}

/**
 * Regenerates and signs RPM repository metadata.
 *
 * @param {string} rpmAsset Release `.rpm` path.
 */
function buildRpmRepo(rpmAsset) {
  copyPackage(rpmAsset, path.join(RPM_ROOT, "x86_64"));
  // Metadata is regenerated from the package headers instead of reusing the previous repodata.
  rmSync(RPM_METADATA, { recursive: true, force: true });
  run("createrepo_c", ["."], RPM_ROOT);

  const metadataPath = path.join(RPM_ROOT, "repodata", "repomd.xml");
  rmSync(`${metadataPath}.asc`, { force: true });
  gpgSign(["--detach-sign", "--armor", "-o", `${metadataPath}.asc`, metadataPath]);
}

/** Adds the release packages to the pool and regenerates and signs all repository metadata. */
function publishRelease() {
  const debAssets = releaseAssets(".deb");
  const rpmAssets = releaseAssets(".rpm");
  if (debAssets.length !== 1) {
    throw new Error(`Expected one .deb release asset, found ${debAssets.length}`);
  }
  if (rpmAssets.length !== 1) {
    throw new Error(`Expected one .rpm release asset, found ${rpmAssets.length}`);
  }

  mkdirSync(PACKAGES_DIR, { recursive: true });
  writeFileSync(path.join(PAGES_DIR, ".nojekyll"), "", "utf8");
  writeFileSync(PUBLIC_KEY_FILE, packageRepoPublicKey(), "utf8");

  pruneUnverifiedPool();
  copyPackage(debAssets[0], path.join(APT_POOL, "main", "g", "ganbaru"));
  buildAptRepo();
  buildRpmRepo(rpmAssets[0]);
  writePoolManifest();

  console.log(`Package repository updated in ${path.relative(REPO_ROOT, PACKAGES_DIR)}`);
}

/**
 * Re-signs the apt metadata of the published pool so its `Valid-Until` moves forward between
 * releases. Any pool file the signed manifest does not vouch for stops the refresh, because
 * pruning it would leave the RPM metadata pointing at a missing file.
 */
function refreshAptMetadata() {
  const manifest = verifiedPoolManifest();
  if (!manifest.size) {
    throw new Error("No signed pool manifest exists; publish a release before refreshing");
  }
  const { drop } = planManifestPrune(manifest, poolFiles());
  if (drop.length) {
    throw new Error(`Pool files are not in the signed manifest; inspect the gh-pages branch:\n  ${drop.join("\n  ")}`);
  }
  buildAptRepo();
  console.log(`Package repository apt metadata refreshed in ${path.relative(REPO_ROOT, PACKAGES_DIR)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length > 1 || (args.length === 1 && args[0] !== REFRESH_FLAG)) {
    console.error(`Usage: node build-package-repo.mjs [${REFRESH_FLAG}]`);
    process.exit(2);
  }
  if (args[0] === REFRESH_FLAG) refreshAptMetadata();
  else publishRelease();
}
