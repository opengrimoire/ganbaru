import type { theme as enTheme } from "../../en/settings/theme";
import type { MessageShape } from "../../types";

export const theme = {
  themesHeading: "Temas",
  quickToggle: "Cambio rápido",
  pickerTitle: "Tema",
  pickerLabel: "Selector de tema",
  closePicker: "Cerrar selector de tema",
  lightQuickToggle: "Tema de cambio rápido para modo claro",
  darkQuickToggle: "Tema de cambio rápido para modo oscuro",
  allThemes: "Todos los temas",
  pasteJson: "Pegar JSON de tema",
  closeImport: "Cerrar importación",
  pasteClipboard: "Pegar desde portapapeles",
  openFile: "Abrir archivo",
  import: "Importar",
  importTheme: "Importar tema",
  imported: "Tema importado",
  exporting: "Exportando tema…",
  exported: "Tema exportado",
  exportedToDownloads: (name: string) => `Tema exportado a Descargas como ${name}`,
  exportFailed: "No se pudo exportar el tema",
  clipboardFailed: "No se pudo leer el portapapeles. Pega manualmente abajo.",
  fileReadFailed: "No se pudo leer el archivo seleccionado.",
  pasteFirst: "Primero pega un objeto JSON de tema.",
  deleteTitle: (name: string) => `¿Eliminar ${name}?`,
  thisTheme: "este tema",
  cannotBeUndone: "Esto no se puede deshacer",
  duplicateAndEdit: "Duplicar y editar",
  duplicateAndEditTheme: "Duplicar y editar tema",
  viewTheme: "Ver tema",
  editTheme: "Editar tema",
  exportJson: "Exportar JSON del tema",
  exportingJson: "Exportando JSON del tema",
  dismissTransferNotification: "Cerrar notificación de transferencia del tema",
  deleteTheme: "Eliminar tema",
  activeTheme: (name: string) => `${name} está activo`,
  applyTheme: (name: string) => `Aplicar ${name}`,
  jsonSchema: "Esquema",
  jsonReadonly: "Representación de solo lectura del tema",
  jsonEditable: "Edita directamente y aplica para guardar cambios",
  copyJson: "Copiar JSON",
  saveToFile: "Guardar en archivo",
  discardEdits: "Descartar cambios",
  applyChanges: "Aplicar cambios",
  rebakeMessage: (savedVersion: number, currentVersion: number) =>
    `Este tema fue guardado con un motor de derivación anterior (v${savedVersion}, actual v${currentVersion}). Rehornea para actualizar colores no fijados.`,
  maybeLater: "Más tarde",
  rebake: "Rehornear",
  editor: {
    dialogLabel: "Editor de tema",
    expandEditor: "Expandir editor",
    collapseEditor: "Contraer editor",
    resetAllToSeed:
      "Restablecer cada fuente, anulación, espacio de paleta y etiqueta de icono al valor que tenía al clonarse",
    restoreSeedTitle: "Restaurar cada valor a la captura de clonación",
    noSeedChangesTitle: "No ha cambiado nada desde que se clonó este tema",
    resetAllToSeedButton: "Restablecer todo a la semilla",
    applyAndReturn: "Aplicar y volver",
    saveAndApply: "Guardar y aplicar",
    backToThemes: "Volver a los temas",
    editingTheme: "Tema personalizado",
    discardTitle: "¿Descartar los cambios del tema?",
    discardMessage: "Se perderán los cambios del tema que no hayas guardado.",
    discardChanges: "Descartar cambios",
    keepEditing: "Seguir editando",
    operationFailed: "La operación del tema falló. El editor permanece abierto.",
    jsonCopied: "JSON copiado al portapapeles",
    jsonCopyFailed: "No se pudo copiar el JSON",
    jsonSaving: "Guardando…",
    jsonSaved: "Guardado en archivo",
    jsonSavedToDownloads: (name: string) => `Guardado en Descargas como ${name}`,
    jsonSaveFailed: "No se pudo guardar el archivo",
    dismissFileNotification: "Cerrar notificación del archivo del tema",
    jsonUpdated: "Tema actualizado desde JSON",
    day: "día",
    night: "noche",
    iconTagLabel: (label: string) => `Etiqueta de icono: ${label}`,
    iconTagEditableLabel: (label: string) =>
      `Etiqueta de icono: ${label} (decorativa, haz clic para cambiarla)`,
    builtInIconTag: "Etiqueta de icono de tema integrado",
    iconTagTitle: (label: string) =>
      `Etiqueta decorativa para uso de ${label}. Haz clic para cambiarla.`,
    themeName: "Nombre del tema",
    sectionsLabel: "Secciones del editor de tema",
    originalValue: "Ya está en el valor original",
    restoreOriginal: "Restaurar valor original",
    resetOriginal: (label: string) => `Restablecer ${label} a su valor original`,
    builtInReadOnly: "Los temas integrados son de solo lectura",
    linkedColorsResetThroughSource:
      "Los colores vinculados se restablecen desde su fuente",
    isolateEditLabel: (label: string) => `Edición aislada de ${label}`,
    isolateEditTitle: "Editar este color independientemente de su fuente",
    isolateEdit: "Edición aislada",
    linkBackLabel: (label: string) => `Volver a vincular ${label} a su fuente`,
    linkBackTitle: "Volver a vincular este color a su fuente",
    linkBack: "Vincular de nuevo",
    backgroundShort: "Fondo",
    textShort: "Texto",
    backgroundControl: (label: string) => `Fondo de ${label}`,
    textControl: (label: string) => `Texto de ${label}`,
    sourceControl: (label: string) => `Fuente de ${label}`,
    expandGroupLabel: (label: string) => `Expandir opciones de ${label}`,
    collapseGroupLabel: (label: string) => `Contraer opciones de ${label}`,
    expand: "Expandir",
    collapse: "Contraer",
    colorDefaults: "Colores predeterminados",
    colorDefaultsDescription: "Superficie, paleta y detalles",
    customCalendarDefault: "Predeterminado personalizado del calendario",
    controlsLabel: "Controles del editor de tema",
    contrastIssue: (count: number) =>
      `${count} ${count === 1 ? "problema" : "problemas"} de contraste`,
    jumpToNextContrast: "Ir a la siguiente fila con contraste insuficiente",
    jumpToNextContrastTitle:
      "Desplaza a la siguiente fila por debajo de su objetivo de contraste. Las superficies atenuadas apuntan a 3:1; las demás apuntan a 4.5:1.",
    jumpToNext: "Ir al siguiente",
    fixAllContrast: "Corregir automáticamente todas las filas con contraste insuficiente",
    fixAllContrastTitle:
      "Elegir un color de texto legible para cada par debajo de su objetivo",
    fixAll: "Corregir todo",
    contrastReadOnlyLabel: (label: string, ratio: string) =>
      `El contraste de texto de ${label} es ${ratio}:1`,
    contrastAutoFixLabel: (label: string) =>
      `Corregir automáticamente el contraste de texto de ${label}`,
    contrastTargetAaBody: "texto de cuerpo AA",
    contrastTargetAaLargeUi: "texto grande o interfaz AA",
    contrastTitle: (ratio: string, target: string, suffix: string) =>
      `Contraste ${ratio}:1. Este par apunta a ${target}:1 (${suffix}).`,
    contrastTitleEditable: (ratio: string, target: string, suffix: string) =>
      `Contraste ${ratio}:1. Este par apunta a ${target}:1 (${suffix}). Haz clic para elegir automáticamente un color de texto legible.`,
    eventPalette: "Paleta de eventos",
    eventPaletteDescription:
      "32 espacios de color, cada uno con una variante atenuada para atenuar eventos pasados opcionalmente, mezclada hacia el fondo del calendario",
    pastVariant: (color: string) => `Variante pasada ${color}`,
    nav: {
      general: "General",
      calendar: "Calendario",
      signals: "Texto y acciones",
      json: "JSON",
    },
    calendarDefault: {
      light: "Basado en claro",
      dark: "Basado en oscuro",
      appCanvas: "Basado en lienzo de app",
      custom: "Basado en personalizado",
    },
    token: {
      appCanvas: {
        title: "Lienzo de app",
        description: "La mayoría de las vistas pintan su propia superficie encima",
      },
      calendarHeader: {
        title: "Encabezado del calendario",
        description: "Barra del calendario y encabezados de día y hora",
      },
      textColor: {
        title: "Color de texto",
        description: "Se aplica al texto de toda la app",
      },
      card: {
        title: "Tarjeta",
        description:
          "Fondo de paneles agrupados, diálogos y tarjetas tintadas",
      },
      primaryAction: {
        title: "Acción primaria",
        description: "Color de acento principal para botones y enlaces resaltados",
      },
      buttonText: {
        title: "Texto de botón",
        description: "Texto sobre botones primarios",
      },
      destructive: {
        title: "Destructivo",
        description: "Color usado para acciones de eliminación y advertencias",
      },
      destructiveText: {
        title: "Texto destructivo",
        description:
          "Color de texto sobre botones destructivos y el cierre de la barra de título al pasar el cursor",
      },
      focusRing: {
        title: "Anillo de enfoque",
        description: "Contorno alrededor de campos y botones enfocados",
      },
      eventPanelSurface: {
        title: "Superficie del panel de evento",
        description: "Fondo del panel para crear o editar eventos",
      },
      eventPanelSectionHeader: {
        title: "Encabezado de sección del panel de evento",
        description: "Franja de fondo detrás de filas de sección",
      },
      eventPanelBodyText: {
        title: "Texto del panel de evento",
        description: "Reemplaza --foreground dentro del panel",
      },
      eventPanelMutedText: {
        title: "Texto atenuado del panel de evento",
        description: "Color de texto secundario dentro del panel",
      },
      confirmAction: {
        title: "Acción de confirmación",
        description:
          "Fondo del botón Guardar y de la píldora de alcance activa en el panel de evento",
      },
      confirmActionText: {
        title: "Texto de acción de confirmación",
        description: "Color de texto del botón Guardar y de la píldora activa",
      },
      armedDelete: {
        title: "Eliminación armada",
        description:
          "Fondo del botón de eliminar cuando está armado (estado de hacer clic otra vez para confirmar)",
      },
      armedDeleteText: {
        title: "Texto de eliminación armada",
        description: "Color de texto del botón de eliminar en estado armado",
      },
      acceptedAttendee: {
        title: "Asistente aceptado",
        description:
          "Color de tarjeta de estado para asistentes aceptados en un evento",
      },
      acceptedAttendeeText: {
        title: "Texto de asistente aceptado",
        description: "Color de texto en la tarjeta de asistencia aceptada",
      },
      tentativeAttendee: {
        title: "Asistente tentativo",
        description:
          "Color de tarjeta de estado para asistentes tentativos en un evento",
      },
      tentativeAttendeeText: {
        title: "Texto de asistente tentativo",
        description: "Color de texto en la tarjeta de asistencia tentativa",
      },
      declinedAttendee: {
        title: "Asistente rechazado",
        description:
          "Color de tarjeta de estado para asistentes rechazados en un evento",
      },
      declinedAttendeeText: {
        title: "Texto de asistente rechazado",
        description: "Color de texto en la tarjeta de asistencia rechazada",
      },
      calendarBackground: {
        title: "Fondo del calendario",
        description: "Fondo de la cuadrícula del calendario",
      },
      gridLines: {
        title: "Líneas de cuadrícula",
        description: "Color de las líneas separadoras de horas y días",
      },
      timeLabels: {
        title: "Etiquetas de hora",
        description: "Números de hora al lado del calendario",
      },
      nowLine: {
        title: "Línea actual",
        description: "Línea horizontal que marca la hora actual",
      },
      emptyRail: {
        title: "Riel vacío",
        description: "Color de partes vacías del riel de sesión Pomodoro",
      },
      breakMarker: {
        title: "Marcador de descanso",
        description: "Color de segmentos de descanso en el riel de sesión",
      },
      focusMarker: {
        title: "Marcador de enfoque",
        description: "Color de segmentos de enfoque en el riel de sesión",
      },
    },
    group: {
      appCanvas: {
        title: "Lienzo de app",
        description:
          "Color de fondo dominante; la mayoría de las superficies se tintan automáticamente desde él",
      },
      calendarSurface: {
        title: "Superficie del calendario",
        description: "Fondo del calendario, cuadrícula y etiquetas de hora",
      },
      calendarDetails: {
        title: "Detalles del calendario",
        description: "Estados del riel y acentos en la cuadrícula del calendario",
      },
      eventPanel: {
        title: "Panel de evento",
        description:
          "Superficies del panel para crear y editar eventos abierto desde el calendario",
      },
      ink: {
        title: "Tinta",
        description: "Color de texto base",
      },
      primaryAction: {
        title: "Acción primaria",
        description: "Acento para botones y enlaces resaltados",
      },
      destructive: {
        title: "Destructivo",
        description: "Señal de peligro",
      },
      confirm: {
        title: "Confirmar",
        description: "Señal positiva",
      },
      warning: {
        title: "Advertencia",
        description: "Señal de cautela",
      },
    },
    pair: {
      popover: {
        title: "Emergente",
        description: "Listas desplegables, menús y paneles flotantes",
      },
      secondarySurface: {
        title: "Superficie secundaria",
        description: "Botones con menos énfasis",
      },
      mutedSurface: {
        title: "Superficie atenuada",
        description: "Pozos sutiles y color predeterminado de texto de ayuda",
      },
      hoverHighlight: {
        title: "Resaltado al pasar el cursor",
        description: "Tinte suave al pasar el cursor sobre filas y botones",
      },
      titleBar: {
        title: "Barra de título",
        description: "Marco superior de la ventana de la app",
      },
      titleBarHover: {
        title: "Barra de título al pasar el cursor",
        description: "Tinte al pasar el cursor sobre botones de la barra de título",
      },
      destructive: {
        title: "Destructivo",
        description: "Acciones de eliminar, eliminación armada y estado rechazado",
      },
      confirm: {
        title: "Confirmar",
        description: "Acciones de guardar, píldoras activas y estado aceptado",
      },
      warning: {
        title: "Advertencia",
        description: "Estado tentativo y superficies de cautela",
      },
    },
  },
} as const satisfies MessageShape<typeof enTheme>;
