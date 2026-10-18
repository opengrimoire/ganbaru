#!/usr/bin/env node

import { Buffer } from "node:buffer";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const REPO_ROOT = path.resolve(CLIENT_ROOT, "..", "..");
const ASSET_DIR = path.join(REPO_ROOT, "dist", "release-assets");
const APK_PATTERN = /^Ganbaru_[0-9A-Za-z.+-]+_android_universal\.apk$/u;
const AAB_PATTERN = /^Ganbaru_[0-9A-Za-z.+-]+_android_universal\.aab$/u;
/** Page alignment for uncompressed native libraries, matching Android 15 16 KB page support. */
const ZIPALIGN_ARGUMENTS = ["-P", "16", "-f", "4"];

/** Read a required value without printing it. */
function requiredEnvironmentValue(name) {
  const value = process.env[name];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${name} must be set`);
  }
  if (value.includes("\0")) throw new Error(`${name} must not contain a null byte`);
  return value;
}

/** Decode a canonical base64 secret and reject malformed or empty keystores. */
function decodeKeystore(value) {
  const compact = value.replace(/\s/gu, "");
  if (!compact || !/^[A-Za-z0-9+/]+={0,2}$/u.test(compact) || compact.length % 4 !== 0) {
    throw new Error("ANDROID_KEYSTORE_BASE64 must contain canonical base64");
  }
  const bytes = Buffer.from(compact, "base64");
  if (bytes.length === 0) throw new Error("ANDROID_KEYSTORE_BASE64 decoded to an empty file");
  if (bytes.toString("base64") !== compact) {
    throw new Error("ANDROID_KEYSTORE_BASE64 is malformed");
  }
  return bytes;
}

/** Require exactly one staged asset whose name matches the release pattern. */
function requireAsset(pattern) {
  const matches = readdirSync(ASSET_DIR).filter((name) => pattern.test(name));
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one Android release asset matching ${pattern}, found ${matches.length}`);
  }
  const assetPath = path.join(ASSET_DIR, matches[0]);
  if (!statSync(assetPath).isFile()) throw new Error(`${matches[0]} is not a regular file`);
  return assetPath;
}

/** Run a tool that receives secrets only through the inherited environment. */
function run(command, args) {
  const result = spawnSync(command, args, { env: process.env, stdio: "inherit" });
  if (result.error) throw new Error(`${path.basename(command)} could not start: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${path.basename(command)} exited with status ${result.status}`);
}

const runnerTemp = requiredEnvironmentValue("RUNNER_TEMP");
const androidHome = requiredEnvironmentValue("ANDROID_HOME");
const buildToolsVersion = requiredEnvironmentValue("ANDROID_BUILD_TOOLS_VERSION");
const keyAlias = requiredEnvironmentValue("ANDROID_KEY_ALIAS");
requiredEnvironmentValue("ANDROID_KEYSTORE_PASSWORD");
requiredEnvironmentValue("ANDROID_KEY_PASSWORD");
const keystore = decodeKeystore(requiredEnvironmentValue("ANDROID_KEYSTORE_BASE64"));

const buildTools = path.join(androidHome, "build-tools", buildToolsVersion);
const zipalign = path.join(buildTools, "zipalign");
const apksigner = path.join(buildTools, "apksigner");
const apk = requireAsset(APK_PATTERN);
const aab = requireAsset(AAB_PATTERN);

const workDirectory = mkdtempSync(path.join(runnerTemp, "ganbaru-android-signing-"));
try {
  const keystorePath = path.join(workDirectory, "release.jks");
  writeFileSync(keystorePath, keystore, { mode: 0o600 });

  const alignedApk = path.join(workDirectory, "aligned.apk");
  const signedApk = path.join(workDirectory, "signed.apk");
  run(zipalign, [...ZIPALIGN_ARGUMENTS, apk, alignedApk]);
  run(apksigner, [
    "sign",
    "--ks", keystorePath,
    "--ks-key-alias", keyAlias,
    "--ks-pass", "env:ANDROID_KEYSTORE_PASSWORD",
    "--key-pass", "env:ANDROID_KEY_PASSWORD",
    "--out", signedApk,
    alignedApk,
  ]);
  run(apksigner, ["verify", "--verbose", signedApk]);

  const signedAab = path.join(workDirectory, "signed.aab");
  run("jarsigner", [
    "-keystore", keystorePath,
    "-storepass:env", "ANDROID_KEYSTORE_PASSWORD",
    "-keypass:env", "ANDROID_KEY_PASSWORD",
    "-signedjar", signedAab,
    aab,
    keyAlias,
  ]);
  run("jarsigner", [
    "-verify", "-strict",
    "-keystore", keystorePath,
    "-storepass:env", "ANDROID_KEYSTORE_PASSWORD",
    signedAab,
    keyAlias,
  ]);

  copyFileSync(signedApk, apk);
  copyFileSync(signedAab, aab);
  console.log(`Signed ${path.basename(apk)} and ${path.basename(aab)}`);
} finally {
  rmSync(workDirectory, { recursive: true, force: true });
}
