import { describe, expect, it } from "vitest";
import { audioType, base64Of, extensionFor, recordingName } from "./recorder";

describe("recordings", () => {
  it("are named by when they were made", () => {
    expect(recordingName(new Date(2026, 9, 13, 5, 30, 7))).toBe("Recording 2026-10-13 05.30.07");
  });

  it("take the extension of what was recorded, and play as it", () => {
    expect(extensionFor("audio/webm;codecs=opus")).toBe("webm");
    expect(extensionFor("audio/mp4")).toBe("m4a");
    expect(extensionFor("audio/ogg")).toBe("ogg");
    expect(audioType("Recordings/Lecture.M4A")).toBe("audio/mp4");
    expect(audioType("x.unknown")).toBe("audio/*");
  });

  it("go to the app in base64", async () => {
    expect(await base64Of(new Blob([new Uint8Array([104, 105])]))).toBe("aGk=");
  });
});
