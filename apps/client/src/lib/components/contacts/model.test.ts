import { describe, expect, it } from "vitest";
import {
  CONTACT_CARD_MATRIX_SIZE,
  CONTACT_CARD_PLACEHOLDER_LENGTH,
  MEMBER_CAPABILITIES,
  capabilitiesForRole,
  contactCardPlaceholderCode,
  placeholderCardMatrix,
} from "./model";

describe("capabilitiesForRole", () => {
  it("nests the presets so each role narrows the one above it", () => {
    const owner = capabilitiesForRole("owner");
    const administrator = capabilitiesForRole("administrator");
    const member = capabilitiesForRole("member");
    const guest = capabilitiesForRole("guest");

    expect([...owner].sort()).toEqual(MEMBER_CAPABILITIES.map((capability) => capability.id).sort());
    expect([...administrator]).toEqual([...owner]);
    for (const capability of member) expect(administrator.has(capability)).toBe(true);
    for (const capability of guest) expect(member.has(capability)).toBe(true);
    expect(guest.size).toBeLessThan(member.size);
    expect(member.size).toBeLessThan(administrator.size);
  });

  it("keeps management capabilities away from members and guests", () => {
    const management = MEMBER_CAPABILITIES.filter((capability) => capability.group === "management");
    expect(management.length).toBeGreaterThan(0);
    for (const capability of management) {
      expect(capabilitiesForRole("member").has(capability.id)).toBe(false);
      expect(capabilitiesForRole("guest").has(capability.id)).toBe(false);
    }
  });

  it("prefills a custom role from the member preset", () => {
    expect([...capabilitiesForRole("custom")]).toEqual([...capabilitiesForRole("member")]);
  });
});

describe("contactCardPlaceholderCode", () => {
  it("is stable base64url text without the real code prefix", () => {
    const placeholder = contactCardPlaceholderCode();
    expect(placeholder).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(placeholder).toHaveLength(CONTACT_CARD_PLACEHOLDER_LENGTH);
    expect(placeholder.startsWith("GANB-")).toBe(false);
    expect(placeholder).toBe(contactCardPlaceholderCode());
  });

  it("mixes letter cases and digits like a real code", () => {
    const placeholder = contactCardPlaceholderCode();
    expect(placeholder).toMatch(/[A-Z]/);
    expect(placeholder).toMatch(/[a-z]/);
    expect(placeholder).toMatch(/[0-9]/);
  });
});

describe("placeholderCardMatrix", () => {
  it("fills a square grid with finder patterns in three corners", () => {
    const { modules, width } = placeholderCardMatrix();
    const at = (row: number, column: number) => modules[row * width + column];

    expect(width).toBe(CONTACT_CARD_MATRIX_SIZE);
    expect(modules).toHaveLength(width * width);
    for (const [top, left] of [[0, 0], [0, width - 7], [width - 7, 0]]) {
      expect(at(top, left)).toBe(true);
      expect(at(top + 1, left + 1)).toBe(false);
      expect(at(top + 3, left + 3)).toBe(true);
      expect(at(top + 6, left + 6)).toBe(true);
    }
    expect(at(7, 7)).toBe(false);
  });

  it("is stable and mixes dark and light modules in the data area", () => {
    const first = placeholderCardMatrix().modules;
    expect(first).toEqual(placeholderCardMatrix().modules);
    expect(first.some(Boolean)).toBe(true);
    expect(first.some((module) => !module)).toBe(true);
  });
});
