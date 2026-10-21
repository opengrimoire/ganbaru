/**
 * Loopback host rule shared with the native browser preview: the exact name `localhost`, an IPv4
 * address in `127.0.0.0/8`, or `::1`. Hostnames come from the WHATWG `URL` parser, which
 * lowercases names, normalizes numeric IPv4 spellings, and keeps IPv6 brackets.
 */
export function isLoopbackHostname(hostname: string): boolean {
  return hostname === "localhost" || hostname === "[::1]" || /^127(?:\.\d{1,3}){3}$/.test(hostname);
}

/**
 * Classifies a URL string for navigation confirmation: `loopback`, `external`, or `invalid` when it does not parse
 * (Rust reports why).
 */
export function urlReach(value: string): "loopback" | "external" | "invalid" {
  let hostname: string;
  try {
    hostname = new URL(value).hostname;
  } catch {
    return "invalid";
  }
  return isLoopbackHostname(hostname) ? "loopback" : "external";
}
