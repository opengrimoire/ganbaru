import { describe, expect, it } from "vitest";
import {
  canBootstrapUntouchedVault,
  completeVaultHandoffOnboarding,
  formatPairingCountdown,
  markIndependentVaultUsed,
  vaultHandoffOnboardingCompleted,
} from "./handoff-onboarding";

describe("vault handoff onboarding state", () => {
  it("persists completion only for the active vault", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };

    expect(vaultHandoffOnboardingCompleted(storage, "vault-a")).toBe(false);
    completeVaultHandoffOnboarding(storage, "vault-a");
    expect(vaultHandoffOnboardingCompleted(storage, "vault-a")).toBe(true);
    expect(vaultHandoffOnboardingCompleted(storage, "vault-b")).toBe(false);
    completeVaultHandoffOnboarding(storage, "vault-b");
    expect(vaultHandoffOnboardingCompleted(storage, "vault-a")).toBe(true);
    expect(vaultHandoffOnboardingCompleted(storage, "vault-b")).toBe(true);
  });

  it("does not treat unscoped legacy onboarding state as completion", () => {
    const storage = {
      getItem: (key: string) => key === "ganbaru.vault-handoff-onboarding.v2" ? "complete" : null,
    };

    expect(vaultHandoffOnboardingCompleted(storage, "vault-a")).toBe(false);
  });

  it("protects the independent vault after onboarding is skipped", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };

    expect(canBootstrapUntouchedVault(storage)).toBe(true);
    markIndependentVaultUsed(storage);
    expect(canBootstrapUntouchedVault(storage)).toBe(false);
    expect(vaultHandoffOnboardingCompleted(storage, "vault-a")).toBe(false);
  });

  it("formats invitation expiry without rounding down early", () => {
    expect(formatPairingCountdown(180_000)).toBe("3:00");
    expect(formatPairingCountdown(300_000)).toBe("5:00");
    expect(formatPairingCountdown(299_001)).toBe("5:00");
    expect(formatPairingCountdown(299_000)).toBe("4:59");
    expect(formatPairingCountdown(-1)).toBe("0:00");
  });
});
