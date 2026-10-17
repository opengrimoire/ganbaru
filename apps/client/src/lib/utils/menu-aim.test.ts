import { describe, expect, it } from "vitest";
import {
  createHoverDelay,
  createHoverMenu,
  isPointerAimingAtSubmenu,
  SUBMENU_CLOSE_DELAY_MS,
  SUBMENU_OPEN_DELAY_MS,
  type HoverTimerScheduler,
} from "./menu-aim";

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

interface FakeClock {
  scheduler: HoverTimerScheduler;
  advance: (ms: number) => void;
}

function createFakeClock(): FakeClock {
  let now = 0;
  let timers: { at: number; action: () => void }[] = [];
  return {
    scheduler(action, delayMs) {
      const timer = { at: now + delayMs, action };
      timers.push(timer);
      return () => {
        timers = timers.filter((candidate) => candidate !== timer);
      };
    },
    advance(ms) {
      now += ms;
      const due = timers.filter((timer) => timer.at <= now);
      timers = timers.filter((timer) => timer.at > now);
      for (const timer of due) timer.action();
    },
  };
}

const mouse = { pointerType: "mouse" };

describe("hover delay", () => {
  it("runs only the latest scheduled action", () => {
    const clock = createFakeClock();
    const delay = createHoverDelay(clock.scheduler);
    const ran: string[] = [];

    delay.schedule(() => ran.push("close"));
    delay.schedule(() => ran.push("switch"));
    clock.advance(SUBMENU_CLOSE_DELAY_MS);

    expect(ran).toEqual(["switch"]);
  });

  it("drops a pending action when canceled within the grace period", () => {
    const clock = createFakeClock();
    const delay = createHoverDelay(clock.scheduler);
    const ran: string[] = [];

    delay.schedule(() => ran.push("close"));
    clock.advance(SUBMENU_CLOSE_DELAY_MS - 1);
    delay.cancel();
    clock.advance(SUBMENU_CLOSE_DELAY_MS);

    expect(ran).toEqual([]);
  });
});

describe("hover menu", () => {
  function setup() {
    const clock = createFakeClock();
    const state = { open: null as string | null };
    const menu = createHoverMenu({
      isOpen: () => state.open !== null,
      close: () => {
        state.open = null;
      },
      scheduler: clock.scheduler,
    });
    const opener = (key: string) => () => {
      state.open = key;
    };
    return { clock, state, menu, opener };
  }

  it("opens a resting trigger only after the open delay", () => {
    const { clock, state, menu, opener } = setup();

    menu.enterTrigger(mouse, opener("groups"));
    clock.advance(SUBMENU_OPEN_DELAY_MS - 1);
    expect(state.open).toBeNull();
    clock.advance(1);
    expect(state.open).toBe("groups");
  });

  it("does not open when the pointer passes over a trigger", () => {
    const { clock, state, menu, opener } = setup();

    menu.enterTrigger(mouse, opener("groups"));
    menu.leave();
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    expect(state.open).toBeNull();
  });

  it("switches to a sibling trigger at once while a panel shows", () => {
    const { clock, state, menu, opener } = setup();
    menu.enterTrigger(mouse, opener("groups"));
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    menu.leave();
    menu.enterTrigger(mouse, opener("projects"));
    expect(state.open).toBe("projects");
    clock.advance(SUBMENU_CLOSE_DELAY_MS);
    expect(state.open).toBe("projects");
  });

  it("closes a hover-opened panel after the pointer leaves trigger and panel", () => {
    const { clock, state, menu, opener } = setup();
    menu.enterTrigger(mouse, opener("groups"));
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    menu.leave();
    menu.enterPanel();
    clock.advance(SUBMENU_CLOSE_DELAY_MS);
    expect(state.open).toBe("groups");

    menu.leave();
    clock.advance(SUBMENU_CLOSE_DELAY_MS - 1);
    expect(state.open).toBe("groups");
    clock.advance(1);
    expect(state.open).toBeNull();
  });

  it("keeps a panel open after a click pins it", () => {
    const { clock, state, menu, opener } = setup();
    menu.enterTrigger(mouse, opener("groups"));
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    expect(menu.claimClick(true)).toBe(true);
    menu.leave();
    clock.advance(SUBMENU_CLOSE_DELAY_MS);

    expect(state.open).toBe("groups");
    expect(menu.claimClick(true)).toBe(false);
  });

  it("keeps a panel open after a press inside it", () => {
    const { clock, state, menu, opener } = setup();
    menu.enterTrigger(mouse, opener("groups"));
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    menu.pin();
    menu.leave();
    clock.advance(SUBMENU_CLOSE_DELAY_MS);

    expect(state.open).toBe("groups");
  });

  it("hover-opens without the pin left by an earlier click", () => {
    const { clock, state, menu, opener } = setup();
    menu.claimClick(false);
    opener("groups")();
    state.open = null;

    menu.enterTrigger(mouse, opener("projects"));
    clock.advance(SUBMENU_OPEN_DELAY_MS);
    menu.leave();
    clock.advance(SUBMENU_CLOSE_DELAY_MS);

    expect(state.open).toBeNull();
  });

  it("ignores touch pointers so a tap only clicks", () => {
    const { clock, state, menu, opener } = setup();

    menu.enterTrigger({ pointerType: "touch" }, opener("groups"));
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    expect(state.open).toBeNull();
  });

  it("drops a pending open when canceled", () => {
    const { clock, state, menu, opener } = setup();

    menu.enterTrigger(mouse, opener("groups"));
    menu.cancel();
    clock.advance(SUBMENU_OPEN_DELAY_MS);

    expect(state.open).toBeNull();
  });
});
