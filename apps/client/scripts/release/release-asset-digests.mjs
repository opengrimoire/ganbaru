#!/usr/bin/env node

import { createHash } from "node:crypto";
import { appendFileSync, copyFileSync, lstatSync, mkdirSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DIGEST_PATTERN = /^[0-9a-f]{64}$/u;
const ARTIFACT_NAME_PATTERN = /^[a-z0-9-]+$/u;

/**
 * Hashes the names and contents of every file in a flat asset directory.
 *
 * @param {string} directory Directory that must contain only regular files.
 * @returns {string} Lowercase hex SHA-256 digest of the sorted manifest.
 */
export function assetDirectoryDigest(directory) {
  const manifest = createHash("sha256");
  for (const name of readdirSync(directory).sort()) {
    const filePath = path.join(directory, name);
    if (!lstatSync(filePath).isFile()) throw new Error(`${name} in ${directory} is not a regular file`);
    const fileDigest = createHash("sha256").update(readFileSync(filePath)).digest("hex");
    manifest.update(`${name}\0${fileDigest}\n`);
  }
  return manifest.digest("hex");
}

/**
 * Names the environment variable that carries an artifact's expected digest.
 *
 * @param {string} artifact Artifact name such as `unsigned-linux-x64`.
 * @returns {string} Variable name such as `DIGEST_UNSIGNED_LINUX_X64`.
 */
export function digestVariable(artifact) {
  return `DIGEST_${artifact.toUpperCase().replaceAll("-", "_")}`;
}

/** Writes the digest of a staged asset directory as a step output. */
function recordDigest(directory, outputKey) {
  const outputPath = process.env.GITHUB_OUTPUT;
  if (!outputPath) throw new Error("GITHUB_OUTPUT must be set");
  const digest = assetDirectoryDigest(directory);
  appendFileSync(outputPath, `${outputKey}=${digest}\n`);
  console.log(`${outputKey} assets digest ${digest}`);
}

/** Verifies each downloaded artifact against its build job digest and merges them without overwrites. */
function collectArtifacts(sourceRoot, targetDirectory, artifacts) {
  mkdirSync(targetDirectory, { recursive: true });
  for (const artifact of artifacts) {
    if (!ARTIFACT_NAME_PATTERN.test(artifact)) throw new Error(`Invalid artifact name: ${artifact}`);
    const expected = process.env[digestVariable(artifact)] ?? "";
    if (!DIGEST_PATTERN.test(expected)) throw new Error(`${digestVariable(artifact)} must be a SHA-256 digest`);
    const directory = path.join(sourceRoot, artifact);
    const actual = assetDirectoryDigest(directory);
    if (actual !== expected) throw new Error(`${artifact} does not match the digest its build job recorded`);
    for (const name of readdirSync(directory).sort()) {
      const target = path.join(targetDirectory, name);
      if (statSync(target, { throwIfNoEntry: false })) throw new Error(`Release asset ${name} appears in more than one artifact`);
      copyFileSync(path.join(directory, name), target);
    }
    console.log(`Verified ${artifact}`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  if (command === "record" && args.length === 2) {
    recordDigest(args[0], args[1]);
  } else if (command === "collect" && args.length >= 3) {
    collectArtifacts(args[0], args[1], args.slice(2));
  } else {
    throw new Error("Usage: release-asset-digests.mjs record <dir> <output-key> | collect <source-root> <target-dir> <artifact>...");
  }
}
