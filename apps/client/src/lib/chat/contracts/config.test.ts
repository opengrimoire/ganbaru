import { describe, expect, it, vi } from "vitest";
import { CHAT_BEHAVIOR_DEFAULTS, CHAT_DIAGNOSTIC_RETENTION_DEFAULT_DAYS } from "./config";

// Load test-only Node APIs without introducing Node globals into the browser type environment.
const { readFileSync } = await vi.importActual<{
  readFileSync: (path: string, encoding: "utf8") => string;
}>("node:fs");
const { fileURLToPath, URL: NodeUrl } = await vi.importActual<{
  URL: typeof URL;
  fileURLToPath: (url: string) => string;
}>("node:url");

function readRepositoryFile(relativePath: string): string {
  return readFileSync(fileURLToPath(new NodeUrl(relativePath, import.meta.url).href), "utf8");
}

const contractsConfig = readRepositoryFile("../../../../../../crates/ganbaru-chat-contracts/src/config.rs");
const deviceState = readRepositoryFile("../../../../src-tauri/app/src/chat/device_state.rs");

/** The body of one Rust function, so assertions read only its literals. */
function rustFunctionBody(source: string, signature: string): string {
  const start = source.indexOf(signature);
  if (start < 0) throw new Error(`Missing Rust function: ${signature}`);
  const end = source.indexOf("\n}\n", start);
  return source.slice(start, end);
}

function rustNumber(literal: string): number {
  return Number(literal.replaceAll("_", ""));
}

/** The variant marked `#[default]` in a Rust enum, in its serde snake_case wire form. */
function rustDefaultVariant(source: string, enumName: string): string {
  const body = rustFunctionBody(source, `pub enum ${enumName} {`);
  const variant = /#\[default\]\s*(\w+)/.exec(body)?.[1];
  if (!variant) throw new Error(`Missing default variant for ${enumName}`);
  return variant.replace(/(?<!^)([A-Z])/g, "_$1").toLowerCase();
}

describe("Chat behavior defaults", () => {
  it("match the Rust behavior defaults", () => {
    const defaults = rustFunctionBody(contractsConfig, "impl Default for ChatBehaviorPreferences {");
    const booleans: Array<[keyof typeof CHAT_BEHAVIOR_DEFAULTS, string]> = [
      ["restoreLastSelectedThread", "restore_last_selected_thread"],
      ["showReasoningSummaries", "show_reasoning_summaries"],
      ["automaticallyFoldSettledWork", "automatically_fold_settled_work"],
      ["confirmMultilineTerminalPaste", "confirm_multiline_terminal_paste"],
      ["notifyWaitingForYou", "notify_waiting_for_you"],
      ["notifyReviewReady", "notify_review_ready"],
      ["notifyFailures", "notify_failures"],
      ["announceBackgroundWork", "announce_background_work"],
      ["notificationIncludeContent", "notification_include_content"],
    ];
    for (const [field, rustField] of booleans) {
      const value = new RegExp(`${rustField}: (true|false),`).exec(defaults)?.[1];
      expect(value, rustField).toBe(String(CHAT_BEHAVIOR_DEFAULTS[field]));
    }

    const scrollback = /^\s*([\d_]+)\s*$/m.exec(rustFunctionBody(contractsConfig, "fn default_terminal_scrollback_lines()"))?.[1];
    const idle = /^\s*([\d_]+)\s*$/m.exec(rustFunctionBody(contractsConfig, "fn default_idle_session_timeout_seconds()"))?.[1];
    expect(rustNumber(scrollback ?? "")).toBe(CHAT_BEHAVIOR_DEFAULTS.terminalScrollbackLines);
    expect(rustNumber(idle ?? "")).toBe(CHAT_BEHAVIOR_DEFAULTS.idleSessionTimeoutSeconds);
    expect(rustDefaultVariant(contractsConfig, "ChatSendKey")).toBe(CHAT_BEHAVIOR_DEFAULTS.sendKey);
    expect(rustDefaultVariant(contractsConfig, "ChatTimelineDensity")).toBe(CHAT_BEHAVIOR_DEFAULTS.timelineDensity);
  });

  it("match the Rust diagnostic retention default", () => {
    const days = /DEFAULT_DIAGNOSTIC_RETENTION_DAYS: u16 = (\d+);/.exec(deviceState)?.[1];
    expect(Number(days)).toBe(CHAT_DIAGNOSTIC_RETENTION_DEFAULT_DAYS);
  });
});
