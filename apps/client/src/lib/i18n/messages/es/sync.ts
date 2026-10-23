import type { sync as enSync } from "../en/sync";
import type { MessageShape } from "../types";

export const sync = {
  heading: "Sincronización",
  description: (features: string) =>
    `Se sincroniza entre tus dispositivos vinculados cuando ambos están en la misma red: ${features}.`,
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
    `${count} ${count === 1 ? "cambio necesita" : "cambios necesitan"} una versión más reciente de Ganbaru`,
  heldNewerManifest: (count: number) =>
    `${count} ${count === 1 ? "cambio viene" : "cambios vienen"} de una estructura de datos más reciente. Actualiza Ganbaru en este dispositivo.`,
  heldInvalid: (count: number) =>
    `${count} ${count === 1 ? "cambio se rechazó por no ser válido" : "cambios se rechazaron por no ser válidos"}`,
  conflicts: (count: number) =>
    `${count} ${count === 1 ? "elemento se editó" : "elementos se editaron"} en dos dispositivos. ${count === 1 ? "Ábrelo" : "Ábrelos"} para elegir qué conservar.`,
  lockUnsupported: "La unidad de esta carpeta no puede saber si otra app de Ganbaru la tiene abierta. No la abras desde dos apps a la vez o tus datos pueden dañarse.",
  syncNow: "Sincronizar ahora",
  syncNowFailed: "No se pudo iniciar la sincronización",
  pause: "Pausar sincronización",
  pauseFailed: "No se pudo cambiar el ajuste de sincronización",
  errorDetail: (message: string) => `Último error: ${message}`,
  writerLimit: "Este dispositivo creó más identidades de sincronización de las permitidas, así que el coordinador rechaza sus cambios nuevos. Se quedan en este dispositivo.",
  clockAhead: (device: string) =>
    `El reloj de ${device} va adelantado respecto a este dispositivo. Sus ediciones ganan los cambios en conflicto hasta que los relojes coincidan, así que revisa su fecha y hora.`,
  clockAheadOwn: "Algunas ediciones de este dispositivo se hicieron con su reloj adelantado. Ganan los cambios en conflicto hasta que pase esa hora.",
  bootstrapRequired: "Este dispositivo estuvo fuera demasiado tiempo para ponerse al día. Tus cambios aquí siguen llegando a tu dispositivo principal, pero para volver a recibir cambios, vuelve a vincular este dispositivo o transfiérele la bóveda.",
  uncounted: {
    dormant: (device: string) =>
      `La sincronización ya no espera a ${device}, que lleva mucho tiempo sin sincronizarse. Cuando vuelva, vincúlalo de nuevo o transfiérele la bóveda.`,
    relinkRequired: (device: string) => `Vuelve a vincular ${device} para que pueda sincronizarse.`,
  },
  connection: {
    lan: (address: string) => `Conectado por la red local en ${address}`,
    overlay: (address: string) => `Conectado por la red superpuesta en ${address}`,
  },
  degraded: {
    backgroundRestricted: "Android restringe Ganbaru en segundo plano, así que la sincronización espera hasta que abras la app.",
    standbyRestricted: "Android rara vez deja que Ganbaru funcione en segundo plano, así que la sincronización puede retrasarse.",
    localNetworkDenied: "Este dispositivo no tiene permiso para llegar a tus otros dispositivos en la red local. Permite el acceso a la red local para Ganbaru en los ajustes del sistema.",
    firewallHub: "El cortafuegos impide que tus otros dispositivos lleguen a este.",
    firewallClient: "No se encontró tu dispositivo principal en la red. Puede que un cortafuegos de este dispositivo lo esté bloqueando.",
    openBatterySettings: "Abrir ajustes de batería",
    allowNetworkAccess: "Permitir acceso a la red",
  },
  recovery: {
    heading: "Eliminado recientemente con ediciones sin guardar",
    description: "Esto se eliminó en un dispositivo mientras otro aún lo editaba.",
    restore: "Restaurar",
    discard: "Descartar",
    restored: "Restaurado",
    discarded: "Ediciones descartadas",
    failed: "No se pudo restaurar",
    discardFailed: "No se pudieron descartar las ediciones",
    loadFailed: "No se pudieron cargar los elementos eliminados",
    untitled: "Sin título",
    descendants: (count: number) =>
      count === 1
        ? "Al restaurarlo también se recupera 1 elemento eliminado con él"
        : `Al restaurarlo también se recuperan ${count} elementos eliminados con él`,
    editedBy: (device: string, time: string) => `Se editó en ${device}, ${time}`,
    thisDevice: "este dispositivo",
    otherDevice: "otro dispositivo",
  },
} as const satisfies MessageShape<typeof enSync>;
