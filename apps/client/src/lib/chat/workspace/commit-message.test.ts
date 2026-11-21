import { describe, expect, it } from "vitest";
import { COMMIT_MESSAGE_MAX_BYTES, commitMessageTooLong, normalizeCommitMessage } from "./commit-message";

describe("commit message bound", () => {
  it("counts UTF-8 bytes rather than characters", () => {
    const ascii = "a".repeat(COMMIT_MESSAGE_MAX_BYTES);
    expect(commitMessageTooLong(ascii)).toBe(false);
    expect(commitMessageTooLong(`${ascii}a`)).toBe(true);
    const multibyte = "é".repeat(COMMIT_MESSAGE_MAX_BYTES / 2);
    expect(commitMessageTooLong(multibyte)).toBe(false);
    expect(commitMessageTooLong(`${multibyte}é`)).toBe(true);
    expect(commitMessageTooLong("界".repeat(1_400))).toBe(true);
    expect(commitMessageTooLong("😀".repeat(COMMIT_MESSAGE_MAX_BYTES / 4 + 1))).toBe(true);
  });

  it("ignores surrounding whitespace that committing trims", () => {
    const ascii = "a".repeat(COMMIT_MESSAGE_MAX_BYTES);
    expect(commitMessageTooLong(`  ${ascii}\n\n`)).toBe(false);
  });

  it("counts CRLF line endings as the LF the backend commits", () => {
    const lines = "ab\r\n".repeat(COMMIT_MESSAGE_MAX_BYTES / 3);
    expect(normalizeCommitMessage(lines)).toBe("ab\n".repeat(COMMIT_MESSAGE_MAX_BYTES / 3).trim());
    expect(commitMessageTooLong(lines)).toBe(false);
    expect(normalizeCommitMessage("one\rtwo")).toBe("one\ntwo");
  });

  it("keeps an empty message within the bound", () => {
    expect(commitMessageTooLong("")).toBe(false);
    expect(normalizeCommitMessage(" \n\t ")).toBe("");
  });
});
