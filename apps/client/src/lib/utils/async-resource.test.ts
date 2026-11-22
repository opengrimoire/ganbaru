import { describe, expect, it } from "vitest";
import {
  abandonAsyncRequest,
  asyncResourceError,
  failAsyncRequest,
  idleAsyncResource,
  isAsyncResourceLoading,
  isCurrentAsyncRequest,
  resolveAsyncRequest,
  startAsyncRequest,
  updateAsyncResourceData,
  type AsyncResource,
} from "./async-resource";

function failed(data: string[], error: string): AsyncResource<string[]> {
  const { resource, token } = startAsyncRequest(idleAsyncResource(data));
  return failAsyncRequest(resource, token, error);
}

describe("async resource", () => {
  it("starts idle with its data and no error", () => {
    const resource = idleAsyncResource(["a"]);
    expect(resource).toEqual({ status: "idle", data: ["a"] });
    expect(isAsyncResourceLoading(resource)).toBe(false);
    expect(asyncResourceError(resource)).toBeNull();
  });

  it("keeps the loaded data visible while a reload runs", () => {
    const { resource } = startAsyncRequest(idleAsyncResource(["a"]));
    expect(resource.status).toBe("loading");
    expect(resource.data).toEqual(["a"]);
    expect(isAsyncResourceLoading(resource)).toBe(true);
  });

  it("clears an earlier error when a request starts", () => {
    const { resource } = startAsyncRequest(failed(["a"], "offline"));
    expect(asyncResourceError(resource)).toBeNull();
    expect(resource.data).toEqual(["a"]);
  });

  it("gives every request a token no other request shares", () => {
    const first = startAsyncRequest(idleAsyncResource(0));
    const second = startAsyncRequest(first.resource);
    expect(first.token).not.toBe(second.token);
    expect(isCurrentAsyncRequest(second.resource, first.token)).toBe(false);
    expect(isCurrentAsyncRequest(second.resource, second.token)).toBe(true);
  });

  it("settles the current request with its data", () => {
    const { resource, token } = startAsyncRequest(idleAsyncResource(["a"]));
    expect(resolveAsyncRequest(resource, token, ["b"])).toEqual({ status: "idle", data: ["b"] });
  });

  it("ignores a response from a superseded request", () => {
    const first = startAsyncRequest(idleAsyncResource(["a"]));
    const second = startAsyncRequest(first.resource);
    expect(resolveAsyncRequest(second.resource, first.token, ["stale"])).toBe(second.resource);
    expect(failAsyncRequest(second.resource, first.token, "stale")).toBe(second.resource);
    expect(abandonAsyncRequest(second.resource, first.token)).toBe(second.resource);
  });

  it("lets the newer request settle after an older one answered late", () => {
    const first = startAsyncRequest(idleAsyncResource(["a"]));
    const second = startAsyncRequest(first.resource);
    const afterStale = resolveAsyncRequest(second.resource, first.token, ["stale"]);
    expect(resolveAsyncRequest(afterStale, second.token, ["fresh"])).toEqual({ status: "idle", data: ["fresh"] });
  });

  it("ignores any response once a reset replaced the resource", () => {
    const { token } = startAsyncRequest(idleAsyncResource(["a"]));
    const reset = idleAsyncResource<string[]>([]);
    expect(resolveAsyncRequest(reset, token, ["late"])).toBe(reset);
    expect(failAsyncRequest(reset, token, "late")).toBe(reset);
  });

  it("ignores a second response for a request that already settled", () => {
    const { resource, token } = startAsyncRequest(idleAsyncResource(["a"]));
    const settled = resolveAsyncRequest(resource, token, ["b"]);
    expect(resolveAsyncRequest(settled, token, ["c"])).toBe(settled);
    expect(failAsyncRequest(settled, token, "late")).toBe(settled);
  });

  it("keeps the loaded data when a request fails", () => {
    const resource = failed(["a"], "offline");
    expect(resource).toEqual({ status: "failed", data: ["a"], error: "offline" });
    expect(asyncResourceError(resource)).toBe("offline");
    expect(isAsyncResourceLoading(resource)).toBe(false);
  });

  it("replaces the data on failure when the caller asks", () => {
    const { resource, token } = startAsyncRequest(idleAsyncResource(["a"]));
    expect(failAsyncRequest(resource, token, "offline", [])).toEqual({ status: "failed", data: [], error: "offline" });
  });

  it("ends an abandoned request with the data it had", () => {
    const { resource, token } = startAsyncRequest(idleAsyncResource(["a"]));
    expect(abandonAsyncRequest(resource, token)).toEqual({ status: "idle", data: ["a"] });
  });

  it("updates the data in place without ending a request in flight", () => {
    const first = startAsyncRequest(idleAsyncResource(["a"]));
    const updated = updateAsyncResourceData(first.resource, (data) => [...data, "b"]);
    expect(updated.data).toEqual(["a", "b"]);
    expect(isCurrentAsyncRequest(updated, first.token)).toBe(true);
    expect(resolveAsyncRequest(updated, first.token, ["c"])).toEqual({ status: "idle", data: ["c"] });
  });

  it("keeps an error while the data is updated in place", () => {
    const updated = updateAsyncResourceData(failed(["a"], "offline"), (data) => data.slice(1));
    expect(updated).toEqual({ status: "failed", data: [], error: "offline" });
  });
});
