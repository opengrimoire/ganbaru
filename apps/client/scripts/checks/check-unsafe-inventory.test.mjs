import { describe, expect, it } from "vitest";
import {
  inventoryPaths,
  staleInventoryViolations,
  unownedUnsafeAllowViolations,
  workspaceLintViolations,
} from "./check-unsafe-inventory.mjs";

const INVENTORY = `## Retained boundary inventory

**Owned module:** \`crates/a/src/ffi.rs\`.

**Owned modules:** \`crates/b/src/platform/\` (\`windows.rs\`) and the Windows portion of \`crates/b/src/lib.rs\`.

Not an owner line: \`crates/c/src/other.rs\`.
`;

const ALLOW = '#![allow(unsafe_code, reason = "see inventory")]';

describe("unsafe inventory check", () => {
  it("reads owned files and directories only from owned module lines", () => {
    expect(inventoryPaths(INVENTORY)).toEqual(["crates/a/src/ffi.rs", "crates/b/src/platform/", "crates/b/src/lib.rs"]);
  });

  it("accepts unsafe allowances only inside owned modules", () => {
    const owned = inventoryPaths(INVENTORY);
    const files = [
      { path: "crates/a/src/ffi.rs", text: ALLOW },
      { path: "crates/b/src/platform/windows.rs", text: `use x;\n${ALLOW}` },
      { path: "crates/c/src/other.rs", text: `fn safe() {}\n#[allow(dead_code, unsafe_code)]\nfn f() {}` },
      { path: "crates/c/src/expect.rs", text: "#[expect(unsafe_code)]" },
      { path: "crates/c/src/warn.rs", text: "#![warn( unsafe_code )]" },
      { path: "crates/c/src/clean.rs", text: "#![deny(unsafe_code)]\n// unsafe_code is denied" },
    ];
    expect(unownedUnsafeAllowViolations(files, owned)).toEqual([
      "crates/c/src/other.rs:2 relaxes unsafe_code outside the inventory",
      "crates/c/src/expect.rs:1 relaxes unsafe_code outside the inventory",
      "crates/c/src/warn.rs:1 relaxes unsafe_code outside the inventory",
    ]);
  });

  it("requires the workspace deny and plain inheritance in every member", () => {
    const workspace = '[workspace.lints.rust]\nunsafe_code = "deny"\n\n[workspace.lints.clippy]\n';
    const inherits = { path: "crates/a/Cargo.toml", text: '[package]\nname = "a"\n\n[lints]\nworkspace = true\n' };
    expect(workspaceLintViolations(workspace, [inherits])).toEqual([]);

    expect(workspaceLintViolations('[workspace.lints.rust]\nunsafe_code = "warn"\n', [inherits])).toHaveLength(1);
    expect(workspaceLintViolations('[workspace.lints.clippy]\nunsafe_code = "deny"\n', [inherits])).toHaveLength(1);
    expect(
      workspaceLintViolations(workspace, [
        { path: "crates/b/Cargo.toml", text: '[package]\nname = "b"\n' },
        { path: "crates/c/Cargo.toml", text: '[lints.rust]\nunsafe_code = "allow"\n' },
        { path: "crates/d/Cargo.toml", text: "[lints]\nworkspace = false\n" },
      ]),
    ).toEqual([
      "crates/b/Cargo.toml must inherit workspace lints with only [lints] workspace = true",
      "crates/c/Cargo.toml must inherit workspace lints with only [lints] workspace = true",
      "crates/d/Cargo.toml must inherit workspace lints with only [lints] workspace = true",
    ]);
  });

  it("reports inventory entries that no longer exist", () => {
    const present = new Set(["crates/a/src/ffi.rs", "crates/b/src/platform/"]);
    expect(staleInventoryViolations(inventoryPaths(INVENTORY), (entry) => present.has(entry))).toEqual([
      "docs/data/security/unsafe-rust.md names missing path crates/b/src/lib.rs",
    ]);
  });
});
