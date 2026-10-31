import { describe, expect, it } from "vitest";
import { isManagedMusicArtworkReference } from "./managed-artwork";

const digest = "0123456789abcdef".repeat(4);

describe("isManagedMusicArtworkReference", () => {
  it("accepts the reference of every stored artwork type", () => {
    for (const extension of ["png", "jpg", "webp", "gif"]) {
      expect(isManagedMusicArtworkReference(`music/artwork/${digest}.${extension}`)).toBe(true);
    }
  });

  it("refuses device paths, other classes, and other spellings", () => {
    for (const value of [
      null,
      undefined,
      "",
      "/Music/custom.jpg",
      "C:\\Music\\cover.png",
      "content://media/external/images/1",
      `quick-notes/images/${digest}.png`,
      `music/artwork/${digest}.jpeg`,
      `music/artwork/${digest}.PNG`,
      `music/artwork/${digest}.bmp`,
      `music/artwork/${digest.toUpperCase()}.png`,
      `music/artwork/${digest.slice(1)}.png`,
      `music/artwork/../${digest}.png`,
      `music/artwork/sub/${digest}.png`,
      ` music/artwork/${digest}.png`,
      `music/artwork/${digest}.png\n`,
    ]) {
      expect(isManagedMusicArtworkReference(value)).toBe(false);
    }
  });
});
