import { describe, expect, it } from "vitest";
import {
  alignPanelSizeToDevicePixel,
  chatBottomPanelResizeMaximum,
  chatLayoutDecision,
  chatLayoutPrimaryActions,
  chatInspectorResizeMaximum,
  chatReplyThreadResizeMaximum,
  chatScrollBehavior,
  clampPanelSizeToWholePixel,
  fittedChatBottomPanelHeight,
  fittedChatInspectorWidth,
  fittedChatReplyThreadWidth,
  middleTruncate,
  panelSizeWithCollapseSnap,
  panelWidthFromKey,
  type ChatColumnResizeInput,
  type ChatLayoutVariant,
} from "./responsive-layout";

function layout(
  containerWidth: number,
  containerHeight = 700,
  previousVariant?: ChatLayoutVariant,
  fontScale = 1,
) {
  return chatLayoutDecision({
    containerWidth,
    containerHeight,
    fontScale,
    railOpen: true,
    inspectorOpen: true,
    inspectorWidth: 520,
    previousVariant,
  });
}

describe("Chat responsive layout", () => {
  it("selects all fit-based panel presentations", () => {
    expect(layout(1_400).variant).toBe("three_column");
    expect(layout(1_200).variant).toBe("inspector_sheet");
    expect(layout(950).variant).toBe("inspector_sheet");
    expect(layout(700).variant).toBe("rail_sheet");
    expect(layout(280, 180).variant).toBe("minimum_recovery");
    expect(chatLayoutDecision({
      containerWidth: 900,
      containerHeight: 700,
      fontScale: 1,
      railOpen: true,
      inspectorOpen: false,
      inspectorWidth: 360,
    }).variant).toBe("no_inspector");
  });

  it("accounts for increased font scale and available height", () => {
    expect(layout(900, 700, undefined, 1.5).variant).toBe("rail_sheet");
    expect(layout(900, 200).variant).toBe("minimum_recovery");
  });

  it("uses hysteresis to avoid panel oscillation at fit thresholds", () => {
    expect(layout(1_230, 700, "three_column").variant).toBe("three_column");
    expect(layout(1_230, 700, "inspector_sheet").variant).toBe("inspector_sheet");
    expect(layout(695, 700, "no_inspector").railPresentation).toBe("column");
    expect(layout(695, 700, "rail_sheet").railPresentation).toBe("sheet");
  });

  it("reserves only the restoration strip when the column explorer is collapsed", () => {
    const collapsed = chatLayoutDecision({
      containerWidth: 520,
      containerHeight: 700,
      fontScale: 1,
      railOpen: false,
      inspectorOpen: false,
      inspectorWidth: 520,
    });
    const expanded = chatLayoutDecision({
      containerWidth: 520,
      containerHeight: 700,
      fontScale: 1,
      railOpen: true,
      inspectorOpen: false,
      inspectorWidth: 520,
    });

    expect(collapsed.railPresentation).toBe("column");
    expect(expanded.railPresentation).toBe("sheet");
  });

  it("keeps every primary recovery route in every layout", () => {
    for (const decision of [layout(1_400), layout(950), layout(700), layout(280, 180)]) {
      expect(chatLayoutPrimaryActions(decision)).toEqual([
        "threads",
        "composer",
        "requests",
        "stop",
        "settings",
        "inspector",
      ]);
    }
  });

  it("keeps reply threads independent from the workspace inspector", () => {
    const both = chatLayoutDecision({
      containerWidth: 1_240,
      containerHeight: 700,
      fontScale: 1,
      railOpen: false,
      inspectorOpen: true,
      inspectorWidth: 480,
      replyThreadOpen: true,
      replyThreadWidth: 440,
    });
    expect(both.railPresentation).toBe("sheet");
    expect(both.replyThreadPresentation).toBe("column");
    expect(both.inspectorPresentation).toBe("column");

    const threadOnly = chatLayoutDecision({
      containerWidth: 900,
      containerHeight: 700,
      fontScale: 1,
      railOpen: true,
      inspectorOpen: false,
      inspectorWidth: 520,
      replyThreadOpen: true,
      replyThreadWidth: 440,
    });
    expect(threadOnly.railPresentation).toBe("sheet");
    expect(threadOnly.replyThreadPresentation).toBe("column");
    expect(threadOnly.inspectorPresentation).toBe("closed");
  });

  it("promotes a reply thread to the main surface before auxiliary panels overflow", () => {
    const constrained = chatLayoutDecision({
      containerWidth: 900,
      containerHeight: 700,
      fontScale: 1,
      railOpen: false,
      inspectorOpen: true,
      inspectorWidth: 460,
      replyThreadOpen: true,
      replyThreadWidth: 440,
    });
    expect(constrained.replyThreadPresentation).toBe("main");
    expect(constrained.inspectorPresentation).toBe("column");

    const narrow = chatLayoutDecision({
      containerWidth: 620,
      containerHeight: 700,
      fontScale: 1,
      railOpen: false,
      inspectorOpen: true,
      inspectorWidth: 240,
      replyThreadOpen: true,
      replyThreadWidth: 440,
    });
    expect(narrow.replyThreadPresentation).toBe("main");
    expect(narrow.inspectorPresentation).toBe("sheet");
  });

  it("shows one selected surface at the minimum recovery floor", () => {
    const inspector = layout(280, 180);
    expect(inspector.activeSurface).toBe("inspector");
    const rail = chatLayoutDecision({
      containerWidth: 280,
      containerHeight: 180,
      fontScale: 1,
      railOpen: true,
      inspectorOpen: false,
      inspectorWidth: 360,
    });
    expect(rail.activeSurface).toBe("rail");
    const conversation = chatLayoutDecision({
      containerWidth: 280,
      containerHeight: 180,
      fontScale: 1,
      railOpen: false,
      inspectorOpen: false,
      inspectorWidth: 360,
    });
    expect(conversation.activeSurface).toBe("conversation");
  });

  it("middle-truncates long Unicode labels without losing either end", () => {
    expect(middleTruncate("proyecto-非常に長い-workspace-folder", 15)).toBe("proyect…-folder");
    expect(middleTruncate("short", 15)).toBe("short");
  });

  it("bounds long Spanish labels and workspace paths without splitting Unicode", () => {
    const spanish = "Configuración del espacio de trabajo de investigación internacional";
    const path = "proyectos/🚀-lanzamiento/investigación/documentación/decisiones-arquitectónicas.md";
    expect(Array.from(middleTruncate(spanish, 24))).toHaveLength(24);
    expect(middleTruncate(spanish, 24)).toMatch(/^Configuració.*ternacional$/);
    expect(Array.from(middleTruncate(path, 30))).toHaveLength(30);
    expect(middleTruncate(path, 30)).toMatch(/^proyectos\/.*tectónicas\.md$/);
  });

  it("supports keyboard-only separator increments, bounds, and reset", () => {
    const resize = (key: string, direction: "standard" | "reversed" = "standard") => panelWidthFromKey({
      current: 260,
      minimum: 160,
      maximum: 520,
      defaultValue: 260,
      step: 16,
      direction,
      key,
    });
    expect(resize("ArrowLeft")).toBe(244);
    expect(resize("ArrowRight")).toBe(276);
    expect(resize("ArrowLeft", "reversed")).toBe(276);
    expect(resize("Home")).toBe(160);
    expect(resize("End")).toBe(520);
    expect(resize("Enter")).toBe(260);
    expect(resize("Escape")).toBeNull();
  });

  it("aligns pointer resize dimensions and fractional bounds to whole pixels", () => {
    expect(clampPanelSizeToWholePixel(319.49, 160, 520)).toBe(319);
    expect(clampPanelSizeToWholePixel(319.5, 160, 520)).toBe(320);
    expect(clampPanelSizeToWholePixel(800, 240, 519.75)).toBe(519);
    expect(clampPanelSizeToWholePixel(100, 240.2, 200)).toBe(241);
  });

  it("aligns moving panel edges to physical pixels at fractional display scales", () => {
    const align = (
      value: number,
      direction: "from-start" | "from-end",
      anchor: number,
      devicePixelRatio = 1.25,
    ) => alignPanelSizeToDevicePixel({
      value,
      minimum: 160,
      maximum: 520,
      anchor,
      direction,
      devicePixelRatio,
    });
    expect(align(320, "from-start", 5.625)).toBeCloseTo(319.975);
    expect((5.625 + align(320, "from-start", 5.625)) * 1.25).toBeCloseTo(407);
    expect((994.375 - align(320, "from-end", 994.375)) * 1.25).toBeCloseTo(843);
    expect(align(900, "from-start", 0)).toBe(520);
    expect(align(100, "from-end", 0)).toBe(160);
    expect(align(320.4, "from-start", 0, 0)).toBe(320);
  });

  it("lets the inspector use spare width without shrinking the conversation", () => {
    const maximum = (containerWidth: number, railVisible = true) => chatInspectorResizeMaximum({
      containerWidth,
      railVisible,
      minimum: 240,
      maximum: 960,
    });
    expect(maximum(1_400)).toBe(704);
    expect(maximum(1_400, false)).toBe(916);
    expect(maximum(2_000)).toBe(960);
    expect(maximum(700)).toBe(240);
    expect(maximum(1_399.75)).toBe(703);
    expect(chatInspectorResizeMaximum({
      containerWidth: 1_240,
      railVisible: false,
      railReservedWidth: 0,
      conversationMinimum: 320,
      additionalReservedWidth: 440,
      minimum: 240,
      maximum: 960,
    })).toBe(480);
  });

  it("lets the reply thread grow while the conversation keeps its narrower minimum beside it", () => {
    const maximum = (input: Partial<ChatColumnResizeInput>) => chatReplyThreadResizeMaximum({
      containerWidth: 1_400,
      railVisible: true,
      minimum: 280,
      maximum: 800,
      ...input,
    });
    expect(maximum({})).toBe(800);
    expect(maximum({ containerWidth: 1_000 })).toBe(424);
    expect(maximum({ containerWidth: 1_000, fontScale: 1.5 })).toBe(280);
    expect(maximum({ railReservedWidth: 0, additionalReservedWidth: 520 })).toBe(560);
    expect(maximum({ conversationMinimum: 440, containerWidth: 1_000 })).toBe(304);
  });

  it("fits the reply thread to a comfortable reading width inside its resize bounds", () => {
    const fitted = (input: Partial<ChatColumnResizeInput>) => fittedChatReplyThreadWidth({
      containerWidth: 1_400,
      railVisible: true,
      minimum: 280,
      maximum: 800,
      ...input,
    });
    expect(fitted({})).toBe(448);
    expect(fitted({ containerWidth: 1_920 })).toBe(560);
    expect(fitted({ containerWidth: 1_000 })).toBe(400);
    expect(fitted({ containerWidth: 1_000, railVisible: false, fontScale: 1.5 })).toBe(476);
  });

  it("fits each outer panel to its content role and available space", () => {
    expect(fittedChatInspectorWidth({
      containerWidth: 1_400,
      railVisible: true,
      minimum: 240,
      maximum: 960,
    })).toBe(532);
    expect(fittedChatInspectorWidth({
      containerWidth: 1_920,
      railVisible: true,
      minimum: 240,
      maximum: 960,
    })).toBe(720);
    expect(fittedChatBottomPanelHeight({
      containerHeight: 700,
      minimum: 96,
      maximum: 520,
    })).toBe(224);
    expect(fittedChatBottomPanelHeight({
      containerHeight: 1_000,
      minimum: 96,
      maximum: 520,
    })).toBe(320);
    expect(fittedChatBottomPanelHeight({
      containerHeight: 700,
      fontScale: 1.5,
      minimum: 96,
      maximum: 520,
    })).toBe(285);
  });

  it("caps bottom panel growth and keeps the conversation reachable", () => {
    const maximum = (containerHeight: number) => chatBottomPanelResizeMaximum({
      containerHeight,
      minimum: 96,
      maximum: 520,
    });
    expect(maximum(700)).toBe(350);
    expect(maximum(1_000)).toBe(500);
    expect(maximum(400)).toBe(160);
    expect(maximum(280)).toBe(96);
  });

  it("uses a magnetic collapse zone without leaving unusably small panels", () => {
    expect(panelSizeWithCollapseSnap(70, 160, 480)).toBe(0);
    expect(panelSizeWithCollapseSnap(73, 160, 480)).toBe(160);
    expect(panelSizeWithCollapseSnap(319.6, 160, 480)).toBe(320);
    expect(panelSizeWithCollapseSnap(900, 160, 480)).toBe(480);
  });

  it("removes smooth scrolling when reduced motion is requested", () => {
    expect(chatScrollBehavior(false)).toBe("smooth");
    expect(chatScrollBehavior(true)).toBe("auto");
  });
});
