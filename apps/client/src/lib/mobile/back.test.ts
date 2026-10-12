import { describe, expect, it } from "vitest";
import {
  MobileBackListenerController,
  resolveMobileBackAction,
  type MobileBackListener,
} from "./back";

describe("resolveMobileBackAction", () => {
  it("closes the active visual layer before routes and destinations", () => {
    expect(resolveMobileBackAction({
      featureLayerOpen: true,
      nestedRouteOpen: true,
      currentView: "notes",
    })).toBe("consume-feature-layer");
  });

  it("closes nested routes before changing destination", () => {
    expect(resolveMobileBackAction({
      featureLayerOpen: false,
      nestedRouteOpen: true,
      currentView: "projects",
    })).toBe("close-nested-route");
  });

  it("returns to Calendar before releasing Back to Android at the root", () => {
    expect(resolveMobileBackAction({
      featureLayerOpen: false,
      nestedRouteOpen: false,
      currentView: "notes",
    })).toBe("navigate-calendar");
    expect(resolveMobileBackAction({
      featureLayerOpen: false,
      nestedRouteOpen: false,
      currentView: "calendar",
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
