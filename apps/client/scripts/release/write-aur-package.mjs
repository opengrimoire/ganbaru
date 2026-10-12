#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const AUR_DIR = process.env.GANBARU_AI_AUR_DIR?.trim()
  ? path.resolve(process.env.GANBARU_AI_AUR_DIR)
  : path.join(REPO_ROOT, "dist", "aur", "ganbaru-ai-bin");
const RELEASE_REPOSITORY = process.env.GANBARU_AI_RELEASE_REPOSITORY?.trim()
  || "opengrimoire/ganbaru-ai";
const PACKAGE_NAME = "ganbaru-ai-bin";
const PACKAGE_DESCRIPTION =
  "Local, privacy-first productivity app for reducing procrastination and burnout";
const MAINTAINER = "Victor Benito Garcia Rocha <victorbenitogr@gmail.com>";

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
 * Extracts the upstream app version from a release tag.
 *
 * @param {string} tagName Release tag name.
 * @returns {string} Upstream version.
 */
function versionFromTag(tagName) {
  const match = tagName.match(/^app-v([0-9]+(?:\.[0-9]+){2})$/u);
  if (!match) {
    throw new Error(`Release tag ${tagName} must look like app-v0.1.0`);
  }
  return match[1];
}

/**
 * Computes a file SHA256 checksum.
 *
 * @param {string} filePath File path.
 * @returns {string} Hex encoded SHA256 checksum.
 */
function sha256(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

/**
 * Finds the release Debian package for a version.
 *
 * @param {string} version Upstream version.
 * @returns {string} Absolute Debian package path.
 */
function debPackage(version) {
  const expectedName = `ganbaru-ai_${version}_amd64.deb`;
  const assetPath = path.join(ASSET_DIR, expectedName);
  if (statSync(assetPath, { throwIfNoEntry: false })?.isFile()) {
    return assetPath;
  }

  const available = readdirSync(ASSET_DIR).filter((name) => name.endsWith(".deb")).sort();
  throw new Error(`Expected ${expectedName} in ${ASSET_DIR}, found ${available.join(", ")}`);
}

/**
 * Writes AUR package files.
 *
 * @param {string} version Upstream version.
 * @param {string} checksum Debian package checksum.
 */
function writeAurFiles(version, checksum) {
  mkdirSync(AUR_DIR, { recursive: true });

  const sourceUrl =
    `https://github.com/${RELEASE_REPOSITORY}/releases/download/app-v\${pkgver}/ganbaru-ai_\${pkgver}_amd64.deb`;
  const resolvedSourceUrl =
    `https://github.com/${RELEASE_REPOSITORY}/releases/download/app-v${version}/ganbaru-ai_${version}_amd64.deb`;

  writeFileSync(
    path.join(AUR_DIR, "PKGBUILD"),
    [
      `# Maintainer: ${MAINTAINER}`,
      `pkgname=${PACKAGE_NAME}`,
      `pkgver=${version}`,
      "pkgrel=1",
      `pkgdesc="${PACKAGE_DESCRIPTION}"`,
      "arch=('x86_64')",
      `url="https://github.com/${RELEASE_REPOSITORY}"`,
      "license=('AGPL-3.0-only')",
      "depends=(",
      "  'alsa-lib'",
      "  'gtk3'",
      "  'hicolor-icon-theme'",
      "  'libayatana-appindicator'",
      "  'webkit2gtk-4.1'",
      ")",
      "makedepends=('libarchive')",
      "provides=('ganbaru-ai')",
      "conflicts=('ganbaru-ai')",
      "options=('!strip' '!debug')",
      `source_x86_64=("ganbaru-ai_\${pkgver}_amd64.deb::${sourceUrl}")`,
      `sha256sums_x86_64=('${checksum}')`,
      `noextract=("ganbaru-ai_\${pkgver}_amd64.deb")`,
      "",
      "prepare() {",
      '  bsdtar -xf "ganbaru-ai_${pkgver}_amd64.deb"',
      "}",
      "",
      "package() {",
      '  bsdtar -xf data.tar.* -C "$pkgdir"',
      "",
      '  rm -rf "$pkgdir/usr/lib/ganbaru-ai/package-repo"',
      '  rmdir "$pkgdir/usr/lib/ganbaru-ai" 2>/dev/null || true',
      '  rmdir "$pkgdir/usr/lib" 2>/dev/null || true',
      "}",
      "",
    ].join("\n"),
    "utf8",
  );

  writeFileSync(
    path.join(AUR_DIR, ".SRCINFO"),
    [
      `pkgbase = ${PACKAGE_NAME}`,
      `\tpkgdesc = ${PACKAGE_DESCRIPTION}`,
      `\tpkgver = ${version}`,
      "\tpkgrel = 1",
      `\turl = https://github.com/${RELEASE_REPOSITORY}`,
      "\tarch = x86_64",
      "\tlicense = AGPL-3.0-only",
      "\tmakedepends = libarchive",
      "\tdepends = alsa-lib",
      "\tdepends = gtk3",
      "\tdepends = hicolor-icon-theme",
      "\tdepends = libayatana-appindicator",
      "\tdepends = webkit2gtk-4.1",
      "\tprovides = ganbaru-ai",
      "\tconflicts = ganbaru-ai",
      `\tnoextract = ganbaru-ai_${version}_amd64.deb`,
      "\toptions = !strip",
      "\toptions = !debug",
      `\tsource_x86_64 = ganbaru-ai_${version}_amd64.deb::${resolvedSourceUrl}`,
      `\tsha256sums_x86_64 = ${checksum}`,
      "",
      `pkgname = ${PACKAGE_NAME}`,
      "",
    ].join("\n"),
    "utf8",
  );

  writeFileSync(
    path.join(AUR_DIR, ".gitignore"),
    ["*.pkg.tar.zst", "*.deb", "pkg/", "src/", ""].join("\n"),
    "utf8",
  );
}

const releaseTag = readRequiredEnv("RELEASE_TAG_NAME");
const version = versionFromTag(releaseTag);
const debAsset = debPackage(version);
const checksum = sha256(debAsset);

writeAurFiles(version, checksum);
console.log(`Wrote ${PACKAGE_NAME} ${version} in ${path.relative(REPO_ROOT, AUR_DIR)}`);
