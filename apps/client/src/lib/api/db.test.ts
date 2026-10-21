import { beforeEach, describe, expect, it, vi } from "vitest";
import { DENSE_DATASET_VERSION, HARNESS_VERSION } from "$lib/benchmark/types";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

beforeEach(() => {
  vi.resetModules();
  invokeMock.mockReset();
});

function pendingState(overrides: Record<string, unknown> = {}): string {
  const now = new Date().toISOString();
  return JSON.stringify({
    vaultMode: "benchmark",
    stage: "phase-a-pending",
    harnessVersion: HARNESS_VERSION,
    datasetVersion: DENSE_DATASET_VERSION,
    startedAt: now,
    updatedAt: now,
    ...overrides,
  });
}

function respond(state: string | null, selected: string): void {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "read_benchmark_state") return state;
    if (command === "select_database_target") return selected;
    throw new Error(`unexpected command ${command}`);
  });
}

describe("ensureDbUrl", () => {
  it("requests the benchmark database for a fresh pending relaunch", async () => {
    respond(pendingState(), "sqlite:benchmark.sqlite");
    const { ensureDbUrl, dbUrl } = await import("./db");
    await expect(ensureDbUrl()).resolves.toBe("sqlite:benchmark.sqlite");
    expect(dbUrl()).toBe("sqlite:benchmark.sqlite");
    expect(invokeMock).toHaveBeenCalledWith("select_database_target", { benchmark: true });
  });

  it("requests the vault when the benchmark state is absent, stale, running, or from another harness", async () => {
    const stale = new Date(Date.now() - 10 * 60 * 1000).toISOString();
    for (const state of [
      null,
      "not json",
      pendingState({ stage: "phase-a-running" }),
      pendingState({ vaultMode: "user" }),
      pendingState({ harnessVersion: `${HARNESS_VERSION}-other` }),
      pendingState({ updatedAt: stale }),
    ]) {
      vi.resetModules();
      invokeMock.mockReset();
      respond(state, "sqlite:ganbaru.sqlite");
      const { ensureDbUrl } = await import("./db");
      await expect(ensureDbUrl()).resolves.toBe("sqlite:ganbaru.sqlite");
      expect(invokeMock).toHaveBeenCalledWith("select_database_target", { benchmark: false });
    }
  });

  it("uses the database Rust selected even when it differs from the request", async () => {
    respond(null, "sqlite:benchmark.sqlite");
    const { ensureDbUrl } = await import("./db");
    await expect(ensureDbUrl()).resolves.toBe("sqlite:benchmark.sqlite");
  });

  it("rejects a database URL Rust never selects", async () => {
    respond(null, "sqlite:other.sqlite");
    const { ensureDbUrl, dbUrl } = await import("./db");
    await expect(ensureDbUrl()).rejects.toThrow("Unexpected database target");
    expect(() => dbUrl()).toThrow("dbUrl() called before ensureDbUrl()");
  });
});
