import type { benchmark as enBenchmark } from "../en/benchmark";
import type { MessageShape } from "../types";

export const benchmark = {
  runBenchmarkTitle: "¿Ejecutar benchmark?",
  runSuiteTitle: (suite: string) => `¿Ejecutar ${suite}?`,
  confirmRun: "Ejecutar",
  suiteIntro: (count: number) =>
    `Ejecuta ${count} benchmarks en secuencia contra bases de datos aisladas`,
  singleIntro: "Reinicia la app varias veces contra una base de datos aislada",
  realCalendarUntouched: "Tu calendario real no se toca.",
  completionNotification:
    "Se muestra una notificación de escritorio cuando termina la ejecución.",
  scenariosHeading: "Escenarios:",
  runningTitle: (label: string) => `Ejecutando benchmark: ${label}`,
  runningSuiteProgress: (current: number, total: number) =>
    `Benchmark ${current}/${total}.`,
  dataset: (label: string) => `Dataset: ${label}.`,
  datasetBase: "base",
  datasetDenseFallback: "dataset denso",
  runningWarning:
    "Evita interactuar con la app: los clics y las pulsaciones pueden alterar las mediciones. Tu calendario real vive en una base de datos separada y se mantiene intacto aunque la ejecución se interrumpa, la app se cierre por fuerza o el sistema se apague. Cancelar descarta la ejecución parcial y reinicia con tus datos reales.",
  completeTitle: "Benchmark completado",
  completeDescription:
    "Revisa las tablas, luego copia el markdown para que un agente lo coloque cuidadosamente en el registro de rendimiento.",
  copied: "Copiado",
  copyMarkdown: "Copiar markdown",
  runMetadata: "Metadatos de ejecución",
  noPrimaryMetrics: "No se capturaron métricas principales.",
  failedTitle: "Benchmark falló",
  unknownError: "Error desconocido.",
  returnToData: "Volver a tus datos",
  table: {
    run: "Ejecución",
    harness: "Harness",
    anchorDate: "Fecha ancla",
    buildRef: "Ref de compilación",
    platform: "Plataforma",
    notes: "Notas",
    dataset: "Dataset",
    runs: "Ejecuciones",
    usablePaintMedianMs: "Mediana de pintura usable ms",
    launchMedianMs: "Mediana de inicio ms",
    launchP95Ms: "P95 de inicio ms",
    statistic: "Estadística",
    backendMb: "Backend MB",
    frontendMb: "Frontend MB",
    networkMb: "Red MB",
    totalMb: "Total MB",
    metric: "Métrica",
    action: "Acción",
    value: "Valor",
    valueMs: "Valor ms",
    unit: "Unidad",
    medianMs: "Mediana ms",
    p95Ms: "P95 ms",
  },
  suite: {
    core: {
      label: "Benchmarks principales",
      description: "Inicio, memoria y latencia de interacción visible.",
    },
    backend: {
      label: "Benchmarks de backend",
      description: "Latencia de importación de calendario respaldada por Rust.",
    },
    all: {
      label: "Todos los benchmarks",
      description: "Suite completa de benchmarks principales y de backend.",
    },
    fallbackPlural: "benchmarks",
  },
  scenario: {
    startupBoot: {
      label: "Arranque de inicio",
      sectionTitle: "Arranque de inicio",
      description:
        "Captura muestras repetidas de lanzamiento del proceso hasta la pintura usable del calendario sin agregar una ventana de estabilización de memoria. Úsalo para regresiones de tiempo de inicio.",
    },
    idleMemory: {
      label: "Memoria en reposo",
      sectionTitle: "Memoria en reposo",
      description:
        "Carga la semana anclada fija, no realiza interacción durante la ventana de carga y reporta memoria.",
    },
    calendarNav: {
      label: "Navegación semanal del calendario",
      sectionTitle: "Memoria de navegación sostenida del calendario",
      description:
        "Envía eventos keydown iniciales y repetidos de ArrowRight más keyup durante una pulsación de 3 segundos, usando el mismo controlador de teclado de ventana y el mismo controlador de navegación sostenida que una pulsación física de flecha derecha. Corre contra el conjunto de datos de calendario denso predeterminado.",
    },
    calendarPanelLatency: {
      label: "Latencia del panel de calendario",
      sectionTitle: "Latencia del panel de calendario",
      description:
        "Mide las dos acciones de apertura del panel de calendario con 50 ejecuciones cada una: hacer clic en eventos existentes variados y hacer clic en bloques de tiempo deterministas para crear.",
    },
    calendarImportOps: {
      label: "Operaciones de importación de calendario",
      sectionTitle: "Operaciones de importación de calendario",
      description:
        "Mide el comando Rust calendar_bulk_import para importaciones repetidas de 100 eventos y un pase de agregar/actualizar 1000 eventos.",
    },
  },
  workload: {
    calendarStartupLaunchSamples: "muestras de inicio de calendario",
    idleCalendarBaseline: "línea base de calendario en reposo",
    heldRightArrowWeekViewNavigation:
      "navegación sostenida con flecha derecha en vista semanal",
    scriptedCalendarPanelOpenActions:
      "acciones programadas de apertura del panel de calendario",
    scriptedCalendarBulkImportCommands:
      "comandos programados de importación masiva de calendario",
  },
  step: {
    settingUp: "Configurando",
    memoryObservation: "Observación de memoria",
    preparingMemoryObservation: "Preparando observación de memoria",
    restartingToSeedDenseDataset: "Reiniciando para sembrar dataset denso",
    restartingForBaselineDataset: "Reiniciando para dataset base",
    restartingForDenseDataset: "Reiniciando para dataset denso",
    restartingForNextBenchmark: "Reiniciando para el siguiente benchmark",
    restartingForNextDenseDataset: "Reiniciando para el siguiente dataset denso",
    startupCooldown: (seconds: number) =>
      `Cerrando durante ${seconds} s para el enfriamiento de inicio`,
    baselineCooldown: (seconds: number, current: number, total: number) =>
      `Cerrando durante ${seconds} s antes del inicio base ${current}/${total}`,
    denseCooldown: (seconds: number, current: number, total: number) =>
      `Cerrando durante ${seconds} s antes del inicio denso ${current}/${total}`,
    launchSample: (current: number, total: number) =>
      `Muestra de inicio ${current}/${total}`,
    seeding: (dataset: string) => `Sembrando ${dataset}`,
    timedWorkload: (label: string, seconds: number) => `${label}: ${seconds} s`,
  },
  notification: {
    suiteCompleteTitle: "Suite de benchmarks completada",
    completeTitle: "Benchmark completado",
    failedTitle: "Benchmark falló",
    suiteCompleteBody: (count: number) =>
      `${count} benchmarks terminaron. Abre la app para revisar la salida del benchmark.`,
    startupCompleteBody: (label: string, baseMs: string, denseMs: string) =>
      `${label}: medianas de inicio base ${baseMs}, denso mayor ${denseMs}. Abre la app para revisar la salida del benchmark.`,
    memoryCompleteBody: (label: string, mb: string) =>
      `${label}: máximo observado ${mb}. Abre la app para revisar la salida del benchmark.`,
    metricsCompleteBody: (label: string, count: number) =>
      `${label}: ${count} filas de métricas. Abre la app para revisar la salida del benchmark.`,
  },
} as const satisfies MessageShape<typeof enBenchmark>;
