export interface ProcessMemory {
  name: string;
  mb: number;
}

export interface MemoryReport {
  processes: ProcessMemory[];
  total_mb: number;
  platform: string;
  metric: MemoryMetric;
}

export interface MemoryMetric {
  name: string;
  slug: string;
  description: string;
}

export type MemoryProcessCategory = "Backend" | "Frontend" | "Network";
export type MemoryDisplayLabel = MemoryProcessCategory | "Total";

export interface MemoryDisplayRow {
  label: MemoryDisplayLabel;
  mb: number | null;
}

export type StartupMemorySnapshot =
  | { status: "pending" }
  | { status: "ready"; report: MemoryReport }
  | { status: "failed"; message: string };

export const MEMORY_PROCESS_CATEGORIES: readonly MemoryProcessCategory[] = [
  "Backend",
  "Frontend",
  "Network",
];

export function categorizeMemoryProcessName(name: string): MemoryProcessCategory {
  const normalized = name.toLowerCase();
  if (normalized.includes("backend")) return "Backend";
  if (normalized.includes("network")) return "Network";
  return "Frontend";
}

export function memoryDisplayRows(report: MemoryReport | null): MemoryDisplayRow[] {
  if (!report) {
    return [
      ...MEMORY_PROCESS_CATEGORIES.map((label) => ({ label, mb: null })),
      { label: "Total", mb: null },
    ];
  }

  const byCategory: Record<MemoryProcessCategory, number> = {
    Backend: 0,
    Frontend: 0,
    Network: 0,
  };
  for (const processMemory of report.processes) {
    byCategory[categorizeMemoryProcessName(processMemory.name)] += processMemory.mb;
  }

  return [
    ...MEMORY_PROCESS_CATEGORIES.map((label) => ({ label, mb: byCategory[label] })),
    { label: "Total", mb: report.total_mb },
  ];
}
