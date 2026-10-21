import { describe, expect, it } from "vitest";
import { urlReach } from "./loopback";

describe("urlReach", () => {
  it("treats localhost, the whole 127.0.0.0/8 block, and IPv6 loopback as loopback", () => {
    for (const value of [
      "http://localhost:3000/",
      "http://LocalHost/",
      "http://127.0.0.2:8080/",
      "http://127.255.255.254/",
      "http://2130706433/",
      "http://[::1]:5173/",
    ]) {
      expect(urlReach(value), value).toBe("loopback");
    }
  });

  it("treats look-alike names and other addresses as external", () => {
    for (const value of [
      "http://localhost.example.com/",
      "http://app.localhost:3000/",
      "http://128.0.0.1/",
      "http://[::ffff:127.0.0.1]/",
      "http://0.0.0.0:3000/",
      "https://example.com/",
    ]) {
      expect(urlReach(value), value).toBe("external");
    }
  });

  it("reports text that does not parse as invalid", () => {
    expect(urlReach("http://")).toBe("invalid");
    expect(urlReach("not a url")).toBe("invalid");
  });
});
