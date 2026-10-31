import type { theme as enTheme } from "../en/theme";
import type { MessageShape } from "../types";

export const theme = {
  builtInName: {
    light: "Claro predeterminado",
    dark: "Oscuro predeterminado",
  },
  sync: {
    conflicts: (count: number) =>
      count === 1
        ? "1 tema se editó en dos dispositivos. Ábrelo en los ajustes de temas para elegir qué conservar."
        : `${count} temas se editaron en dos dispositivos. Ábrelos en los ajustes de temas para elegir qué conservar.`,
    untitled: "Tema sin título",
    restored: "Tema restaurado",
    restoreFailed: "No se pudo restaurar el tema",
  },
  conflict: {
    badge: "Conflicto",
    title: "Este tema se editó en dos dispositivos",
    hint: "Elige la versión que quieres conservar. Para conservar ambas, duplica este tema primero.",
    loading: "Cargando versiones",
    loadFailed: "No se pudieron cargar las versiones",
    palette: "Paleta de eventos",
    shown: "Visible ahora",
    keepThis: "Conservar esta",
    useOther: "Usar esta",
    versionFrom: (device: string, time: string) => `${device}, ${time}`,
    thisDevice: "Este dispositivo",
    otherDevice: "Otro dispositivo",
    resolved: "Este conflicto ya se resolvió",
    failed: "No se pudo resolver el conflicto",
  },
} as const satisfies MessageShape<typeof enTheme>;
