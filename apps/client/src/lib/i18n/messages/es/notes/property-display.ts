export const propertyDisplay = {
  databaseDisplayRange: (start: string, end: string) => `${start} a ${end}`,
  databaseDateStart: "Inicio",
  databaseDateEnd: "Fin",
  databaseDateEndDate: "Fecha de fin",
  databaseDateAddTime: "Añadir hora",
  databaseDateRemoveTime: "Quitar hora",
  databaseDateTimeZone: "Zona horaria",
  databaseDateZoneHint: "Hora local",
  databaseDateClear: "Borrar fecha",
  databaseDateInvalidZone: "Introduce una zona horaria IANA válida, como Europe/Madrid, o deja el campo vacío.",
  databaseDateSaveFailed: "No se pudo guardar la fecha.",
} as const;
