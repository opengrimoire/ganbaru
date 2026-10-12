import { describe, expect, it } from "vitest";
import {
  musicFolderDisplayName,
  musicFolderRelationship,
  normalizeComparableFolderPath,
} from "./drafts";

describe("music source drafts", () => {
  it("derives friendly names across platform separators", () => {
    expect(musicFolderDisplayName("/home/user/Music/Soundtracks/")).toBe("Soundtracks");
    expect(musicFolderDisplayName("C:\\Music\\Game OST\\")).toBe("Game OST");
  });

  it("normalizes separators and case for device-local overlap checks", () => {
    expect(normalizeComparableFolderPath("C:\\Music\\OST\\")).toBe("c:/music/ost");
    expect(musicFolderRelationship("C:\\Music\\OST", ["c:/music/ost/"])).toBe("duplicate");
    expect(musicFolderRelationship("/music/ost/game", ["/music/ost"])).toBe("nested");
    expect(musicFolderRelationship("/music", ["/music/ost"])).toBe("contains-existing");
    expect(musicFolderRelationship("/audio", ["/music/ost"])).toBe("separate");
  });
});
