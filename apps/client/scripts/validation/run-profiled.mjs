#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Runs one concurrency-sensitive validation stage with the limits of the selected profile.
 *
 * `GANBARU_VALIDATE_PROFILE` selects `standard` (the default, also used by CI) or `low-memory`
 * for machines that cannot spare about 5 GB for a gate while other applications run.
 *
 * Usage: node run-profiled.mjs <clippy | rust-tests | frontend-tests>
 */

const PROFILE_VARIABLE = "GANBARU_VALIDATE_PROFILE";
const BENCHMARK_TESTS = "src/lib/benchmark/**/*.test.ts";

/**
 * @typedef {object} ValidationProfile
 * @property {number} cargoJobs Parallel Cargo build jobs.
 * @property {number} rustTestThreads Runtime threads per Rust test binary.
 * @property {number} vitestShards Sequential Vitest shards; each releases its module graph before the next.
 * @property {number} vitestWorkers Vitest workers inside each shard.
 */

/** @type {Record<string, ValidationProfile>} */
const PROFILES = {
  standard: { cargoJobs: 4, rustTestThreads: 4, vitestShards: 2, vitestWorkers: 2 },
  "low-memory": { cargoJobs: 1, rustTestThreads: 2, vitestShards: 4, vitestWorkers: 1 },
};

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");

/**
 * Resolve the profile named by the environment, rejecting unknown names instead of guessing.
 * @returns {ValidationProfile}
 */
function selectedProfile() {
  const name = process.env[PROFILE_VARIABLE]?.trim() || "standard";
  const profile = PROFILES[name];
  if (!profile) {
    throw new Error(`${PROFILE_VARIABLE}=${name} is not a validation profile; use ${Object.keys(PROFILES).join(" or ")}.`);
  }
  return profile;
}

/**
 * Run a command from the repository root with inherited output, exiting with its status on failure.
 * @param {string} command
 * @param {string[]} args
 */
function run(command, args) {
  const result = spawnSync(command, args, { cwd: repoRoot, stdio: "inherit" });
  if (result.error) throw new Error(`Could not start ${command}: ${result.error.message}`);
  if (result.status !== 0) process.exit(result.status ?? 1);
}

/** Turbo's launcher is a Node script, so running it through Node works on every platform without a shell. */
function turboEntry() {
  return createRequire(import.meta.url).resolve("turbo/bin/turbo");
}

/** @type {Record<string, (profile: ValidationProfile) => void>} */
const STAGES = {
  clippy: (profile) => run("cargo", ["clippy", "--workspace", "-j", String(profile.cargoJobs), "--", "-D", "warnings"]),
  "rust-tests": (profile) => run("cargo", ["test", "--workspace", "-j", String(profile.cargoJobs), "--", `--test-threads=${profile.rustTestThreads}`]),
  "frontend-tests": (profile) => {
    const turbo = turboEntry();
    for (let shard = 1; shard <= profile.vitestShards; shard += 1) {
      run(process.execPath, [turbo, "test", "--concurrency=1", "--", `--maxWorkers=${profile.vitestWorkers}`, `--exclude=${BENCHMARK_TESTS}`, `--shard=${shard}/${profile.vitestShards}`]);
    }
  },
};

const stageName = process.argv[2];
const stage = stageName ? STAGES[stageName] : undefined;
if (!stage) {
  console.error(`Usage: node run-profiled.mjs <${Object.keys(STAGES).join(" | ")}>`);
  process.exit(2);
}
try {
  stage(selectedProfile());
} catch (caught) {
  console.error(caught instanceof Error ? caught.message : String(caught));
  process.exit(1);
}
