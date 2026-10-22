import { describe, expect, it } from "vitest";
import {
  applyTerminalOutput,
  boundTerminalContext,
  sanitizeTerminalPaste,
  TerminalOutputAcknowledger,
  TerminalPanelRegistry,
  terminalErrorMessage,
  terminalLinkTarget,
  terminalOutputByteLength,
  terminalPasteNeedsConfirmation,
} from "./terminal-model";

function chunk(generation: number, sequence: number, text: string) {
  return {
    terminalId: "terminal",
    generation,
    sequence,
    dataBase64: btoa(text),
    replay: false,
  };
}

describe("Chat terminal model", () => {
  it("accepts contiguous output and detects duplicates and replay gaps", () => {
    const initial = { generation: 1, lastSequence: 0 };
    const accepted = applyTerminalOutput(initial, chunk(1, 1, "hello"));
    expect(accepted.kind).toBe("accept");
    if (accepted.kind !== "accept") throw new Error("expected accepted output");
    expect(new TextDecoder().decode(accepted.bytes)).toBe("hello");
    expect(applyTerminalOutput(accepted.state, chunk(1, 1, "hello")).kind).toBe("duplicate");
    expect(applyTerminalOutput(accepted.state, chunk(1, 3, "later")).kind).toBe("gap");
    expect(applyTerminalOutput(accepted.state, chunk(2, 1, "restart")).kind).toBe("accept");
  });

  it("requires confirmation only for multiline paste when enabled", () => {
    expect(terminalPasteNeedsConfirmation("pnpm test", true)).toBe(false);
    expect(terminalPasteNeedsConfirmation("pnpm test\npnpm check", true)).toBe(true);
    expect(terminalPasteNeedsConfirmation("pnpm test\npnpm check", false)).toBe(false);
  });

  it("removes escape and control characters from pasted text", () => {
    expect(sanitizeTerminalPaste("echo safe\x1b[201~rm -rf ~\n")).toEqual({
      text: "echo safe[201~rm -rf ~\n",
      removedControls: true,
    });
    expect(sanitizeTerminalPaste("a\x1b[20\x1b[201~1~b").text).not.toContain("\x1b");
    expect(sanitizeTerminalPaste("a\u009b201~b\u0007\u007f").text).toBe("a201~b");
    expect(sanitizeTerminalPaste("col\tone\r\nnext é")).toEqual({
      text: "col\tone\r\nnext é",
      removedControls: false,
    });
  });

  it("bounds UTF-8 context without splitting a code point", () => {
    const result = boundTerminalContext("abécd", 4);
    expect(result.text).toBe("abé");
    expect(result.byteSize).toBe(4);
    expect(result.truncated).toBe(true);
    expect(boundTerminalContext("abécd", 3).text).toBe("ab");
  });

  it("reads structured terminal errors without exposing object coercion", () => {
    expect(terminalErrorMessage({
      code: "invalid_state_transition",
      message: "Chat terminal is not running",
      field: null,
      recoverable: true,
      details: null,
    }, "Terminal operation failed")).toBe("Chat terminal is not running");
    expect(terminalErrorMessage({
      message: "Terminal dimensions are outside the supported range",
      field: "dimensions",
    }, "Terminal operation failed")).toBe(
      "dimensions: Terminal dimensions are outside the supported range",
    );
    expect(terminalErrorMessage({ code: "internal" }, "Terminal operation failed")).toBe(
      "Terminal operation failed",
    );
  });

  it("keeps terminal collections and selections independent by panel", () => {
    const registry = new TerminalPanelRegistry();
    const first = { id: "first" };
    const second = { id: "second" };

    expect(registry.claimAvailable([first], "inspector")).toEqual([first]);
    expect(registry.claimAvailable([first], "bottom")).toEqual([]);

    registry.assign(second.id, "bottom");
    expect(registry.claimAvailable([first, second], "inspector")).toEqual([first]);
    expect(registry.claimAvailable([first, second], "bottom")).toEqual([second]);

    registry.select("thread", "inspector", first.id);
    registry.select("thread", "bottom", second.id);
    expect(registry.selected("thread", "inspector")).toBe(first.id);
    expect(registry.selected("thread", "bottom")).toBe(second.id);

    registry.release(first.id);
    expect(registry.claimAvailable([first, second], "bottom")).toEqual([first, second]);
  });
});

describe("terminal output flow control", () => {
  it("releases acknowledgements only once a batch reaches the interval", () => {
    const acknowledger = new TerminalOutputAcknowledger(10);
    expect(acknowledger.record(4)).toBe(0);
    expect(acknowledger.record(5)).toBe(0);
    expect(acknowledger.record(3)).toBe(12);
    expect(acknowledger.record(0)).toBe(0);
    expect(acknowledger.record(-5)).toBe(0);
    expect(acknowledger.record(10)).toBe(10);
  });

  it("measures decoded chunk size for every padding length", () => {
    for (const text of ["", "a", "ab", "abc", "abcd", "ÿ\u0000\u001b[0m"]) {
      expect(terminalOutputByteLength(chunk(1, 1, text))).toBe(text.length);
    }
  });
});

describe("terminal hyperlinks", () => {
  it("opens only credential-free web targets and shows the normalized target", () => {
    expect(terminalLinkTarget("https://example.com/docs?q=1")).toBe("https://example.com/docs?q=1");
    expect(terminalLinkTarget("http://localhost:5173")).toBe("http://localhost:5173/");
    expect(terminalLinkTarget("HTTPS://Example.COM/a")).toBe("https://example.com/a");
  });

  it("ignores schemes output could use to reach local files, apps, or scripts", () => {
    for (const uri of [
      "file:///etc/passwd",
      "javascript:alert(1)",
      "vscode://file/home/user/.bashrc",
      "smb://attacker.example/share",
      "data:text/html,<script>alert(1)</script>",
      "ssh://host",
    ]) {
      expect(terminalLinkTarget(uri)).toBeNull();
    }
  });

  it("ignores targets with embedded credentials or that do not parse", () => {
    expect(terminalLinkTarget("https://user:token@example.com/")).toBeNull();
    expect(terminalLinkTarget("https://token@example.com/")).toBeNull();
    expect(terminalLinkTarget("not a url")).toBeNull();
    expect(terminalLinkTarget("")).toBeNull();
  });
});
