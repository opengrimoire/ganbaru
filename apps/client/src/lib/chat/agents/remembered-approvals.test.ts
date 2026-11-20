import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { rememberedApprovalDescription, rememberedApprovalSubject } from "./remembered-approvals";

const t: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;
describe("remembered approval presentation", () => {
  it("shows a web origin by its host, keeping a port", () => {
    expect(rememberedApprovalSubject({ kind: "url_origin", key: "https://docs.example.org" })).toBe("docs.example.org");
    expect(rememberedApprovalSubject({ kind: "url_origin", key: "http://docs.example.org:8080" })).toBe("docs.example.org:8080");
  });

  it("shows an origin that does not parse as stored, and commands and tools exactly", () => {
    expect(rememberedApprovalSubject({ kind: "url_origin", key: "not a url" })).toBe("not a url");
    expect(rememberedApprovalSubject({ kind: "command", key: "pnpm test" })).toBe("pnpm test");
    expect(rememberedApprovalSubject({ kind: "tool", key: "WebFetch" })).toBe("WebFetch");
  });

  it("describes each rule in one sentence", () => {
    expect(rememberedApprovalDescription(t, { kind: "url_origin", key: "https://docs.example.org" }))
      .toBe("Always allow pages from docs.example.org in this channel");
    expect(rememberedApprovalDescription(t, { kind: "command", key: "pnpm test" })).toBe("Always allow `pnpm test` in this channel");
    expect(rememberedApprovalDescription(t, { kind: "tool", key: "WebFetch" })).toBe("Always allow this tool in this channel");
    expect(rememberedApprovalDescription(tEs, { kind: "url_origin", key: "https://docs.example.org" }))
      .toBe("Permitir siempre las páginas de docs.example.org en este canal");
  });
});
