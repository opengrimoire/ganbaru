/**
 * Preview page hook, installed as an initialization script so it runs before any page script.
 *
 * It captures the intrinsics and DOM getters that snapshots and waits need, then defines one
 * frozen, non-configurable object on `window` under a name chosen at random for each app run.
 * Later page scripts can patch globals and prototypes, but cannot change what this hook calls,
 * so they cannot inflate a result past its bound or make it throw past the size check.
 *
 * @param {{name: string, resultBudgetBytes: number, fieldChars: number, urlChars: number, titleChars: number, fitRounds: number}} config
 */
(config) => {
  const apply = Reflect.apply;
  const createObject = Object.create;
  const defineProperty = Object.defineProperty;
  const freeze = Object.freeze;
  const getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
  const getPrototypeOf = Object.getPrototypeOf;
  const stringify = JSON.stringify;
  const slice = String.prototype.slice;
  const encoder = new TextEncoder();
  const encode = TextEncoder.prototype.encode;
  const getter = (target, key) => getOwnPropertyDescriptor(target, key).get;
  const byteLengthOf = getter(getPrototypeOf(Uint8Array.prototype), "byteLength");
  const documentRef = document;
  const locationRef = location;
  const hrefOf = getter(locationRef, "href");
  const titleOf = getter(Document.prototype, "title");
  const bodyOf = getter(Document.prototype, "body");
  const rootOf = getter(Document.prototype, "documentElement");
  const innerTextOf = getter(HTMLElement.prototype, "innerText");
  const outerHtmlOf = getter(Element.prototype, "outerHTML");
  const querySelector = Document.prototype.querySelector;
  const { name, resultBudgetBytes, fieldChars, urlChars, titleChars, fitRounds } = config;

  const bounded = (value, limit) => apply(slice, value, [0, limit]);
  const half = (value) => bounded(value, (value.length / 2) | 0);
  /** UTF-8 size of `value` after the engine encodes the returned string as JSON again. */
  const resultBytes = (value) =>
    apply(byteLengthOf, apply(encode, encoder, [apply(stringify, null, [value])]), []);

  const snapshot = () => {
    const body = apply(bodyOf, documentRef, []);
    const root = apply(rootOf, documentRef, []);
    const result = createObject(null);
    result.url = bounded(apply(hrefOf, locationRef, []), urlChars);
    result.title = bounded(apply(titleOf, documentRef, []), titleChars);
    let text = body === null ? "" : bounded(apply(innerTextOf, body, []), fieldChars);
    let html = root === null ? "" : bounded(apply(outerHtmlOf, root, []), fieldChars);
    result.truncated = false;
    for (let round = 0; ; round += 1) {
      result.text = text;
      result.html = html;
      const encoded = apply(stringify, null, [result]);
      if (resultBytes(encoded) <= resultBudgetBytes) return encoded;
      if (text.length === 0 && html.length === 0) return null;
      if (round >= fitRounds) {
        text = "";
        html = "";
      } else if (html.length > 0) {
        html = half(html);
      } else {
        text = half(text);
      }
      result.truncated = true;
    }
  };

  const matches = (selector) => {
    try {
      return apply(querySelector, documentRef, [selector]) === null ? "absent" : "matched";
    } catch {
      return "invalid";
    }
  };

  const hook = createObject(null);
  defineProperty(hook, "snapshot", { value: snapshot });
  defineProperty(hook, "matches", { value: matches });
  defineProperty(window, name, { value: freeze(hook) });
}
