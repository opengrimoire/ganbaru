import { describe, expect, it } from "vitest";
import { PhysicalPosition } from "@tauri-apps/api/dpi";
import { nativeFileDrag } from "./native-file-drops";

const position = new PhysicalPosition(10, 20);

describe("nativeFileDrag", () => {
  it("treats entering and moving over the window as hovering", () => {
    expect(nativeFileDrag({ type: "enter", paths: ["/a.png"], position })).toEqual({ kind: "hover" });
    expect(nativeFileDrag({ type: "over", position })).toEqual({ kind: "hover" });
  });

  it("reports how many files were dropped without their paths", () => {
    const drag = nativeFileDrag({ type: "drop", paths: ["/a.png", "/b.jpg"], position });
    expect(drag).toEqual({ kind: "drop", count: 2 });
    expect(JSON.stringify(drag)).not.toContain("a.png");
  });

  it("reports a canceled drag as leaving", () => {
    expect(nativeFileDrag({ type: "leave" })).toEqual({ kind: "leave" });
  });
});
