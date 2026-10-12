import type { updates as enUpdates } from "../en/updates";
import type { MessageShape } from "../types";

export const updates = {
  checkingFeed: "Revisando el feed de versiones configurado",
  current: "Ganbaru AI está actualizado",
  versionAvailable: (version: string) => `La versión ${version} está disponible`,
  updateAvailable: "Hay una actualización disponible",
  downloadingBytes: (bytes: string) => `Descargando ${bytes}`,
  downloadingPercent: (percent: number) => `Descargando ${percent}%`,
  installedRestarting: "Actualización instalada. Reiniciando Ganbaru AI",
  checkFailed: "Falló la revisión de actualizaciones",
  notChecked: "No se ha revisado si hay actualizaciones en esta ventana",
  promptAvailable: (version: string) => `Hay una actualización disponible: ${version}`,
  downloaded: (bytes: string) => `${bytes} descargados`,
  downloadedOfTotal: (downloaded: string, total: string) => `${downloaded} de ${total}`,
  updateAndRestart: "Actualizar y reiniciar",
  copyCommand: "Copiar comando",
  commandCopied: "Copiado",
  releaseNotes: "Notas de la versión",
  dismissNotification: "Descartar notificación de actualización",
  feedNotConfigured: "Esta compilación no tiene configurado un feed de actualizaciones",
} as const satisfies MessageShape<typeof enUpdates>;
