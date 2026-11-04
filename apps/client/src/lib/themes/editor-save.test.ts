import { describe, expect, it } from "vitest";
import type { UserThemeWrite } from "$lib/api/themes";
import { planThemeSave, themeSaveGroupKey } from "./editor-save";

function theme(overrides: Partial<UserThemeWrite> = {}): UserThemeWrite {
  return {
    id: "theme-1",
    displayName: "Forest",
    iconLabel: "dark",
    seedIconLabel: "dark",
    blendCanvas: "#101010",
    seedBlendCanvas: "#101010",
    derivationEngineVersion: 3,
    calendarDefaultMode: "app-canvas",
    calendarDefaultCustom: "#101010",
    seedCalendarDefaultMode: "app-canvas",
    seedCalendarDefaultCustom: "#101010",
    tokens: [
      { kind: "source", key: "canvas", value: "#101010", isolated: false },
      { kind: "app", key: "--primary", value: "#00aa00", isolated: false },
    ],
    palette: [
      { slot: 0, value: "#ff0000" },
      { slot: 1, value: "#00ff00" },
    ],
    seedTokens: [{ kind: "source", key: "canvas", value: "#101010", isolated: false }],
    seedPalette: [{ slot: 0, value: "#ff0000" }],
    ...overrides,
  };
}

function withPrimary(value: string): Partial<UserThemeWrite> {
  return {
    tokens: [
      { kind: "source", key: "canvas", value: "#101010", isolated: false },
      { kind: "app", key: "--primary", value, isolated: false },
    ],
  };
}

describe("planThemeSave", () => {
  it("writes nothing when the session changed nothing, even after a remote edit", () => {
    const base = theme();
    const stored = theme({ displayName: "Remote name", ...withPrimary("#0000ff") });
    expect(planThemeSave({ base, edited: theme(), stored })).toEqual({ kind: "unchanged" });
  });

  it("writes nothing for an unchanged session of a deleted theme", () => {
    expect(planThemeSave({ base: theme(), edited: theme(), stored: undefined })).toEqual({ kind: "unchanged" });
  });

  it("reports a deleted theme when the session changed it", () => {
    const plan = planThemeSave({ base: theme(), edited: theme(withPrimary("#123456")), stored: undefined });
    expect(plan).toEqual({ kind: "deleted" });
  });

  it("keeps a remote rename when the session changed only the content", () => {
    const edited = theme(withPrimary("#123456"));
    const stored = theme({ displayName: "Remote name" });
    const plan = planThemeSave({ base: theme(), edited, stored });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.conflicts).toEqual([]);
    expect(plan.write.displayName).toBe("Remote name");
    expect(plan.write.tokens).toEqual(edited.tokens);
  });

  it("keeps remote content when the session changed only the name and icon", () => {
    const stored = theme({ ...withPrimary("#0000ff"), blendCanvas: "#202020" });
    const edited = theme({ displayName: "Mine", iconLabel: "light" });
    const plan = planThemeSave({ base: theme(), edited, stored });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.conflicts).toEqual([]);
    expect(plan.write.displayName).toBe("Mine");
    expect(plan.write.iconLabel).toBe("light");
    expect(plan.write.tokens).toEqual(stored.tokens);
    expect(plan.write.blendCanvas).toBe("#202020");
  });

  it("reports a conflict when both sides changed the content differently", () => {
    const plan = planThemeSave({
      base: theme(),
      edited: theme(withPrimary("#123456")),
      stored: theme(withPrimary("#0000ff")),
    });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.conflicts).toEqual(["content"]);
    expect(plan.write.tokens).toEqual(theme(withPrimary("#123456")).tokens);
  });

  it("reports only the groups both sides changed to different values", () => {
    const plan = planThemeSave({
      base: theme(),
      edited: theme({ displayName: "Mine", iconLabel: "light", ...withPrimary("#123456") }),
      stored: theme({ displayName: "Theirs", iconLabel: "light", ...withPrimary("#123456") }),
    });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.conflicts).toEqual(["name"]);
  });

  it("ignores a remote change to a group the session left alone", () => {
    const plan = planThemeSave({
      base: theme(),
      edited: theme({ iconLabel: "light" }),
      stored: theme({ calendarDefaultMode: "custom" }),
    });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.conflicts).toEqual([]);
    expect(plan.write.calendarDefaultMode).toBe("custom");
    expect(plan.write.iconLabel).toBe("light");
  });

  it("writes every changed group against the stored theme when no base was recorded", () => {
    const stored = theme({ displayName: "Theirs" });
    const plan = planThemeSave({ base: undefined, edited: theme(withPrimary("#123456")), stored });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.conflicts).toEqual([]);
    expect(plan.write.displayName).toBe("Forest");
    expect(plan.write.tokens).toEqual(theme(withPrimary("#123456")).tokens);
  });

  it("keeps the stored seed in the write", () => {
    const stored = theme({ seedBlendCanvas: "#303030" });
    const plan = planThemeSave({ base: theme(), edited: theme({ seedBlendCanvas: "#999999", iconLabel: "light" }), stored });
    expect(plan.kind).toBe("write");
    if (plan.kind !== "write") return;
    expect(plan.write.seedBlendCanvas).toBe("#303030");
  });
});

describe("themeSaveGroupKey", () => {
  it("compares content regardless of row order", () => {
    const a = theme();
    const b = theme({ tokens: [...a.tokens].reverse(), palette: [...a.palette].reverse() });
    expect(themeSaveGroupKey(a, "content")).toBe(themeSaveGroupKey(b, "content"));
  });

  it("treats an isolation change as a content change", () => {
    const a = theme();
    const b = theme({
      tokens: a.tokens.map((token) => (token.kind === "app" ? { ...token, isolated: true } : token)),
    });
    expect(themeSaveGroupKey(a, "content")).not.toBe(themeSaveGroupKey(b, "content"));
  });
});
