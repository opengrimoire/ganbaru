import type { sync as enSync } from "../en/sync";
import type { MessageShape } from "../types";

export const sync = {
  heading: "Sincronización",
  description: "Las notas rápidas se sincronizan entre tus dispositivos vinculados cuando ambos están en la misma red.",
  state: {
    off: "Desactivada",
    idle: "Al día",
    syncing: "Sincronizando...",
    offline: "Esperando a tu otro dispositivo",
    paused: "En pausa",
    waiting_for_identity: "Esperando a que tu otro dispositivo confirme el vínculo",
    error: "La sincronización se detuvo por un error",
  },
  role: {
    hub: "Tus otros dispositivos se sincronizan a través de este",
    client: "Este dispositivo se sincroniza a través de tu dispositivo principal",
  },
  lastExchange: (time: string) => `Última sincronización: ${time}`,
  neverExchanged: "Aún no se ha sincronizado",
  pending: (count: number) =>
    `${count} ${count === 1 ? "cambio pendiente" : "cambios pendientes"} de sincronizar`,
  waiting: (count: number) =>
    `${count} ${count === 1 ? "cambio recibido pendiente" : "cambios recibidos pendientes"} de aplicar`,
  heldHeading: "Cambios que este dispositivo aún no puede usar",
  heldNewerFormat: (count: number) =>
    `${count} ${count === 1 ? "cambio necesita" : "cambios necesitan"} una versión más reciente de Ganbaru AI`,
  heldNewerManifest: (count: number) =>
    `${count} ${count === 1 ? "cambio viene" : "cambios vienen"} de una estructura de datos más reciente. Actualiza Ganbaru AI en este dispositivo.`,
  heldInvalid: (count: number) =>
    `${count} ${count === 1 ? "cambio se rechazó por no ser válido" : "cambios se rechazaron por no ser válidos"}`,
  conflicts: (count: number) =>
    `${count} ${count === 1 ? "nota se editó" : "notas se editaron"} en dos dispositivos. ${count === 1 ? "Ábrela" : "Ábrelas"} en Notas rápidas para elegir qué conservar.`,
  syncNow: "Sincronizar ahora",
  syncNowFailed: "No se pudo iniciar la sincronización",
  pause: "Pausar sincronización",
  pauseFailed: "No se pudo cambiar el ajuste de sincronización",
  errorDetail: (message: string) => `Último error: ${message}`,
  recovery: {
    heading: "Eliminadas recientemente con ediciones sin guardar",
    description: "Estas notas se eliminaron en un dispositivo mientras otro aún las editaba.",
    restore: "Restaurar",
    discard: "Descartar",
    restored: "Nota restaurada",
    discarded: "Ediciones descartadas",
    failed: "No se pudo restaurar la nota",
    discardFailed: "No se pudieron descartar las ediciones",
    loadFailed: "No se pudieron cargar las notas eliminadas",
    untitled: "Nota sin título",
    editedBy: (device: string, time: string) => `Editada en ${device}, ${time}`,
    thisDevice: "este dispositivo",
    otherDevice: "otro dispositivo",
  },
} as const satisfies MessageShape<typeof enSync>;
