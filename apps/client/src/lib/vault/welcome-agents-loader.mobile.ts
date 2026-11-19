import type { Component } from "svelte";

/** Rejects on mobile, where the tour has no coding agents screen, so provider setup stays outside mobile bundles. */
export function loadWelcomeAgentsStep(): Promise<Component> {
  return Promise.reject(new Error("The coding agents welcome screen is unavailable on mobile"));
}
