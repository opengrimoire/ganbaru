import type { Component } from "svelte";

let stepPromise: Promise<Component> | null = null;

/**
 * Loads the coding agents welcome screen once, so the screen before it can start the load. Mobile builds resolve this
 * module to one that rejects, which keeps provider setup out of their bundles.
 */
export function loadWelcomeAgentsStep(): Promise<Component> {
  stepPromise ??= import("$lib/components/vault/welcome/WelcomeAgentsStep.svelte")
    .then((module) => module.default)
    .catch((error: unknown) => {
      stepPromise = null;
      throw error;
    });
  return stepPromise;
}
