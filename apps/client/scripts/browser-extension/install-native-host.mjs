#!/usr/bin/env node
import { chmodSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "../../../..");

const args = process.argv.slice(2);
const extensionId = args[0];
const browserNames = new Set(["chrome", "chromium", "brave", "edge"]);
const appTargetAliases = new Map([
  ["app", "app"],
  ["prod", "app"],
  ["production", "app"],
  ["dev", "dev"],
  ["development", "dev"],
]);
const appIdentifiers = {
  app: "org.opengrimoire.ganbaruai",
  dev: "org.opengrimoire.ganbaruai.dev",
};
const hostNames = {
  app: "org.opengrimoire.ganbaru_ai.distractions",
  dev: "org.opengrimoire.ganbaru_ai.distractions_dev",
};

let browser = "chrome";
let appTarget = "app";

function printUsage() {
  console.error("Usage: node apps/client/scripts/browser-extension/install-native-host.mjs <extension-id> [chrome|chromium|brave|edge] [app|dev]");
  console.error("Use app for the normal Ganbaru AI app. Use dev only when testing against pnpm tauri dev.");
}

if (!extensionId || !/^[a-p]{32}$/.test(extensionId)) {
  if (extensionId) {
    console.error(`Invalid extension id. Expected 32 letters from a to p, got ${extensionId.length} characters.`);
  }
  printUsage();
  process.exit(1);
}

for (const arg of args.slice(1)) {
  if (browserNames.has(arg)) {
    browser = arg;
    continue;
  }
  const normalizedTarget = appTargetAliases.get(arg);
  if (normalizedTarget) {
    appTarget = normalizedTarget;
    continue;
  }
  console.error(`Unsupported argument: ${arg}`);
  printUsage();
  process.exit(1);
}

const binaryName = process.platform === "win32"
  ? "ganbaru-ai-native-messaging.exe"
  : "ganbaru-ai-native-messaging";
const hostPath = join(repoRoot, "target", "debug", binaryName);

if (!existsSync(hostPath)) {
  console.error(`Native host not found at ${hostPath}`);
  console.error("Run: pnpm -w run build:native-host");
  process.exit(1);
}

function linuxManifestDir() {
  const configHome = process.env.XDG_CONFIG_HOME || join(process.env.HOME || "", ".config");
  const profileDirs = {
    chrome: "google-chrome",
    chromium: "chromium",
    brave: join("BraveSoftware", "Brave-Browser"),
    edge: "microsoft-edge",
  };
  const profileDir = profileDirs[browser];
  if (!profileDir) {
    throw new Error(`Unsupported browser '${browser}'`);
  }
  return join(configHome, profileDir, "NativeMessagingHosts");
}

function macManifestDir() {
  const home = process.env.HOME || "";
  const profileDirs = {
    chrome: join("Google", "Chrome"),
    chromium: "Chromium",
    brave: join("BraveSoftware", "Brave-Browser"),
    edge: join("Microsoft Edge"),
  };
  const profileDir = profileDirs[browser];
  if (!profileDir) {
    throw new Error(`Unsupported browser '${browser}'`);
  }
  return join(home, "Library", "Application Support", profileDir, "NativeMessagingHosts");
}

function appConfigDir() {
  const identifier = appIdentifiers[appTarget];
  if (process.platform === "linux") {
    const configHome = process.env.XDG_CONFIG_HOME || join(process.env.HOME || "", ".config");
    return join(configHome, identifier);
  }
  if (process.platform === "darwin") {
    const home = process.env.HOME || "";
    return join(home, "Library", "Application Support", identifier);
  }
  throw new Error("This installer currently supports Linux and macOS. On Windows, create the native host manifest and registry key manually.");
}

function manifestDir() {
  if (process.platform === "linux") return linuxManifestDir();
  if (process.platform === "darwin") return macManifestDir();
  throw new Error("This installer currently supports Linux and macOS. On Windows, create the native host manifest and registry key manually.");
}

function shellQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

const hostManifestDir = manifestDir();
mkdirSync(hostManifestDir, { recursive: true });

const hostName = hostNames[appTarget];
const manifestPath = join(hostManifestDir, `${hostName}.json`);
const launcherPath = join(hostManifestDir, hostName);
const targetConfigDir = appConfigDir();
const launcher = [
  "#!/usr/bin/env sh",
  `GANBARU_AI_CONFIG_DIR=${shellQuote(targetConfigDir)}`,
  "export GANBARU_AI_CONFIG_DIR",
  `GANBARU_AI_NATIVE_HOST_NAME=${shellQuote(hostName)}`,
  "export GANBARU_AI_NATIVE_HOST_NAME",
  `exec ${shellQuote(hostPath)} "$@"`,
  "",
].join("\n");
writeFileSync(launcherPath, launcher, { mode: 0o755 });
chmodSync(launcherPath, 0o755);

const manifest = {
  name: hostName,
  description: `Ganbaru AI anti-distraction native host (${appTarget})`,
  path: launcherPath,
  type: "stdio",
  allowed_origins: [`chrome-extension://${extensionId}/`],
};

writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Installed ${hostName} for ${browser} ${appTarget} at ${manifestPath}`);
console.log(`Native host launcher: ${launcherPath}`);
console.log(`Ganbaru AI config: ${targetConfigDir}`);
