export type MusicBuilderLayoutMode = "wide" | "medium" | "narrow";

export interface MusicBuilderLayoutInput {
  width: number;
  height: number;
}

export interface MusicBuilderLayoutProjection {
  mode: MusicBuilderLayoutMode;
  contextPanelPresentation: "persistent" | "sheet";
  dockPresentation: "sidebar" | "bottom";
  comfortable: boolean;
}

export function projectMusicBuilderLayout(
  input: MusicBuilderLayoutInput,
): MusicBuilderLayoutProjection {
  const width = Math.max(0, input.width);
  const height = Math.max(0, input.height);
  if (width >= 1120 && height >= 440) {
    return {
      mode: "wide",
      contextPanelPresentation: "persistent",
      dockPresentation: "sidebar",
      comfortable: true,
    };
  }
  if (width >= 620 && height >= 300) {
    return {
      mode: "medium",
      contextPanelPresentation: "persistent",
      dockPresentation: "sidebar",
      comfortable: height >= 380,
    };
  }
  return {
    mode: "narrow",
    contextPanelPresentation: "sheet",
    dockPresentation: "bottom",
    comfortable: width >= 360 && height >= 300,
  };
}
