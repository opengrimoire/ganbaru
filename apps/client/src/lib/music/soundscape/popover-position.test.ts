import { describe, expect, it } from "vitest";
import { centeredSoundscapePanelLeft } from "./popover-position";

describe("soundscape player panel placement", () => {
  it("centers over the trigger when there is room", () => {
    expect(centeredSoundscapePanelLeft(400, 36, 304, 800)).toBe(266);
  });

  it("keeps the panel inside either viewport edge", () => {
    expect(centeredSoundscapePanelLeft(8, 36, 304, 800)).toBe(8);
    expect(centeredSoundscapePanelLeft(760, 36, 304, 800)).toBe(488);
  });
});
