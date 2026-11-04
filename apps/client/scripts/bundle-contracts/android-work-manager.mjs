import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

/** The foreground service WorkManager declares for long-running and pre-Android 12 expedited work. */
export const WORK_MANAGER_FOREGROUND_SERVICE = "androidx.work.impl.foreground.SystemForegroundService";

const TOOLS_NAMESPACE = 'xmlns:tools="http://schemas.android.com/tools"';
const MERGED_MANIFEST_DIRECTORY = /^merged_manifests?$/u;

/**
 * Read the attributes of every `<service>` element in a manifest.
 *
 * @param {string} source Manifest XML text.
 * @returns {Array<Map<string, string>>} One attribute map per service element.
 */
export function serviceElements(source) {
  const withoutComments = source.replace(/<!--[\s\S]*?-->/gu, "");
  return [...withoutComments.matchAll(/<service\b([^>]*)>/gu)].map(([, attributes]) => {
    const values = new Map();
    for (const [, name, value] of attributes.matchAll(/([\w:.-]+)\s*=\s*"([^"]*)"/gu)) {
      values.set(name, value);
    }
    return values;
  });
}

/**
 * Check that the app manifest removes WorkManager's foreground service from the merged manifest.
 *
 * @param {string} source App manifest XML text.
 * @returns {string[]} Contract failures, empty when the removal is declared.
 */
export function sourceManifestFailures(source) {
  const failures = [];
  if (!source.includes(TOOLS_NAMESPACE)) {
    failures.push("app manifest must declare the tools namespace");
  }
  const declarations = serviceElements(source).filter(
    (attributes) => attributes.get("android:name") === WORK_MANAGER_FOREGROUND_SERVICE,
  );
  if (declarations.length !== 1 || declarations[0].get("tools:node") !== "remove") {
    failures.push(`app manifest must remove ${WORK_MANAGER_FOREGROUND_SERVICE} with tools:node="remove"`);
  }
  return failures;
}

/**
 * Check that a merged manifest declares no WorkManager foreground service.
 *
 * @param {string} source Merged manifest XML text.
 * @param {string} label Name of the merged manifest in failures.
 * @returns {string[]} Contract failures, empty when no such service is declared.
 */
export function mergedManifestFailures(source, label) {
  const declared = serviceElements(source).some(
    (attributes) => attributes.get("android:name") === WORK_MANAGER_FOREGROUND_SERVICE,
  );
  return declared ? [`${label} must not declare ${WORK_MANAGER_FOREGROUND_SERVICE}`] : [];
}

/**
 * Find merged manifests an Android build left under `buildDir`, or none when it was never built.
 *
 * @param {string} buildDir The app module build directory.
 * @returns {Promise<string[]>} Paths of merged `AndroidManifest.xml` files.
 */
export async function findMergedManifests(buildDir) {
  const intermediates = path.join(buildDir, "intermediates");
  let entries;
  try {
    entries = await readdir(intermediates, { withFileTypes: true });
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return [];
    throw error;
  }
  const manifests = [];
  for (const entry of entries) {
    if (!entry.isDirectory() || !MERGED_MANIFEST_DIRECTORY.test(entry.name)) continue;
    const directory = path.join(intermediates, entry.name);
    const files = await readdir(directory, { recursive: true, withFileTypes: true });
    for (const file of files) {
      if (file.isFile() && file.name === "AndroidManifest.xml") {
        manifests.push(path.join(file.parentPath, file.name));
      }
    }
  }
  return manifests.sort();
}

/**
 * Check the app manifest declaration and, when an Android build exists, every merged manifest.
 *
 * @param {string} manifestSource App manifest XML text.
 * @param {string} buildDir The app module build directory.
 * @returns {Promise<string[]>} Contract failures.
 */
export async function workManagerForegroundServiceFailures(manifestSource, buildDir) {
  const failures = sourceManifestFailures(manifestSource);
  for (const manifest of await findMergedManifests(buildDir)) {
    const label = `merged manifest ${path.relative(buildDir, manifest)}`;
    failures.push(...mergedManifestFailures(await readFile(manifest, "utf8"), label));
  }
  return failures;
}
