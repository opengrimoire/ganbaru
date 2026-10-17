import { describe, expect, it } from "vitest";
import { organizationalScrollFollowsEnd } from "./organizational-scroll";

describe("organizationalScrollFollowsEnd", () => {
  it("follows only while the reader remains near the end", () => {
    expect(organizationalScrollFollowsEnd({ scrollHeight: 1_000, clientHeight: 400, scrollTop: 600 })).toBe(true);
    expect(organizationalScrollFollowsEnd({ scrollHeight: 1_000, clientHeight: 400, scrollTop: 580 })).toBe(true);
    expect(organizationalScrollFollowsEnd({ scrollHeight: 1_000, clientHeight: 400, scrollTop: 500 })).toBe(false);
  });

  it("treats a surface without overflow as following", () => {
    expect(organizationalScrollFollowsEnd({ scrollHeight: 300, clientHeight: 400, scrollTop: 0 })).toBe(true);
  });
});
