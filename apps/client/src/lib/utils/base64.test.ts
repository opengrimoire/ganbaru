import { describe, expect, it } from "vitest";
import { encodeBase64 } from "./base64";

function decode(text: string): Uint8Array {
  return Uint8Array.from(atob(text), (character) => character.charCodeAt(0));
}

describe("encodeBase64", () => {
  it("encodes empty input and pads short input", () => {
    expect(encodeBase64(new Uint8Array())).toBe("");
    expect(encodeBase64(new Uint8Array([0x89]))).toBe("iQ==");
    expect(encodeBase64(new Uint8Array([0x89, 0x50, 0x4e, 0x47]))).toBe("iVBORw==");
  });

  it("round-trips every byte value across chunk boundaries", () => {
    const bytes = Uint8Array.from({ length: 0x8000 * 2 + 3 }, (_, index) => (index * 7) % 256);
    const encoded = encodeBase64(bytes);
    expect(encoded).toHaveLength(Math.ceil(bytes.length / 3) * 4);
    expect(decode(encoded)).toEqual(bytes);
  });
});
