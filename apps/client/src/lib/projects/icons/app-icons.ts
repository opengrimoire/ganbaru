export type ProjectAppIconNode = readonly [string, Readonly<Record<string, string>>];

export const SPORT_SHOE_ICON_NODE = [
  ["path", { d: "m15 10.42 4.8-5.07" }],
  ["path", { d: "M19 18h3" }],
  [
    "path",
    {
      d: "M9.5 22 21.414 9.415A2 2 0 0 0 21.2 6.4l-5.61-4.208A1 1 0 0 0 14 3v2a2 2 0 0 1-1.394 1.906L8.677 8.053A1 1 0 0 0 8 9c-.155 6.393-2.082 9-4 9a2 2 0 0 0 0 4h14",
    },
  ],
] as const satisfies readonly ProjectAppIconNode[];

const PROJECT_APP_ICON_NODES: Readonly<Record<string, readonly ProjectAppIconNode[]>> = {
  "sport-shoe": SPORT_SHOE_ICON_NODE,
};

export function projectAppIconNode(slug: string): readonly ProjectAppIconNode[] | null {
  return PROJECT_APP_ICON_NODES[slug] ?? null;
}
