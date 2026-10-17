import { describe, expect, it } from "vitest";
import { categorizeMemoryProcessName, memoryDisplayRows } from "./memory-report";
import type { MemoryReport } from "./memory-report";

describe("categorizeMemoryProcessName", () => {
  it("maps backend and network processes to their own rows", () => {
    expect(categorizeMemoryProcessName("Backend")).toBe("Backend");
    expect(categorizeMemoryProcessName("WebKitNetworkProcess")).toBe("Network");
  });

  it("maps frontend and unknown child processes to the frontend row", () => {
    expect(categorizeMemoryProcessName("Frontend")).toBe("Frontend");
    expect(categorizeMemoryProcessName("WebView2 #1")).toBe("Frontend");
    expect(categorizeMemoryProcessName("Renderer")).toBe("Frontend");
  });
});

describe("memoryDisplayRows", () => {
  it("returns stable placeholder rows while the first report is pending", () => {
    expect(memoryDisplayRows(null)).toEqual([
      { label: "Backend", mb: null },
      { label: "Frontend", mb: null },
      { label: "Network", mb: null },
      { label: "Total", mb: null },
    ]);
  });

  it("aggregates processes into stable display rows", () => {
    const report: MemoryReport = {
      platform: "Linux",
      total_mb: 42,
      metric: {
        name: "PSS",
        slug: "pss",
        description: "Proportional Set Size",
      },
      processes: [
        { name: "Backend", mb: 10 },
        { name: "Frontend", mb: 20 },
        { name: "WebView2 #1", mb: 5 },
        { name: "Network", mb: 7 },
      ],
    };

    expect(memoryDisplayRows(report)).toEqual([
      { label: "Backend", mb: 10 },
      { label: "Frontend", mb: 25 },
      { label: "Network", mb: 7 },
      { label: "Total", mb: 42 },
    ]);
  });
});
