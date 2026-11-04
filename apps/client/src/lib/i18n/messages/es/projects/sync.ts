import type { syncMessages as enSyncMessages } from "../../en/projects/sync";
import type { MessageShape } from "../../types";

export const syncMessages = {
  sync: {
    projectConflicts: (count: number) =>
      count === 1
        ? "1 nombre de proyecto se editó en dos dispositivos. Abre los ajustes del proyecto para elegir qué conservar."
        : `${count} nombres de proyecto se editaron en dos dispositivos. Abre los ajustes del proyecto para elegir qué conservar.`,
    taskConflicts: (count: number) =>
      count === 1
        ? "1 tarea se editó en dos dispositivos. Ábrela para elegir qué conservar."
        : `${count} tareas se editaron en dos dispositivos. Ábrelas para elegir qué conservar.`,
    checklistConflicts: (count: number) =>
      count === 1
        ? "1 elemento de lista de verificación se editó en dos dispositivos. Abre la tarea para elegir qué conservar."
        : `${count} elementos de lista de verificación se editaron en dos dispositivos. Abre la tarea para elegir qué conservar.`,
    dependencyConflicts: (count: number) =>
      count === 1
        ? "1 dependencia se quitó para romper un ciclo. Abre la tarea para mantenerla quitada o volver a añadirla."
        : `${count} dependencias se quitaron para romper un ciclo. Abre la tarea para mantenerlas quitadas o volver a añadirlas.`,
    groupUntitled: "Grupo sin título",
    groupRecoveryNote: "Al restaurarlo se crea el grupo de nuevo al final de la lista, con los proyectos que se eliminaron con él.",
    groupRestored: "Grupo restaurado",
    groupRestoreFailed: "No se pudo restaurar el grupo",
    projectUntitled: "Proyecto sin título",
    projectRecoveryNote: "Al restaurarlo se crea el proyecto de nuevo en su grupo, con sus tareas, y se le asigna una carpeta de trabajo nueva.",
    projectRestored: "Proyecto restaurado",
    projectRestoreFailed: "No se pudo restaurar el proyecto",
    sectionUntitled: "Sección sin título",
    sectionRecoveryNote: "Al restaurarla se crea la sección de nuevo en su proyecto.",
    sectionRestored: "Sección restaurada",
    sectionRestoreFailed: "No se pudo restaurar la sección",
    statusUntitled: "Estado sin título",
    statusRecoveryNote: "Al restaurarlo se crea el estado de nuevo en su proyecto.",
    statusRestored: "Estado restaurado",
    statusRestoreFailed: "No se pudo restaurar el estado",
    taskUntitled: "Tarea sin título",
    taskRecoveryNote: "Al restaurarla se crea la tarea de nuevo en su proyecto, con su lista de verificación y las dependencias que siguen vigentes.",
    taskRestored: "Tarea restaurada",
    taskRestoreFailed: "No se pudo restaurar la tarea",
  },
  conflict: {
    nameTitle: "Este nombre de proyecto se editó en dos dispositivos",
    titleTitle: "Este título se editó en dos dispositivos",
    descriptionTitle: "Esta descripción se editó en dos dispositivos",
    checklistTitle: "Este elemento se editó en dos dispositivos",
    hint: "Elige la versión que quieres conservar.",
    empty: "Vacío",
    shown: "Visible ahora",
    keepThis: "Conservar esta",
    useOther: "Usar esta",
    versionFrom: (device: string, time: string) => `${device}, ${time}`,
    thisDevice: "Este dispositivo",
    otherDevice: "Otro dispositivo",
    resolved: "Este conflicto ya se resolvió",
    failed: "No se pudo resolver el conflicto",
    dependencyTitle: "Dependencia quitada para romper un ciclo",
    dependencyHint: "Dos dispositivos añadieron dependencias que juntas hacían que las tareas se esperaran unas a otras en bucle, así que esta se ocultó.",
    dependencyLine: (blocking: string, blocked: string) => `${blocking} bloquea a ${blocked}`,
    keepRemoved: "Mantener quitada",
    restore: "Volver a añadir",
    restoreCycles: "Esta dependencia no se puede volver a añadir mientras recree el ciclo. Quita antes otra dependencia del bucle.",
  },
} as const satisfies MessageShape<typeof enSyncMessages>;
