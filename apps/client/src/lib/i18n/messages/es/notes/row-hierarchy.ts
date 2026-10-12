export const rowHierarchy = {
  databaseSubitemCreate: "Añadir subelemento",
  databaseSubitemCount: (count: string) => `${count} subelementos`,
  databaseSubitemMoveToRoot: "Mover al nivel superior",
  databaseSubitemMove: "Mover debajo de otra fila",
  databaseSubitemParent: "Fila principal",
  databaseSubitemUnloadedParent: "Fila principal actual (sin cargar)",
  databaseSubitemRoot: "Nivel superior",
  databaseSubitemMoveApply: "Mover fila",
} as const;
