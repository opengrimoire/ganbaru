import type { diagnostics as enDiagnostics } from "../en/diagnostics";
import type { MessageShape } from "../types";

export const diagnostics = {
  liveRam: "RAM en vivo",
  startupRam: "RAM de inicio (10 s)",
  startupRamSnapshot: "Captura de RAM de inicio (10 s)",
  pin: "Fijar",
  unpin: "Desfijar",
  devModeUsesMoreResources: "El modo de desarrollo usa más recursos.",
  startupSnapshotPending: "Captura de inicio pendiente...",
  startupSnapshotUnavailable: "Captura de inicio no disponible.",
  copied: "Copiado",
  copyLiveRam: "Copiar RAM en vivo",
  copyChart: "Copiar gráfica",
  copyStartupRam: "Copiar RAM de inicio",
  launchTime: "Tiempo de inicio",
  noBootMarksCaptured: "No se capturaron marcas de arranque.",
  copyLaunchTable: "Copiar tabla de inicio",
  speedLogHeading: (count: number) => `Registro de velocidad (${count})`,
  speedLogCopyHeading: (count: number) =>
    `Registro de velocidad (${count} acciones):`,
  trackOn: "activado",
  trackOff: "desactivado",
  trackState: (state: string) => `Registrar: ${state}`,
  stopTrackingTitle:
    "Dejar de registrar acciones de navegación, vistas y panel de eventos",
  startTrackingTitle:
    "Registrar acciones de navegación, vistas y panel de eventos",
  clear: "Limpiar",
  copySpeedLog: "Copiar registro de velocidad",
  benchmarks: "Benchmarks",
  restartsApp: "reinicia la app",
  runSuite: (label: string) => `Ejecutar ${label.toLowerCase()}`,
  showSuite: (label: string) => `Mostrar ${label}`,
  hideSuite: (label: string) => `Ocultar ${label}`,
  metric: "Métrica",
  ramByProcess: "RAM por proceso:",
  memory: {
    backend: "Backend",
    frontend: "Frontend",
    network: "Red",
    total: "Total",
    totalMetric: (name: string) => `Total ${name}`,
    unitMb: "MB",
    waiting: "Esperando el reporte de memoria.",
    linuxPss: (fallback: string) =>
      `En Linux, 'Total' usa PSS para Ganbaru y WebKit.${fallback} PSS reparte la RAM compartida de forma justa, así que se acerca más al costo real de la app. Para comparar con otra app, mide también el PSS de esa app. Si usas RSS en System Monitor, compara RSS con RSS, pero eso es menos preciso.`,
    linuxRssFallback: " Algunos procesos usaron RSS porque PSS no estaba disponible.",
    windowsWorkingSet:
      "En Windows, 'Total' usa Working Set para Ganbaru y WebView2. Working Set incluye RAM privada y compartida actualmente en memoria. Memory en Task Manager muestra el working set privado, así que deja fuera la RAM compartida. Para comparar con otra app, mide también el Working Set de esa app. Si usas Memory en Task Manager, compara Task Manager con Task Manager, pero eso es menos preciso.",
    macUnavailable:
      "En macOS, 'Total' debería ser la suma de la huella física de Ganbaru y los procesos WebView. Aún no está implementado.",
    metricDescription: (name: string, description: string) => `${name}: ${description}`,
  },
} as const satisfies MessageShape<typeof enDiagnostics>;
