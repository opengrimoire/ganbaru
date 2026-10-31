import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  WORK_MANAGER_FOREGROUND_SERVICE,
  findMergedManifests,
  mergedManifestFailures,
  sourceManifestFailures,
  workManagerForegroundServiceFailures,
} from "./android-work-manager.mjs";

const TOOLS = 'xmlns:tools="http://schemas.android.com/tools"';
const REMOVAL = `<service android:name="${WORK_MANAGER_FOREGROUND_SERVICE}" tools:node="remove" />`;

/** Build an app manifest with the given root attributes and application children. */
function manifest(rootAttributes, children) {
  return `<manifest xmlns:android="http://schemas.android.com/apk/res/android" ${rootAttributes}>
  <application>
    ${children}
  </application>
</manifest>`;
}

const temporaryDirectories = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

/** Create a scratch app build directory. */
async function buildDirectory() {
  const directory = await mkdtemp(path.join(tmpdir(), "ganbaru-android-build-"));
  temporaryDirectories.push(directory);
  return directory;
}

describe("WorkManager foreground service contract", () => {
  it("accepts an app manifest that removes the WorkManager foreground service", () => {
    expect(sourceManifestFailures(manifest(TOOLS, REMOVAL))).toEqual([]);
  });

  it("requires the tools namespace and the remove marker", () => {
    expect(sourceManifestFailures(manifest("", REMOVAL))).toEqual(["app manifest must declare the tools namespace"]);
    const kept = `<service android:name="${WORK_MANAGER_FOREGROUND_SERVICE}" tools:node="merge" />`;
    expect(sourceManifestFailures(manifest(TOOLS, kept))).toHaveLength(1);
    expect(sourceManifestFailures(manifest(TOOLS, ""))).toHaveLength(1);
  });

  it("ignores a removal that only appears in a comment", () => {
    expect(sourceManifestFailures(manifest(TOOLS, `<!-- ${REMOVAL} -->`))).toHaveLength(1);
  });

  it("rejects a merged manifest that still declares the service", () => {
    const merged = manifest(
      TOOLS,
      `<service android:name="androidx.work.impl.background.systemjob.SystemJobService" />
    <service
        android:name="${WORK_MANAGER_FOREGROUND_SERVICE}"
        android:exported="false" />`,
    );
    expect(mergedManifestFailures(merged, "merged")).toEqual([
      `merged must not declare ${WORK_MANAGER_FOREGROUND_SERVICE}`,
    ]);
    const removed = manifest(TOOLS, '<service android:name="androidx.work.impl.background.systemjob.SystemJobService" />');
    expect(mergedManifestFailures(removed, "merged")).toEqual([]);
  });

  it("finds no merged manifests before an Android build", async () => {
    const directory = await buildDirectory();
    expect(await findMergedManifests(directory)).toEqual([]);
    expect(await workManagerForegroundServiceFailures(manifest(TOOLS, REMOVAL), directory)).toEqual([]);
  });

  it("checks every merged manifest an Android build left", async () => {
    const directory = await buildDirectory();
    const clean = path.join(directory, "intermediates", "merged_manifest", "release", "processReleaseMainManifest");
    const leaked = path.join(directory, "intermediates", "merged_manifests", "debug", "processDebugManifest");
    const unrelated = path.join(directory, "intermediates", "packaged_res", "debug");
    for (const folder of [clean, leaked, unrelated]) await mkdir(folder, { recursive: true });
    await writeFile(path.join(clean, "AndroidManifest.xml"), manifest(TOOLS, ""));
    await writeFile(
      path.join(leaked, "AndroidManifest.xml"),
      manifest(TOOLS, `<service android:name="${WORK_MANAGER_FOREGROUND_SERVICE}" />`),
    );
    await writeFile(
      path.join(unrelated, "AndroidManifest.xml"),
      manifest(TOOLS, `<service android:name="${WORK_MANAGER_FOREGROUND_SERVICE}" />`),
    );
    expect(await findMergedManifests(directory)).toEqual([
      path.join(clean, "AndroidManifest.xml"),
      path.join(leaked, "AndroidManifest.xml"),
    ]);
    const failures = await workManagerForegroundServiceFailures(manifest(TOOLS, REMOVAL), directory);
    expect(failures).toEqual([
      `merged manifest ${path.join("intermediates", "merged_manifests", "debug", "processDebugManifest", "AndroidManifest.xml")} must not declare ${WORK_MANAGER_FOREGROUND_SERVICE}`,
    ]);
  });
});
