import { invoke } from "@tauri-apps/api/core";
import {
  HARNESS_VERSION,
  DENSE_DATASET_VERSION,
  isBenchmarkPendingStage,
  isFreshBenchmarkPendingAge,
  isFreshBenchmarkTotalAge,
} from "$lib/benchmark/types";

/**
 * Connection string for the active database. Resolved exactly once on the
 * first call to `ensureDbUrl()` and cached for the rest of the process lifetime.
 *
 * The resolution checks the persisted benchmark state file: only fresh
 * `*-pending` states route to the isolated `benchmark.sqlite`.
 * Interrupted `*-running` states fall back to the user DB and are cleaned
 * up by the benchmark runner after boot.
 */
let cachedUrl: string | null = null;

const USER_DB_URL = "sqlite:ganbaru.sqlite";
const BENCHMARK_DB_URL = "sqlite:benchmark.sqlite";

/** Minimal shape needed for the URL decision. The full state lives in `benchmark/types.ts`. */
interface VaultModeProbe {
  vaultMode?: "user" | "benchmark";
  stage?: string;
  harnessVersion?: string;
  datasetVersion?: string;
  startedAt?: string;
  updatedAt?: string;
}

async function wantsBenchmarkDatabase(): Promise<boolean> {
  try {
    const json = await invoke<string | null>("read_benchmark_state");
    if (json) {
      const parsed = JSON.parse(json) as VaultModeProbe;
      const validVersion = parsed?.harnessVersion === HARNESS_VERSION
        && parsed?.datasetVersion === DENSE_DATASET_VERSION;
      const fresh = isFreshBenchmarkTotalAge(parsed) && isFreshBenchmarkPendingAge(parsed);
      const benchmarkPending = isBenchmarkPendingStage(parsed?.stage);
      return parsed?.vaultMode === "benchmark" && benchmarkPending && validVersion && fresh;
    }
  } catch (e) {
    console.error("dbUrl: failed to probe benchmark state, falling back to user DB", e);
  }
  return false;
}

/**
 * Asks Rust to select this process's database. Rust fixes the first selection for the
 * process lifetime, so every window receives the same URL and commands naming any other
 * database fail.
 */
async function resolveUrl(): Promise<string> {
  const url = await invoke<string>("select_database_target", {
    benchmark: await wantsBenchmarkDatabase(),
  });
  if (url !== USER_DB_URL && url !== BENCHMARK_DB_URL) {
    throw new Error(`Unexpected database target ${url}`);
  }
  return url;
}

/**
 * Returns the resolved DB URL. Throws if called before `ensureDbUrl()` has
 * been awaited at least once. Boot paths call `ensureDbUrl()` before any
 * command that needs a synchronous URL, so the throw is a guard against
 * accidental misuse.
 */
export function dbUrl(): string {
  if (cachedUrl === null) {
    throw new Error("dbUrl() called before ensureDbUrl()");
  }
  return cachedUrl;
}

let dbPromise: Promise<string> | null = null;

export async function ensureDbUrl(): Promise<string> {
  if (!dbPromise) {
    dbPromise = (async () => {
      const url = await resolveUrl();
      cachedUrl = url;
      return url;
    })();
    dbPromise.catch(() => {
      dbPromise = null;
      cachedUrl = null;
    });
  }
  return dbPromise;
}
