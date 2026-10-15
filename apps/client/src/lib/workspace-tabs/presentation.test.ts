import { describe, expect, it } from "vitest";
import { workspaceTabPositionShortcut } from "./presentation";

describe("workspace tab position shortcuts", () => {
  it("numbers the first eight tabs", () => {
    expect(workspaceTabPositionShortcut(0, 3)).toBe("Alt+1");
    expect(workspaceTabPositionShortcut(7, 12)).toBe("Alt+8");
  });

  it("gives the last tab Alt+9 whatever its position", () => {
    expect(workspaceTabPositionShortcut(8, 9)).toBe("Alt+9");
    expect(workspaceTabPositionShortcut(11, 12)).toBe("Alt+9");
  });

  it("leaves tabs between the eighth and the last without a shortcut", () => {
    expect(workspaceTabPositionShortcut(8, 12)).toBeNull();
    expect(workspaceTabPositionShortcut(10, 12)).toBeNull();
  });

  it("keeps a short strip's last tab on its own number", () => {
    expect(workspaceTabPositionShortcut(2, 3)).toBe("Alt+3");
  });
});
