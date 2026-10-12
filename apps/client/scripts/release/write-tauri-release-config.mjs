#!/usr/bin/env node

import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const RELEASE_CONFIG_PATH = path.join(
  CLIENT_ROOT,
  "src-tauri",
  "tauri.release.conf.json",
);
const PACKAGE_REPO_PUBLIC_KEY_PATH = path.join(
  CLIENT_ROOT,
  "src-tauri",
  "package-repo",
  "ganbaru-ai-package-repo.asc",
);
const PACKAGE_REPO_PUBLIC_KEY_BUNDLE_SOURCE =
  "package-repo/ganbaru-ai-package-repo.asc";
const PACKAGE_REPO_PUBLIC_KEY_INSTALL_PATH =
  "/usr/lib/ganbaru-ai/package-repo/ganbaru-ai-package-repo.asc";
const POLKIT_POLICY_BUNDLE_SOURCE =
  "packaging/linux/org.opengrimoire.ganbaruai.policy";
const POLKIT_POLICY_INSTALL_PATH =
  "/usr/share/polkit-1/actions/org.opengrimoire.ganbaruai.policy";
const GITHUB_REPOSITORY_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u;

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
 * Builds the release feed URL used by Tauri's updater.
 *
 * @returns {string} HTTPS endpoint for the static updater JSON.
 */
function updaterEndpoint() {
  const override = process.env.TAURI_UPDATER_ENDPOINT?.trim();
  if (override) {
    if (!override.startsWith("https://")) {
      throw new Error("TAURI_UPDATER_ENDPOINT must use https");
    }
    return override;
  }

  const repository =
    process.env.GANBARU_AI_RELEASE_REPOSITORY?.trim() ||
    process.env.GITHUB_REPOSITORY?.trim();
  if (!repository || !GITHUB_REPOSITORY_PATTERN.test(repository)) {
    throw new Error("GANBARU_AI_RELEASE_REPOSITORY must be an owner/name repository");
  }

  return `https://github.com/${repository}/releases/latest/download/latest.json`;
}

/**
 * Normalizes the package repository public key used by Linux installers.
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

const packageRepoPublicKeyContent = packageRepoPublicKey();
mkdirSync(path.dirname(PACKAGE_REPO_PUBLIC_KEY_PATH), { recursive: true });
writeFileSync(PACKAGE_REPO_PUBLIC_KEY_PATH, packageRepoPublicKeyContent, "utf8");

const config = {
  $schema: "https://schema.tauri.app/config/2",
  bundle: {
    createUpdaterArtifacts: false,
    linux: {
      deb: {
        files: {
          [PACKAGE_REPO_PUBLIC_KEY_INSTALL_PATH]: PACKAGE_REPO_PUBLIC_KEY_BUNDLE_SOURCE,
          [POLKIT_POLICY_INSTALL_PATH]: POLKIT_POLICY_BUNDLE_SOURCE,
        },
        postInstallScript: "packaging/deb/postinst",
        postRemoveScript: "packaging/deb/postrm",
      },
      rpm: {
        files: {
          [PACKAGE_REPO_PUBLIC_KEY_INSTALL_PATH]: PACKAGE_REPO_PUBLIC_KEY_BUNDLE_SOURCE,
          [POLKIT_POLICY_INSTALL_PATH]: POLKIT_POLICY_BUNDLE_SOURCE,
        },
        postInstallScript: "packaging/rpm/postinstall.sh",
        postRemoveScript: "packaging/rpm/postremove.sh",
      },
    },
  },
  plugins: {
    updater: {
      pubkey: readRequiredEnv("TAURI_UPDATER_PUBLIC_KEY"),
      endpoints: [updaterEndpoint()],
      windows: {
        installMode: "passive",
      },
    },
  },
};

writeFileSync(RELEASE_CONFIG_PATH, `${JSON.stringify(config, null, 2)}\n`, "utf8");
console.log(`Wrote ${path.relative(CLIENT_ROOT, RELEASE_CONFIG_PATH)}`);
