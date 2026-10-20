#!/usr/bin/env node

import { writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { packageRepoPublicKey, updaterPublicKey } from "./release-public-keys.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..", "..");
const RELEASE_CONFIG_PATH = path.join(
  CLIENT_ROOT,
  "src-tauri",
  "tauri.release.conf.json",
);
const PACKAGE_REPO_PUBLIC_KEY_BUNDLE_SOURCE =
  "release-keys/ganbaru-package-repo.asc";
const PACKAGE_REPO_PUBLIC_KEY_INSTALL_PATH =
  "/usr/lib/ganbaru/package-repo/ganbaru-package-repo.asc";
const POLKIT_POLICY_BUNDLE_SOURCE =
  "packaging/linux/org.opengrimoire.ganbaru.policy";
const POLKIT_POLICY_INSTALL_PATH =
  "/usr/share/polkit-1/actions/org.opengrimoire.ganbaru.policy";
const LINKING_FIREWALL_HELPER_BUNDLE_SOURCE =
  "../../../target/release/ganbaru-linking-firewall";
const LINKING_FIREWALL_HELPER_INSTALL_PATH =
  "/usr/lib/ganbaru/ganbaru-linking-firewall";
const GITHUB_REPOSITORY_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u;

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
    process.env.GANBARU_RELEASE_REPOSITORY?.trim() ||
    process.env.GITHUB_REPOSITORY?.trim();
  if (!repository || !GITHUB_REPOSITORY_PATTERN.test(repository)) {
    throw new Error("GANBARU_RELEASE_REPOSITORY must be an owner/name repository");
  }

  return `https://github.com/${repository}/releases/latest/download/latest.json`;
}

// The bundled package repository key is the committed file itself; reading it validates the format and any
// configured copy before the bundle refers to it.
packageRepoPublicKey();

const config = {
  $schema: "https://schema.tauri.app/config/2",
  bundle: {
    createUpdaterArtifacts: false,
    linux: {
      deb: {
        files: {
          [PACKAGE_REPO_PUBLIC_KEY_INSTALL_PATH]: PACKAGE_REPO_PUBLIC_KEY_BUNDLE_SOURCE,
          [POLKIT_POLICY_INSTALL_PATH]: POLKIT_POLICY_BUNDLE_SOURCE,
          [LINKING_FIREWALL_HELPER_INSTALL_PATH]: LINKING_FIREWALL_HELPER_BUNDLE_SOURCE,
        },
        postInstallScript: "packaging/deb/postinst",
        preRemoveScript: "packaging/deb/prerm",
        postRemoveScript: "packaging/deb/postrm",
      },
      rpm: {
        files: {
          [PACKAGE_REPO_PUBLIC_KEY_INSTALL_PATH]: PACKAGE_REPO_PUBLIC_KEY_BUNDLE_SOURCE,
          [POLKIT_POLICY_INSTALL_PATH]: POLKIT_POLICY_BUNDLE_SOURCE,
          [LINKING_FIREWALL_HELPER_INSTALL_PATH]: LINKING_FIREWALL_HELPER_BUNDLE_SOURCE,
        },
        postInstallScript: "packaging/rpm/postinstall.sh",
        preRemoveScript: "packaging/rpm/preremove.sh",
        postRemoveScript: "packaging/rpm/postremove.sh",
      },
    },
  },
  plugins: {
    updater: {
      pubkey: updaterPublicKey(),
      endpoints: [updaterEndpoint()],
      windows: {
        installMode: "passive",
      },
    },
  },
};

writeFileSync(RELEASE_CONFIG_PATH, `${JSON.stringify(config, null, 2)}\n`, "utf8");
console.log(`Wrote ${path.relative(CLIENT_ROOT, RELEASE_CONFIG_PATH)}`);
