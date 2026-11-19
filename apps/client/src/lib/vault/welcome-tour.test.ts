import { describe, expect, it } from "vitest";
import { completeWelcomeTour, markWelcomeTourPending, welcomeTourPending } from "./welcome-tour";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
  };
}

describe("welcome tour state", () => {
  it("shows the tour only for a vault the setup screens marked", () => {
    const storage = memoryStorage();
    expect(welcomeTourPending(storage, "vault-a")).toBe(false);
    markWelcomeTourPending(storage, "vault-a");
    expect(welcomeTourPending(storage, "vault-a")).toBe(true);
    expect(welcomeTourPending(storage, "vault-b")).toBe(false);
  });

  it("stops showing the tour once it completes", () => {
    const storage = memoryStorage();
    markWelcomeTourPending(storage, "vault-a");
    markWelcomeTourPending(storage, "vault-b");
    completeWelcomeTour(storage, "vault-a");
    expect(welcomeTourPending(storage, "vault-a")).toBe(false);
    expect(welcomeTourPending(storage, "vault-b")).toBe(true);
  });

  it("skips the tour when storage is missing or refuses access", () => {
    const refusing = {
      getItem: () => { throw new Error("denied"); },
      setItem: () => { throw new Error("denied"); },
      removeItem: () => { throw new Error("denied"); },
    };
    expect(welcomeTourPending(undefined, "vault-a")).toBe(false);
    expect(() => markWelcomeTourPending(refusing, "vault-a")).not.toThrow();
    expect(() => completeWelcomeTour(refusing, "vault-a")).not.toThrow();
    expect(welcomeTourPending(refusing, "vault-a")).toBe(false);
  });
});
