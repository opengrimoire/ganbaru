import { describe, expect, it } from "vitest";
import { isCheckedSource, nativeConfirmViolations } from "./check-native-confirm.mjs";

describe("native confirmation check", () => {
  it("reports native confirmation calls with their line", () => {
    const files = [
      { path: "src/lib/a.svelte", text: "<script>\n  if (!window.confirm(\"Delete?\")) return;\n</script>" },
      { path: "src/lib/b.ts", text: "const ok = globalThis.confirm (message);" },
      { path: "src/lib/c.ts", text: "const ok = window\n.confirm;\nwindow . confirm(\"x\");" },
    ];
    expect(nativeConfirmViolations(files)).toEqual([
      "src/lib/a.svelte:2 calls the native confirmation",
      "src/lib/b.ts:1 calls the native confirmation",
      "src/lib/c.ts:3 calls the native confirmation",
    ]);
  });

  it("accepts the shared service and names that only contain confirm", () => {
    const files = [
      { path: "src/lib/a.ts", text: "await requestConfirmation({ message, confirmLabel });" },
      { path: "src/lib/b.svelte", text: "const confirmed = myWindow.confirmation(); onconfirm(true);" },
    ];
    expect(nativeConfirmViolations(files)).toEqual([]);
  });

  it("ignores test files and files that are not frontend sources", () => {
    const call = "window.confirm(\"x\");";
    expect(nativeConfirmViolations([
      { path: "src/lib/a.test.ts", text: call },
      { path: "src/lib/Stub.test.svelte", text: call },
      { path: "src/lib/notes.md", text: call },
    ])).toEqual([]);
    expect(isCheckedSource("src/lib/a.svelte.ts")).toBe(true);
    expect(isCheckedSource("src/lib/a.test.ts")).toBe(false);
    expect(isCheckedSource("src/app.css")).toBe(false);
  });
});
