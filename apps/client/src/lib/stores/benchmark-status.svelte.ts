export type BenchmarkStatus = "idle" | "confirming" | "running" | "summary" | "error";

class BenchmarkStatusStore {
  status = $state<BenchmarkStatus>("idle");

  setStatus(status: BenchmarkStatus): void {
    this.status = status;
  }
}

let store: BenchmarkStatusStore | null = null;

export function getBenchmarkStatus(): BenchmarkStatusStore {
  if (!store) store = new BenchmarkStatusStore();
  return store;
}
