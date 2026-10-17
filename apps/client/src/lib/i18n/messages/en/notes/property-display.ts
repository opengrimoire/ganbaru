export const propertyDisplay = {
  databaseDisplayRange: (start: string, end: string) => `${start} to ${end}`,
  databaseDateStart: "Start",
  databaseDateEnd: "End",
  databaseDateEndDate: "End date",
  databaseDateAddTime: "Add time",
  databaseDateRemoveTime: "Remove time",
  databaseDateTimeZone: "Time zone",
  databaseDateZoneHint: "Local time",
  databaseDateClear: "Clear date",
  databaseDateInvalidZone: "Enter a valid IANA time zone, such as Europe/Madrid, or leave it empty.",
  databaseDateSaveFailed: "The date could not be saved.",
} as const;
