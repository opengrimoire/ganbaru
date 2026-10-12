#!/usr/bin/env node

import { Buffer } from "node:buffer";
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const ANDROID_ROOT = path.join(CLIENT_ROOT, "src-tauri", "gen", "android");
const PROPERTIES_PATH = path.join(ANDROID_ROOT, "keystore.properties");

/** Read a required signing value without printing it. */
function requiredEnvironmentValue(name) {
  const value = process.env[name];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${name} must be set`);
  }
  if (value.includes("\0")) throw new Error(`${name} must not contain a null byte`);
  return value;
}

/** Encode an arbitrary Java properties value without exposing separators or newlines. */
function escapePropertyValue(value) {
  return [...value].map((character, index) => {
    if (character === "\\") return "\\\\";
    if (character === "\n") return "\\n";
    if (character === "\r") return "\\r";
    if (character === "\t") return "\\t";
    if (character === "=" || character === ":" || character === "#" || character === "!") {
      return `\\${character}`;
    }
    if (character === " " && index === 0) return "\\ ";
    return character;
  }).join("");
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

const runnerTemp = requiredEnvironmentValue("RUNNER_TEMP");
const keyAlias = requiredEnvironmentValue("ANDROID_KEY_ALIAS");
const storePassword = requiredEnvironmentValue("ANDROID_KEYSTORE_PASSWORD");
const keyPassword = requiredEnvironmentValue("ANDROID_KEY_PASSWORD");
const keystore = decodeKeystore(requiredEnvironmentValue("ANDROID_KEYSTORE_BASE64"));
const keystorePath = path.join(runnerTemp, "ganbaru-ai-android-release.jks");

mkdirSync(ANDROID_ROOT, { recursive: true });
writeFileSync(keystorePath, keystore, { mode: 0o600 });
chmodSync(keystorePath, 0o600);

const properties = [
  `storeFile=${escapePropertyValue(keystorePath)}`,
  `storePassword=${escapePropertyValue(storePassword)}`,
  `keyAlias=${escapePropertyValue(keyAlias)}`,
  `keyPassword=${escapePropertyValue(keyPassword)}`,
  "",
].join("\n");
writeFileSync(PROPERTIES_PATH, properties, { encoding: "utf8", mode: 0o600 });
chmodSync(PROPERTIES_PATH, 0o600);

console.log("Prepared Android release signing files in protected runner storage.");
