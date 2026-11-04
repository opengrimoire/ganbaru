import { describe, expect, it } from "vitest";
import appCommandPermissions from "../../../../src-tauri/permissions/app-commands.toml?raw";

const SOURCES = import.meta.glob<string>(["/src/**/*.ts", "/src/**/*.svelte", "!/src/**/*.test.ts"], {
  eager: true,
  query: "?raw",
  import: "default",
});

/** Overlay components plus the modules `main-desktop.ts` runs before it branches into an overlay. */
const OVERLAY_ROOTS = [
  "/src/lib/components/pomodoro/PomodoroOverlayWindow.svelte",
  "/src/lib/components/pomodoro/PomodoroOverlayBlocker.svelte",
  "/src/lib/platform.ts",
  "/src/lib/modal-focus.ts",
  "/src/lib/i18n/translator.svelte.ts",
  "/src/lib/i18n/pre-vault-language.ts",
  "/src/lib/i18n/locales.ts",
  "/src/lib/windows/desktop-readiness.ts",
  "/src/lib/stores/perf-log.svelte.ts",
  "/src/lib/pomodoro/blocked-screen.ts",
];

const IMPORT_PATTERN =
  /(?:import|export)\s[^"'`;]*?from\s*["']([^"']+)["']|import\s*["']([^"']+)["']|import\(\s*["']([^"']+)["']\s*\)/gu;
const LITERAL_INVOKE_PATTERN = /\binvoke\w*\s*(?:<[^()]*?>)?\s*\(\s*["'`]([^"'`$]+)["'`]/gu;
const DYNAMIC_INVOKE_PATTERN = /\binvoke\s*(?:<[^()]*?>)?\s*\(\s*([^"'`\s][^,)]*)/gu;

function normalizePath(value: string): string {
  const parts: string[] = [];
  for (const part of value.split("/")) {
    if (part === "..") parts.pop();
    else if (part !== "." && part !== "") parts.push(part);
  }
  return `/${parts.join("/")}`;
}

function resolveImport(from: string, specifier: string): string | null {
  let base: string;
  if (specifier.startsWith("$lib/")) base = `/src/lib/${specifier.slice("$lib/".length)}`;
  else if (specifier.startsWith(".")) base = normalizePath(`${from.slice(0, from.lastIndexOf("/"))}/${specifier}`);
  else return null;
  for (const candidate of [base, `${base}.ts`, `${base}.svelte.ts`, `${base}.js`, `${base}/index.ts`]) {
    if (candidate in SOURCES) return candidate;
  }
  return null;
}

/** Walks the static and dynamic imports reachable from the overlay roots and collects invoked commands. */
function reachableCommands(): { commands: Set<string>; dynamicCalls: string[]; files: number } {
  const seen = new Set<string>();
  const commands = new Set<string>();
  const dynamicCalls: string[] = [];
  const pending = [...OVERLAY_ROOTS];
  while (pending.length > 0) {
    const file = pending.pop();
    if (file === undefined || seen.has(file)) continue;
    const text = SOURCES[file];
    if (text === undefined) throw new Error(`Missing overlay source ${file}`);
    seen.add(file);
    for (const match of text.matchAll(IMPORT_PATTERN)) {
      const specifier = match[1] ?? match[2] ?? match[3];
      const resolved = specifier === undefined ? null : resolveImport(file, specifier);
      if (resolved !== null) pending.push(resolved);
    }
    for (const match of text.matchAll(LITERAL_INVOKE_PATTERN)) {
      if (match[1] !== undefined) commands.add(match[1]);
    }
    for (const match of text.matchAll(DYNAMIC_INVOKE_PATTERN)) {
      dynamicCalls.push(`${file}: ${match[1] ?? ""}`);
    }
  }
  return { commands, dynamicCalls, files: seen.size };
}

function permissionCommands(identifier: string): Set<string> {
  const start = appCommandPermissions.indexOf(`identifier = "${identifier}"`);
  if (start < 0) throw new Error(`Missing permission ${identifier}`);
  const listStart = appCommandPermissions.indexOf("commands.allow = [", start);
  const listEnd = appCommandPermissions.indexOf("]", listStart);
  return new Set(
    [...appCommandPermissions.slice(listStart, listEnd).matchAll(/"([^"]+)"/gu)].map((match) => match[1] ?? ""),
  );
}

describe("Focus overlay command permission", () => {
  it("allows exactly the commands overlay windows can reach", () => {
    const { commands, dynamicCalls, files } = reachableCommands();
    expect(files).toBeGreaterThan(OVERLAY_ROOTS.length);
    expect(dynamicCalls).toEqual([]);
    expect([...commands].sort()).toEqual([...permissionCommands("focus-overlay-commands")].sort());
  });

  it("keeps overlay commands inside the full app command set", () => {
    const all = permissionCommands("app-commands");
    for (const command of permissionCommands("focus-overlay-commands")) expect(all.has(command)).toBe(true);
  });
});
