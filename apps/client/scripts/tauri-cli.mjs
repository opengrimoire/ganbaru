#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { existsSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CLIENT_ROOT = path.resolve(SCRIPT_DIR, "..");
const DEV_CONFIG = path.join("src-tauri", "tauri.dev.conf.json");
const TAURI_CLI_ENTRY = require.resolve("@tauri-apps/cli/tauri.js");
const PRESERVE_GDK_BACKEND_ENV = "GANBARU_AI_DEV_PRESERVE_GDK_BACKEND";
const ANDROID_JAVA_HOME_ENV = "GANBARU_AI_ANDROID_JAVA_HOME";
const ANDROID_JAVA_MAJOR = 21;

/**
 * Finds the boundary between Tauri CLI options and application arguments.
 *
 * @param {string[]} args CLI arguments.
 * @returns {number} Index where application arguments begin.
 */
function appArgsStart(args) {
  const markerIndex = args.indexOf("--");
  return markerIndex === -1 ? args.length : markerIndex;
}

/**
 * Checks whether the command already provides a Tauri config override.
 *
 * @param {string[]} args CLI arguments.
 * @returns {boolean} Whether a config override is present.
 */
function hasConfigOverride(args) {
  const end = appArgsStart(args);
  for (let i = 1; i < end; i += 1) {
    const arg = args[i];
    if (arg === "--config" || arg === "-c" || arg.startsWith("--config=")) {
      return true;
    }
  }
  return false;
}

/**
 * Adds Ganbaru AI's dev identity to desktop dev runs by default.
 *
 * @param {string[]} args CLI arguments.
 * @returns {string[]} Arguments to pass to the Tauri CLI.
 */
function withDevConfig(args) {
  if (args[0] !== "dev" || hasConfigOverride(args)) {
    return args;
  }

  const insertAt = appArgsStart(args);
  return [
    ...args.slice(0, insertAt),
    "--config",
    DEV_CONFIG,
    ...args.slice(insertAt),
  ];
}

/**
 * Checks whether the CLI invocation starts a desktop or mobile development run.
 *
 * @param {string[]} args CLI arguments.
 * @returns {boolean} Whether the command compiles a development application.
 */
function isDevelopmentCommand(args) {
  return args[0] === "dev" ||
    ((args[0] === "android" || args[0] === "ios") && args[1] === "dev");
}

/** Check whether the Tauri invocation targets Android. */
function isAndroidCommand(args) {
  return args[0] === "android";
}

/** Resolve the Java executable below a candidate JDK home. */
function javaExecutable(javaHome) {
  const executable = process.platform === "win32" ? "java.exe" : "java";
  return path.join(javaHome, "bin", executable);
}

/** Read the major Java version for a candidate JDK home. */
function javaMajorVersion(javaHome) {
  const executable = javaExecutable(javaHome);
  if (!existsSync(executable)) return null;
  const result = spawnSync(executable, ["-version"], { encoding: "utf8" });
  const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
  const match = output.match(/version\s+"(\d+)(?:\.|\x22)/u)
    ?? output.match(/(?:openjdk|java)\s+(\d+)(?:\.|\s)/iu);
  return match ? Number.parseInt(match[1], 10) : null;
}

/** Discover the JDK home represented by the Java executable on PATH. */
function javaHomeFromPath(environment) {
  const locator = process.platform === "win32" ? "where.exe" : "which";
  const result = spawnSync(locator, [process.platform === "win32" ? "java.exe" : "java"], {
    encoding: "utf8",
    env: environment,
  });
  const executable = result.stdout?.split(/\r?\n/u).find((line) => line.trim())?.trim();
  if (!executable) return null;
  try {
    return path.dirname(path.dirname(realpathSync(executable)));
  } catch {
    return null;
  }
}

/** Select the supported JDK for Gradle without depending on Android Studio's runtime. */
function androidJavaHome(environment) {
  const explicit = environment[ANDROID_JAVA_HOME_ENV]?.trim();
  if (explicit) {
    if (javaMajorVersion(explicit) !== ANDROID_JAVA_MAJOR) {
      throw new Error(`${ANDROID_JAVA_HOME_ENV} must point to JDK ${ANDROID_JAVA_MAJOR}`);
    }
    return explicit;
  }

  const candidates = [
    environment.JAVA_HOME,
    environment.JAVA_HOME_21_X64,
    javaHomeFromPath(environment),
    process.platform === "linux" ? "/usr/lib/jvm/java-21-openjdk-amd64" : null,
    process.platform === "linux" ? "/usr/lib/jvm/temurin-21-jdk-amd64" : null,
  ];
  const seen = new Set();
  for (const candidate of candidates) {
    const normalized = candidate?.trim();
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    if (javaMajorVersion(normalized) === ANDROID_JAVA_MAJOR) return normalized;
  }

  throw new Error(
    `Android builds require JDK ${ANDROID_JAVA_MAJOR}. Set ${ANDROID_JAVA_HOME_ENV} or JAVA_HOME to its installation directory.`,
  );
}

/**
 * Builds the environment for the Tauri child process.
 *
 * Development runs default to one Cargo build job, and Android runs use the
 * supported JDK as `JAVA_HOME`. Development terminals can inherit an X11 override while the desktop session
 * itself uses Wayland. Letting GTK select the native backend keeps development
 * rendering on the same path as an installed Ganbaru AI build.
 *
 * @param {string[]} args CLI arguments.
 * @param {NodeJS.ProcessEnv} environment Parent process environment.
 * @returns {NodeJS.ProcessEnv} Environment to pass to the Tauri CLI.
 */
function tauriChildEnvironment(args, environment) {
  const developmentCommand = isDevelopmentCommand(args);
  const androidCommand = isAndroidCommand(args);
  if (!developmentCommand && !androidCommand) {
    return environment;
  }

  const childEnvironment = { ...environment };
  if (developmentCommand && childEnvironment.CARGO_BUILD_JOBS === undefined) {
    childEnvironment.CARGO_BUILD_JOBS = "1";
  }

  if (androidCommand) {
    childEnvironment.JAVA_HOME = androidJavaHome(childEnvironment);
  }

  const usesWaylandSession =
    childEnvironment.XDG_SESSION_TYPE?.trim().toLowerCase() === "wayland" &&
    Boolean(childEnvironment.WAYLAND_DISPLAY?.trim());
  const forcesX11 = childEnvironment.GDK_BACKEND?.trim().toLowerCase() === "x11";
  const preservesGdkBackend = childEnvironment[PRESERVE_GDK_BACKEND_ENV]?.trim() === "1";

  if (developmentCommand && usesWaylandSession && forcesX11 && !preservesGdkBackend) {
    delete childEnvironment.GDK_BACKEND;
  }

  return childEnvironment;
}

const args = process.argv.slice(2);
let childEnv;
try {
  childEnv = tauriChildEnvironment(args, process.env);
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`Failed to prepare Tauri CLI environment: ${message}`);
  process.exit(1);
}

const child = spawn(process.execPath, [TAURI_CLI_ENTRY, ...withDevConfig(args)], {
  cwd: CLIENT_ROOT,
  env: childEnv,
  stdio: "inherit",
});

child.on("error", (error) => {
  console.error(`Failed to run Tauri CLI: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
