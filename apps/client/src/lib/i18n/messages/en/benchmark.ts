export const benchmark = {
  runBenchmarkTitle: "Run benchmark?",
  runSuiteTitle: (suite: string) => `Run ${suite}?`,
  confirmRun: "Run",
  suiteIntro: (count: number) =>
    `Runs ${count} benchmarks sequentially against isolated databases`,
  singleIntro: "Restarts the app a few times against an isolated database",
  realCalendarUntouched: "Your real calendar is not touched.",
  completionNotification: "A desktop notification fires when the run finishes.",
  scenariosHeading: "Scenarios:",
  runningTitle: (label: string) => `Running benchmark: ${label}`,
  runningSuiteProgress: (current: number, total: number) =>
    `Benchmark ${current}/${total}.`,
  dataset: (label: string) => `Dataset: ${label}.`,
  datasetBase: "base",
  datasetDenseFallback: "dense dataset",
  runningWarning:
    "Avoid interacting with the app: clicks and key presses can skew the measurements. Your real calendar lives on a separate database and stays untouched even if the run is interrupted, the app is force-closed, or the system shuts down. Cancel discards the partial run and restarts on your real data.",
  completeTitle: "Benchmark complete",
  completeDescription:
    "Review the tables, then copy markdown for an agent to place carefully in the performance record.",
  copied: "Copied",
  copyMarkdown: "Copy markdown",
  runMetadata: "Run metadata",
  noPrimaryMetrics: "No primary metrics were captured.",
  failedTitle: "Benchmark failed",
  unknownError: "Unknown error.",
  returnToData: "Return to your data",
  table: {
    run: "Run",
    harness: "Harness",
    anchorDate: "Anchor date",
    buildRef: "Build ref",
    platform: "Platform",
    notes: "Notes",
    dataset: "Dataset",
    runs: "Runs",
    usablePaintMedianMs: "Usable paint median ms",
    launchMedianMs: "Launch median ms",
    launchP95Ms: "Launch P95 ms",
    statistic: "Statistic",
    backendMb: "Backend MB",
    frontendMb: "Frontend MB",
    networkMb: "Network MB",
    totalMb: "Total MB",
    metric: "Metric",
    action: "Action",
    value: "Value",
    valueMs: "Value ms",
    unit: "Unit",
    medianMs: "Median ms",
    p95Ms: "P95 ms",
  },
  suite: {
    core: {
      label: "Core benchmarks",
      description: "Startup, memory, and user-visible interaction latency.",
    },
    backend: {
      label: "Backend benchmarks",
      description: "Rust-backed calendar import latency.",
    },
    all: {
      label: "All benchmarks",
      description: "Complete core and backend benchmark suite.",
    },
    fallbackPlural: "benchmarks",
  },
  scenario: {
    startupBoot: {
      label: "Startup boot",
      sectionTitle: "Startup boot",
      description:
        "Captures repeated process launch samples to usable calendar paint without adding a memory settling window. Use this for startup-time regressions.",
    },
    idleMemory: {
      label: "Idle memory",
      sectionTitle: "Idle memory",
      description:
        "Loads the fixed anchored week, performs no interaction for the workload window, and reports memory.",
    },
    calendarNav: {
      label: "Calendar week-view nav",
      sectionTitle: "Calendar held navigation memory",
      description:
        "Dispatches initial and repeated ArrowRight keydown events plus keyup for a 3-second hold, using the same window keyboard handler and held-navigation controller as a physical right-arrow hold. It runs against the default dense calendar dataset.",
    },
    calendarPanelLatency: {
      label: "Calendar panel latency",
      sectionTitle: "Calendar panel latency",
      description:
        "Measures the two calendar panel open actions with 50 runs each: clicking varied existing events and clicking deterministic time slots for create.",
    },
    calendarImportOps: {
      label: "Calendar import operations",
      sectionTitle: "Calendar import operations",
      description:
        "Measures the Rust calendar_bulk_import command for repeated 100-event imports and one 1000-event add/update pass.",
    },
  },
  workload: {
    calendarStartupLaunchSamples: "calendar startup launch samples",
    idleCalendarBaseline: "idle calendar baseline",
    heldRightArrowWeekViewNavigation: "held right-arrow week-view navigation",
    scriptedCalendarPanelOpenActions: "scripted calendar panel open actions",
    scriptedCalendarBulkImportCommands: "scripted calendar bulk import commands",
  },
  step: {
    settingUp: "Setting up",
    memoryObservation: "Memory observation",
    preparingMemoryObservation: "Preparing memory observation",
    restartingToSeedDenseDataset: "Restarting to seed dense dataset",
    restartingForBaselineDataset: "Restarting for baseline dataset",
    restartingForDenseDataset: "Restarting for dense dataset",
    restartingForNextBenchmark: "Restarting for next benchmark",
    restartingForNextDenseDataset: "Restarting for next dense dataset",
    startupCooldown: (seconds: number) =>
      `Closing for ${seconds} s startup cooldown`,
    baselineCooldown: (seconds: number, current: number, total: number) =>
      `Closing for ${seconds} s cooldown before baseline launch ${current}/${total}`,
    denseCooldown: (seconds: number, current: number, total: number) =>
      `Closing for ${seconds} s cooldown before dense launch ${current}/${total}`,
    launchSample: (current: number, total: number) =>
      `Launch sample ${current}/${total}`,
    seeding: (dataset: string) => `Seeding ${dataset}`,
    timedWorkload: (label: string, seconds: number) => `${label}: ${seconds} s`,
  },
  notification: {
    suiteCompleteTitle: "Benchmark suite complete",
    completeTitle: "Benchmark complete",
    failedTitle: "Benchmark failed",
    suiteCompleteBody: (count: number) =>
      `${count} benchmarks finished. Open the app to review the benchmark output.`,
    startupCompleteBody: (label: string, baseMs: string, denseMs: string) =>
      `${label}: launch medians base ${baseMs}, largest dense ${denseMs}. Open the app to review the benchmark output.`,
    memoryCompleteBody: (label: string, mb: string) =>
      `${label}: max observed ${mb}. Open the app to review the benchmark output.`,
    metricsCompleteBody: (label: string, count: number) =>
      `${label}: ${count} metric rows. Open the app to review the benchmark output.`,
  },
} as const;
