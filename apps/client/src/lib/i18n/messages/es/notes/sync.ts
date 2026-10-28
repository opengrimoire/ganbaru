import type { syncMessages as enSyncMessages } from "../../en/notes/sync";
import type { MessageShape } from "../../types";

export const syncMessages = {
  sync: {
    pageConflicts: (count: number) =>
      count === 1
        ? "1 título de página se editó en dos dispositivos. Abre la página para elegir qué conservar."
        : `${count} títulos de página se editaron en dos dispositivos. Abre las páginas para elegir qué conservar.`,
    blockConflicts: (count: number) =>
      count === 1
        ? "1 bloque se editó en dos dispositivos. Abre su página para elegir qué conservar."
        : `${count} bloques se editaron en dos dispositivos. Abre sus páginas para elegir qué conservar.`,
    propertyConflicts: (count: number) =>
      count === 1
        ? "1 propiedad de base de datos cambió en dos dispositivos. Se conserva la versión que se muestra ahora."
        : `${count} propiedades de base de datos cambiaron en dos dispositivos. Se conserva la versión que se muestra ahora.`,
    valueConflicts: (count: number) =>
      count === 1
        ? "1 valor de propiedad se editó en dos dispositivos. Abre su página para elegir qué conservar."
        : `${count} valores de propiedad se editaron en dos dispositivos. Abre sus páginas para elegir qué conservar.`,
    templateConflicts: (count: number) =>
      count === 1
        ? "1 plantilla se editó en dos dispositivos. Se conserva la versión que se muestra ahora."
        : `${count} plantillas se editaron en dos dispositivos. Se conserva la versión que se muestra ahora.`,
    threadConflicts: (count: number) =>
      count === 1
        ? "1 conversación se resolvió y se reabrió en dos dispositivos. Abre su página para elegir qué conservar."
        : `${count} conversaciones se resolvieron y se reabrieron en dos dispositivos. Abre sus páginas para elegir qué conservar.`,
    commentConflicts: (count: number) =>
      count === 1
        ? "1 comentario se editó en dos dispositivos. Abre su página para elegir qué conservar."
        : `${count} comentarios se editaron en dos dispositivos. Abre sus páginas para elegir qué conservar.`,
    pageUntitled: "Página sin título",
    blockUntitled: "Bloque sin título",
    commentUntitled: "Comentario",
    recoveryNote: "El contenido de Notas eliminado en otro dispositivo se puede descartar aquí. Todavía no se puede restaurar.",
    commentRecoveryNote: "Este comentario se eliminó con el bloque al que pertenecía y aparece bajo ese bloque.",
  },
  conflict: {
    titleTitle: "El título de esta página se editó en dos dispositivos",
    blockTitle: "Este bloque se editó en dos dispositivos",
    valueTitle: (property: string) => `${property} se editó en dos dispositivos`,
    threadTitle: "Esta conversación se resolvió y se reabrió en dos dispositivos",
    commentTitle: "Este comentario se editó en dos dispositivos",
    hint: "Elige la versión que quieres conservar.",
    keepBothHint: "Conservar ambas mantiene la versión que se muestra ahora y añade la otra después.",
    empty: "Vacío",
    shown: "Se muestra ahora",
    keepThis: "Conservar esta",
    useOther: "Usar esta",
    keepBoth: "Conservar ambas",
    versionFrom: (device: string, time: string) => `${device}, ${time}`,
    thisDevice: "Este dispositivo",
    otherDevice: "Otro dispositivo",
    resolved: "Este conflicto ya se resolvió",
    failed: "No se pudo resolver el conflicto",
    threadOpen: "Abierta",
    threadResolved: "Resuelta",
    unnamedProperty: "Una propiedad",
    editTitle: "Tu edición de este bloque choca con un cambio de otro dispositivo",
    editHint: "Tu texto se queda en el editor hasta que elijas.",
    editRemovedTitle: "Este bloque se eliminó en otro dispositivo mientras lo editabas",
    mine: "Tu texto",
    theirs: "Texto de otro dispositivo",
    useMine: "Usar el mío",
    useTheirs: "Usar el otro",
    restoreMine: "Restaurar con mi texto",
    discardMine: "Descartar mi texto",
  },
  mismatch: {
    label: (type: string) => `Guardado como ${type}`,
    hint: "El tipo de la propiedad cambió en otro dispositivo. Convertir conserva este valor en el nuevo tipo; Descartar lo borra.",
    convert: "Convertir",
    discard: "Descartar",
    failed: "No se pudo cambiar el valor",
  },
} as const satisfies MessageShape<typeof enSyncMessages>;
