export const rowHierarchy = {
  databaseSubitemCreate: "Add sub-item",
  databaseSubitemCount: (count: string) => `${count} sub-items`,
  databaseSubitemMoveToRoot: "Move to top level",
  databaseSubitemMove: "Move under another row",
  databaseSubitemParent: "Parent row",
  databaseSubitemUnloadedParent: "Current parent (not loaded)",
  databaseSubitemRoot: "Top level",
  databaseSubitemMoveApply: "Move row",
} as const;
