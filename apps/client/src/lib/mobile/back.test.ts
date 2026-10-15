import { describe, expect, it } from "vitest";
import {
  MobileBackListenerController,
  resolveMobileBackAction,
  type MobileBackListener,
} from "./back";

describe("resolveMobileBackAction", () => {
  it("closes the active visual layer before routes and tabs", () => {
    expect(resolveMobileBackAction({
      featureLayerOpen: true,
      nestedRouteOpen: true,
      firstTabActive: false,
    })).toBe("consume-feature-layer");
  });

  it("closes nested routes before changing tabs", () => {
    expect(resolveMobileBackAction({
      featureLayerOpen: false,
      nestedRouteOpen: true,
      firstTabActive: false,
    })).toBe("close-nested-route");
  });

  it("returns to the first tab before releasing Back to Android from it", () => {
    expect(resolveMobileBackAction({
      featureLayerOpen: false,
      nestedRouteOpen: false,
      firstTabActive: false,
    })).toBe("activate-first-tab");
    expect(resolveMobileBackAction({
      featureLayerOpen: false,
      nestedRouteOpen: false,
      firstTabActive: true,
    })).toBe("release-to-system");
  });
});

describe("MobileBackListenerController", () => {
  it("registers only while the app can consume Back", async () => {
    let registrations = 0;
    let removals = 0;
    const listener: MobileBackListener = {
      async unregister() {
        removals += 1;
      },
    };
    const controller = new MobileBackListenerController(
      async () => {
        registrations += 1;
        return listener;
      },
      () => undefined,
      (error) => {
        throw error;
      },
    );

    await controller.setEnabled(false);
    expect(registrations).toBe(0);
    await controller.setEnabled(true);
    await controller.setEnabled(true);
    expect(registrations).toBe(1);
    await controller.setEnabled(false);
    expect(removals).toBe(1);
  });

  it("releases a listener created during a transition back to the root", async () => {
    let markRegistrationStarted!: () => void;
    let resolveRegistration!: (listener: MobileBackListener) => void;
    const registrationStarted = new Promise<void>((resolve) => {
      markRegistrationStarted = resolve;
    });
    const registration = new Promise<MobileBackListener>((resolve) => {
      resolveRegistration = resolve;
    });
    let removals = 0;
    const controller = new MobileBackListenerController(
      () => {
        markRegistrationStarted();
        return registration;
      },
      () => undefined,
      (error) => {
        throw error;
      },
    );

    const enabling = controller.setEnabled(true);
    await registrationStarted;
    const disabling = controller.setEnabled(false);
    resolveRegistration({
      async unregister() {
        removals += 1;
      },
    });
    await enabling;
    await disabling;

    expect(removals).toBe(1);
  });
});
