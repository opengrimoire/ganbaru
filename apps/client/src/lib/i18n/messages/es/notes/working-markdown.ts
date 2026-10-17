import type { workingMarkdown as enWorkingMarkdown } from "../../en/notes/working-markdown";
import type { MessageShape } from "../../types";

export const workingMarkdown = {
  workingMarkdown: {
    title: "Carpetas de trabajo",
    refreshTree: "Actualizar archivos Markdown",
    truncated: "El análisis de la carpeta alcanzó un límite de seguridad. Algunos archivos Markdown no se muestran.",
    unavailableFolders: (count: number) => count === 1
      ? "Una carpeta de trabajo del proyecto no está disponible en este dispositivo."
      : `${count} carpetas de trabajo del proyecto no están disponibles en este dispositivo.`,
    unsaved: "Cambios sin guardar",
    edit: "Editar",
    preview: "Vista previa",
    refreshFile: "Actualizar",
    rawEditor: "Editor de Markdown sin formato",
    openExternally: "Abrir externamente",
    copyPath: "Copiar ruta relativa",
    saving: "Guardando",
    conflict: "Este archivo cambió fuera de Ganbaru. Tu texto local no fue sobrescrito.",
    reloadRemote: "Recargar archivo",
    copyLocal: "Copiar texto local",
    discardConfirm: "¿Descartar los cambios sin guardar de este archivo Markdown?",
  },
} as const satisfies MessageShape<typeof enWorkingMarkdown>;
