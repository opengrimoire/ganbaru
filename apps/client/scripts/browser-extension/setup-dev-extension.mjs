#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "../../../..");
const extensionDir = join(repoRoot, "extensions", "chromium");
const devExtensionDir = join(repoRoot, "extensions", "chromium-dev");
const devHostName = "org.opengrimoire.ganbaru_ai.distractions_dev";

const build = spawnSync("cargo", ["build", "--bin", "ganbaru-ai-native-messaging"], {
  cwd: repoRoot,
  stdio: "inherit",
});

if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

if (!existsSync(extensionDir)) {
  console.error(`Extension folder not found: ${extensionDir}`);
  process.exit(1);
}

rmSync(devExtensionDir, { recursive: true, force: true });
cpSync(extensionDir, devExtensionDir, { recursive: true });

const devManifestPath = join(devExtensionDir, "manifest.json");
const devManifest = JSON.parse(readFileSync(devManifestPath, "utf8"));
devManifest.name = "Ganbaru AI dev";
devManifest.description = "Development build of the Ganbaru AI anti-distraction extension.";
devManifest.action = {
  ...devManifest.action,
  default_title: "Ganbaru AI dev",
};
writeFileSync(devManifestPath, `${JSON.stringify(devManifest, null, 2)}\n`);

writeFileSync(
  join(devExtensionDir, "host-config.js"),
  [
    "// Chromium native messaging host names allow underscores but not hyphens.",
    `export const HOST_NAME = "${devHostName}";`,
    "",
  ].join("\n"),
);

console.log("Chromium extension folder is ready.");
console.log("");
console.log("Daily-use extension folder:");
console.log(extensionDir);
console.log("");
console.log("Dev-test extension folder:");
console.log(devExtensionDir);
console.log("");
console.log("In your Chromium-based browser:");
console.log("1. Open the extensions manager manually.");
console.log("   Chrome or Chromium: chrome://extensions/");
console.log("   Brave: brave://extensions/");
console.log("   Edge: edge://extensions/");
console.log("2. Turn on Developer mode in the extensions page.");
console.log("3. Click Load unpacked for the daily-use extension.");
console.log(extensionDir);
console.log("4. Optionally click Load unpacked again for the dev-test extension.");
console.log(devExtensionDir);
console.log("");
console.log("After the Ganbaru AI card appears, copy its extension id and run:");
console.log("node apps/client/scripts/browser-extension/install-native-host.mjs <extension-id> <chrome|chromium|brave|edge> app");
console.log("");
console.log("After the Ganbaru AI dev card appears, copy its extension id and run:");
console.log("node apps/client/scripts/browser-extension/install-native-host.mjs <extension-id> <chrome|chromium|brave|edge> dev");
