/**
 * Returns whether a tab URL is a web page the blocker evaluates. Browser, extension, and
 * new-tab pages use other schemes, so they are never blocked.
 *
 * @param {unknown} url Tab or navigation URL.
 * @returns {boolean} True for `http:` and `https:` URLs.
 */
export function isSupportedPageUrl(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

/**
 * Returns the lowercase host of a page URL.
 *
 * @param {string} url Page URL.
 * @returns {string | null} The host, or null when the URL does not parse.
 */
export function hostFromUrl(url) {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return null;
  }
}

/**
 * Reduces a page URL to the origin and path that blocking rules read, so the query string,
 * fragment, and any credentials never reach the native host.
 *
 * @param {string} url Page URL.
 * @returns {string | null} The origin followed by the path, or null when the URL does not parse.
 */
export function decisionUrl(url) {
  try {
    const parsed = new URL(url);
    return `${parsed.origin}${parsed.pathname}`;
  } catch {
    return null;
  }
}
