import { describe, expect, it } from "vitest";
import { lazyLoadCanStart, type LazyComponentLoadState } from "./lazy-component-loader";

type State = LazyComponentLoadState<"editor", string>;

const loading: State = { key: "editor", requestId: 1, status: "loading" };
const ready: State = { key: "editor", requestId: 1, status: "ready", component: "Editor" };
const failed: State = { key: "editor", requestId: 1, status: "failed", error: new Error("chunk missing") };

describe("lazyLoadCanStart", () => {
  it("starts a first load on its own", () => {
    expect(lazyLoadCanStart(null)).toBe(true);
  });

  it("never starts a load on its own after one failed, so a failing import does not loop", () => {
    expect(lazyLoadCanStart(failed)).toBe(false);
  });

  it("starts again after a failure only when the person retries", () => {
    expect(lazyLoadCanStart(failed, true)).toBe(true);
  });

  it("never starts while a load runs or once the component is ready", () => {
    for (const retry of [false, true]) {
      expect(lazyLoadCanStart(loading, retry)).toBe(false);
      expect(lazyLoadCanStart(ready, retry)).toBe(false);
    }
  });
});
