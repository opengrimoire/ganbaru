import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { noticeCode, resolveNoticeTitle } from "./notice-title";

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

describe("resolveNoticeTitle", () => {
  it("returns the catalog title for a known code in both languages", () => {
    expect(resolveNoticeTitle("claude_api_retry", "Provider text", tEn)).toBe("Claude is retrying the API request");
    expect(resolveNoticeTitle("claude_api_retry", "Provider text", tEs)).toBe("Claude está reintentando la solicitud a la API");
    expect(resolveNoticeTitle("streamed_text_limit", "", tEs)).toBe("Se recortó el texto del proveedor");
  });

  it("keeps the provider title for an unknown or interpolated code", () => {
    expect(resolveNoticeTitle("codex_runtime_warning", "Sandbox degraded", tEs)).toBe("Sandbox degraded");
    expect(resolveNoticeTitle("acp_host_tools_unavailable", "Grok cannot use host tools", tEn)).toBe("Grok cannot use host tools");
  });

  it("does not treat inherited object keys as known codes", () => {
    expect(resolveNoticeTitle("toString", "Provider title", tEn)).toBe("Provider title");
  });

  it("falls back to the generic notice label when the provider title is empty", () => {
    expect(resolveNoticeTitle("future_code", "   ", tEn)).toBe("Notice");
    expect(resolveNoticeTitle("future_code", "", tEs)).toBe("Aviso");
  });
});

describe("noticeCode", () => {
  it("reads a non-empty string code and ignores other metadata shapes", () => {
    expect(noticeCode({ code: "claude_api_retry" })).toBe("claude_api_retry");
    expect(noticeCode({ code: "" })).toBeNull();
    expect(noticeCode({ code: 3 })).toBeNull();
    expect(noticeCode(["code"])).toBeNull();
    expect(noticeCode(null)).toBeNull();
  });
});
