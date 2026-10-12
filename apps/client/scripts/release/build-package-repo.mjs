#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const PAGES_DIR = process.env.GANBARU_AI_PACKAGE_PAGES_DIR?.trim()
  ? path.resolve(process.env.GANBARU_AI_PACKAGE_PAGES_DIR)
  : path.join(REPO_ROOT, "dist", "pages");
const PACKAGES_DIR = path.join(PAGES_DIR, "packages");
const APT_ROOT = path.join(PACKAGES_DIR, "apt");
const RPM_ROOT = path.join(PACKAGES_DIR, "rpm");
const PUBLIC_KEY_FILE = path.join(PACKAGES_DIR, "ganbaru-ai-package-repo.asc");
const APT_RELEASE_CONFIG = path.join(REPO_ROOT, "dist", "apt-release.conf");

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
 * Reads a public key from the environment.
 *
 * @returns {string} ASCII-armored OpenPGP public key.
 */
function packageRepoPublicKey() {
  const key = readRequiredEnv("GANBARU_AI_PACKAGE_REPO_PUBLIC_KEY");
  if (!key.includes("BEGIN PGP PUBLIC KEY BLOCK")) {
    throw new Error("GANBARU_AI_PACKAGE_REPO_PUBLIC_KEY must be an ASCII-armored public key");
  }
  return key.endsWith("\n") ? key : `${key}\n`;
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
      readRequiredEnv("GANBARU_AI_PACKAGE_REPO_GPG_PASSPHRASE_FILE"),
      "--local-user",
      readRequiredEnv("GANBARU_AI_PACKAGE_REPO_GPG_KEY_ID"),
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
 * Regenerates and signs apt repository metadata.
 *
 * @param {string} debAsset Release `.deb` path.
 */
function buildAptRepo(debAsset) {
  const poolDir = path.join(APT_ROOT, "pool", "main", "g", "ganbaru-ai");
  const binaryDir = path.join(APT_ROOT, "dists", "stable", "main", "binary-amd64");
  mkdirSync(binaryDir, { recursive: true });
  copyPackage(debAsset, poolDir);

  const packages = commandOutput("dpkg-scanpackages", ["--arch", "amd64", "pool"], APT_ROOT);
  writeFileSync(path.join(binaryDir, "Packages"), packages, "utf8");
  run("gzip", ["-kf", "Packages"], binaryDir);

  writeFileSync(
    APT_RELEASE_CONFIG,
    [
      'APT::FTPArchive::Release::Origin "Ganbaru AI";',
      'APT::FTPArchive::Release::Label "Ganbaru AI";',
      'APT::FTPArchive::Release::Suite "stable";',
      'APT::FTPArchive::Release::Codename "stable";',
      'APT::FTPArchive::Release::Architectures "amd64";',
      'APT::FTPArchive::Release::Components "main";',
      "",
    ].join("\n"),
    "utf8",
  );

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
  run("createrepo_c", ["--update", "."], RPM_ROOT);

  const metadataPath = path.join(RPM_ROOT, "repodata", "repomd.xml");
  rmSync(`${metadataPath}.asc`, { force: true });
  gpgSign(["--detach-sign", "--armor", "-o", `${metadataPath}.asc`, metadataPath]);
}

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

buildAptRepo(debAssets[0]);
buildRpmRepo(rpmAssets[0]);

console.log(`Package repository updated in ${path.relative(REPO_ROOT, PACKAGES_DIR)}`);
