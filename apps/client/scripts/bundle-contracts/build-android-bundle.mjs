import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(scriptDir, "..", "..");
const outputDir = path.join(clientDir, ".bundle-contracts", "android");
const previousPlatform = process.env.TAURI_ENV_PLATFORM;

try {
  process.env.TAURI_ENV_PLATFORM = "android";
  const { build } = await import("vite");
  await build({
    root: clientDir,
    logLevel: "warn",
    build: {
      outDir: outputDir,
      emptyOutDir: true,
    },
  });
} finally {
  if (previousPlatform === undefined) delete process.env.TAURI_ENV_PLATFORM;
  else process.env.TAURI_ENV_PLATFORM = previousPlatform;
}
