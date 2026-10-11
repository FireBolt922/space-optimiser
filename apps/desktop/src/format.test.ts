import { describe, expect, it } from "vitest";
import { formatCount, formatDuration, plural } from "./format";

describe("format", () => {
  it("groups thousands in counts", () => {
    expect(formatCount(2_203_944)).toBe("2,203,944");
    expect(plural(1_203_836, "file", "files")).toBe("1,203,836 files");
    expect(plural(1, "file", "files")).toBe("1 file");
  });

  it("says durations in words", () => {
    expect(formatDuration(240)).toBe("less than a second");
    expect(formatDuration(1_000)).toBe("1 second");
    expect(formatDuration(12_900)).toBe("12 seconds");
    expect(formatDuration(60_000)).toBe("1 minute");
    expect(formatDuration(90_900)).toBe("1 minute 30 seconds");
    expect(formatDuration(3_780_000)).toBe("1 hour 3 minutes");
    expect(formatDuration(7_200_000)).toBe("2 hours");
  });
});
