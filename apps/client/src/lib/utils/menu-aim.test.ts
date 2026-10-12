import { describe, expect, it } from "vitest";
import { isPointerAimingAtSubmenu } from "./menu-aim";

describe("menu aim", () => {
  it("keeps a right-side submenu open for diagonal movement into it", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 420, y: 160 },
      point: { x: 455, y: 205 },
      submenu: { left: 460, right: 720, top: 140, bottom: 420 },
      side: "right",
    })).toBe(true);
  });

  it("keeps a right-side submenu open when movement starts near the row center", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 280, y: 160 },
      point: { x: 360, y: 205 },
      submenu: { left: 460, right: 720, top: 140, bottom: 420 },
      side: "right",
      topTolerance: 8,
      bottomTolerance: 32,
    })).toBe(true);
  });

  it("does not keep a right-side submenu open for vertical movement through rows", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 420, y: 160 },
      point: { x: 422, y: 230 },
      submenu: { left: 460, right: 720, top: 140, bottom: 420 },
      side: "right",
    })).toBe(false);
  });

  it("does not keep a right-side submenu open when the pointer leaves the corridor", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 420, y: 160 },
      point: { x: 455, y: 470 },
      submenu: { left: 460, right: 720, top: 140, bottom: 420 },
      side: "right",
    })).toBe(false);
  });

  it("allows a wider bottom corridor without widening the top", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 420, y: 160 },
      point: { x: 470, y: 435 },
      submenu: { left: 460, right: 720, top: 140, bottom: 420 },
      side: "right",
      topTolerance: 8,
      bottomTolerance: 32,
    })).toBe(true);

    expect(isPointerAimingAtSubmenu({
      origin: { x: 420, y: 160 },
      point: { x: 470, y: 120 },
      submenu: { left: 460, right: 720, top: 140, bottom: 420 },
      side: "right",
      topTolerance: 8,
      bottomTolerance: 32,
    })).toBe(false);
  });

  it("keeps a left-side submenu open for diagonal movement into it", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 480, y: 160 },
      point: { x: 440, y: 205 },
      submenu: { left: 140, right: 430, top: 140, bottom: 420 },
      side: "left",
    })).toBe(true);
  });

  it("keeps an upper submenu open for movement toward it", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 300, y: 400 },
      point: { x: 340, y: 350 },
      submenu: { left: 240, right: 520, top: 100, bottom: 330 },
      side: "top",
    })).toBe(true);
  });

  it("rejects sideways movement when the submenu is above", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 300, y: 400 },
      point: { x: 390, y: 398 },
      submenu: { left: 240, right: 520, top: 100, bottom: 330 },
      side: "top",
    })).toBe(false);
  });

  it("keeps a lower submenu open for movement toward it", () => {
    expect(isPointerAimingAtSubmenu({
      origin: { x: 300, y: 160 },
      point: { x: 340, y: 215 },
      submenu: { left: 240, right: 520, top: 200, bottom: 430 },
      side: "bottom",
    })).toBe(true);
  });
});
