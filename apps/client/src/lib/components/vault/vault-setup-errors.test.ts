import { describe, expect, it } from "vitest";
import {
  NO_VAULT_SETUP_ERRORS,
  recordVaultSetupFailure,
  vaultSetupErrorText,
  type VaultSetupFailure,
} from "./vault-setup-errors";

const format = (failure: VaultSetupFailure): string => `${failure.action}: ${String(failure.raw)}.`;

describe("vault setup errors", () => {
  it("shows nothing without failures", () => {
    expect(vaultSetupErrorText(NO_VAULT_SETUP_ERRORS, format)).toBeNull();
  });

  it("keeps the startup failure when loading the default location fails afterwards", () => {
    const startup = recordVaultSetupFailure(NO_VAULT_SETUP_ERRORS, "attempt", { raw: "locked", action: "startup" });
    const both = recordVaultSetupFailure(startup, "location", { raw: "no documents folder", action: "general" });

    expect(vaultSetupErrorText(both, format)).toBe("startup: locked. general: no documents folder.");
  });

  it("keeps the location failure when a later attempt fails", () => {
    const location = recordVaultSetupFailure(NO_VAULT_SETUP_ERRORS, "location", { raw: "no documents folder", action: "general" });
    const both = recordVaultSetupFailure(location, "attempt", { raw: "denied", action: "change" });

    expect(vaultSetupErrorText(both, format)).toBe("change: denied. general: no documents folder.");
  });

  it("replaces the startup failure with a later attempt's failure", () => {
    const startup = recordVaultSetupFailure(NO_VAULT_SETUP_ERRORS, "attempt", { raw: "locked", action: "startup" });
    const retried = recordVaultSetupFailure(startup, "attempt", { raw: "denied", action: "import" });

    expect(vaultSetupErrorText(retried, format)).toBe("import: denied.");
  });
});
